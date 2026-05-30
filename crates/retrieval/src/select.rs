//! Bounded, multi-lane selection.
//!
//! Retrieval accumulates two sub-scores for every candidate that matched an
//! admitting key: a fingerprint/statement score and a role/binder score. A
//! single combined top-k would let the heavier fingerprint families crowd
//! selective role matches out of the bound, so selection bounds each lane on its
//! own and unions the survivors. Within a lane the worst kept candidate sits at
//! the top of a min-heap, ready to be evicted when something better arrives; the
//! float scores are quantized to integer micros so ordering is total and
//! deterministic, and ties break on declaration id. The scores never leave this
//! crate — callers see only the resulting rank order and per-lane saturation
//! counts.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap, HashSet};

use crate::FeatureFamily;
use crate::policy::Lane;

/// One scored candidate entering selection. `fp_score` and `role_score` are the
/// per-lane sums; their total drives the final rank order.
pub(crate) struct Scored {
    pub(crate) declaration_id: String,
    pub(crate) fp_score: f64,
    pub(crate) role_score: f64,
    pub(crate) families: BTreeMap<FeatureFamily, u32>,
}

impl Scored {
    fn total(&self) -> f64 {
        self.fp_score + self.role_score
    }

    fn lane_score(&self, lane: Lane) -> f64 {
        match lane {
            Lane::FingerprintStatement => self.fp_score,
            Lane::RoleBinder => self.role_score,
        }
    }
}

/// How many candidates one lane dropped beyond its bound.
pub(crate) struct LaneSaturation {
    pub(crate) lane: Lane,
    pub(crate) dropped: usize,
}

/// Bound each recall lane to `limit` and union the survivors, returned ranked
/// best-first by total score (ties break on the smaller declaration id). A
/// candidate kept by either lane appears once. Also returns, per lane, how many
/// candidates that lane dropped beyond its bound.
pub(crate) fn select_lanes(scored: Vec<Scored>, limit: usize) -> (Vec<Scored>, Vec<LaneSaturation>) {
    let mut kept: HashSet<usize> = HashSet::new();
    let mut saturations = Vec::new();

    for lane in Lane::ALL {
        let eligible: Vec<usize> = scored
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.lane_score(lane) > 0.0)
            .map(|(index, _)| index)
            .collect();
        let dropped = lane_top_k(&scored, &eligible, lane, limit, &mut kept);
        if dropped > 0 {
            saturations.push(LaneSaturation { lane, dropped });
        }
    }

    let mut survivors: Vec<Scored> = scored
        .into_iter()
        .enumerate()
        .filter_map(|(index, candidate)| kept.contains(&index).then_some(candidate))
        .collect();
    survivors.sort_unstable_by(rank_order);
    (survivors, saturations)
}

/// Mark the best `limit` of `eligible` (ranked by lane score) as kept, returning
/// how many were dropped beyond the bound.
fn lane_top_k(scored: &[Scored], eligible: &[usize], lane: Lane, limit: usize, kept: &mut HashSet<usize>) -> usize {
    if limit == 0 {
        return eligible.len();
    }

    let mut heap: BinaryHeap<Reverse<LaneEntry<'_>>> = BinaryHeap::new();
    for &index in eligible {
        let Some(candidate) = scored.get(index) else {
            continue;
        };
        let entry = LaneEntry {
            score_micros: micros(candidate.lane_score(lane)),
            declaration_id: &candidate.declaration_id,
            index,
        };
        if heap.len() < limit {
            heap.push(Reverse(entry));
        } else if let Some(Reverse(worst)) = heap.peek()
            && entry > *worst
        {
            heap.pop();
            heap.push(Reverse(entry));
        }
    }

    let dropped = eligible.len().saturating_sub(heap.len());
    for Reverse(entry) in heap {
        kept.insert(entry.index);
    }
    dropped
}

/// Final rank order over the unioned survivors: higher total first, smaller
/// declaration id first on a tie.
fn rank_order(left: &Scored, right: &Scored) -> Ordering {
    micros(right.total())
        .cmp(&micros(left.total()))
        .then_with(|| left.declaration_id.cmp(&right.declaration_id))
}

/// One candidate inside a single lane's bounded selection.
struct LaneEntry<'a> {
    score_micros: i64,
    declaration_id: &'a str,
    index: usize,
}

impl PartialEq for LaneEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.score_micros == other.score_micros && self.declaration_id == other.declaration_id
    }
}

impl Eq for LaneEntry<'_> {}

impl Ord for LaneEntry<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        // "Greater" means "stronger candidate": higher score wins, and on a tie
        // the lexicographically smaller declaration id wins (so it ranks first
        // and survives eviction).
        self.score_micros
            .cmp(&other.score_micros)
            .then_with(|| other.declaration_id.cmp(self.declaration_id))
    }
}

impl PartialOrd for LaneEntry<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn micros(score: f64) -> i64 {
    (score * 1_000_000.0).round() as i64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Scored, select_lanes};
    use crate::policy::Lane;

    fn fingerprint(id: &str, score: f64) -> Scored {
        Scored {
            declaration_id: id.to_owned(),
            fp_score: score,
            role_score: 0.0,
            families: BTreeMap::new(),
        }
    }

    fn role(id: &str, score: f64) -> Scored {
        Scored {
            declaration_id: id.to_owned(),
            fp_score: 0.0,
            role_score: score,
            families: BTreeMap::new(),
        }
    }

    fn ids(kept: &[Scored]) -> Vec<&str> {
        kept.iter().map(|entry| entry.declaration_id.as_str()).collect()
    }

    #[test]
    fn keeps_best_within_a_single_lane_and_reports_overflow() {
        let input = vec![role("a", 1.0), role("b", 3.0), role("c", 2.0)];
        let (kept, saturations) = select_lanes(input, 2);
        assert_eq!(ids(&kept), vec!["b", "c"]);
        let dropped: Vec<(Lane, usize)> = saturations.iter().map(|s| (s.lane, s.dropped)).collect();
        assert_eq!(dropped, vec![(Lane::RoleBinder, 1)]);
    }

    #[test]
    fn ties_break_on_declaration_id() {
        let input = vec![role("beta", 5.0), role("alpha", 5.0)];
        let (kept, saturations) = select_lanes(input, 5);
        assert_eq!(ids(&kept), vec!["alpha", "beta"]);
        assert!(saturations.is_empty());
    }

    #[test]
    fn zero_limit_drops_everything_without_panicking() {
        let input = vec![role("a", 1.0)];
        let (kept, saturations) = select_lanes(input, 0);
        assert!(kept.is_empty());
        let dropped: Vec<usize> = saturations.iter().map(|s| s.dropped).collect();
        assert_eq!(dropped, vec![1]);
    }

    #[test]
    fn role_lane_rescues_a_match_a_fingerprint_cohort_would_evict() {
        // Three fingerprint candidates outscore the lone role candidate on total,
        // so a single combined top-2 would evict it. The role lane keeps it.
        let input = vec![
            fingerprint("fp-a", 100.0),
            fingerprint("fp-b", 90.0),
            fingerprint("fp-c", 80.0),
            role("role-only", 18.0),
        ];
        let (kept, _saturations) = select_lanes(input, 2);
        assert!(
            ids(&kept).contains(&"role-only"),
            "the role lane must rescue role-only: {:?}",
            ids(&kept)
        );
    }
}
