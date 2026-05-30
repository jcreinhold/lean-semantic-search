//! Bounded top-k selection.
//!
//! Retrieval accumulates a score for every candidate that matched an admitting
//! key, then keeps only the best `limit`. A min-heap of size `limit` does this
//! without sorting the whole match set and without ever materializing more than
//! the candidates that actually matched: the worst kept candidate sits at the
//! top, ready to be evicted when something better arrives. The float score is
//! quantized to integer micros so ordering is total and deterministic; ties
//! break on declaration id. The score itself never leaves this crate — callers
//! see only the resulting rank order and a saturation count.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap};

use crate::FeatureFamily;

/// One scored candidate entering selection.
pub(crate) struct Scored {
    pub(crate) declaration_id: String,
    pub(crate) score: f64,
    pub(crate) families: BTreeMap<FeatureFamily, u32>,
}

struct HeapEntry {
    score_micros: i64,
    scored: Scored,
}

impl HeapEntry {
    fn new(scored: Scored) -> Self {
        Self {
            score_micros: micros(scored.score),
            scored,
        }
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.score_micros == other.score_micros && self.scored.declaration_id == other.scored.declaration_id
    }
}

impl Eq for HeapEntry {}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // "Greater" means "stronger candidate": higher score wins, and on a tie
        // the lexicographically smaller declaration id wins (so it ranks first).
        self.score_micros
            .cmp(&other.score_micros)
            .then_with(|| other.scored.declaration_id.cmp(&self.scored.declaration_id))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Keep the best `limit` candidates, returning them ranked best-first along with
/// the number dropped beyond the limit (the saturation overflow).
pub(crate) fn top_k(scored: Vec<Scored>, limit: usize) -> (Vec<Scored>, usize) {
    let total = scored.len();
    if limit == 0 {
        return (Vec::new(), total);
    }

    let mut heap: BinaryHeap<Reverse<HeapEntry>> = BinaryHeap::new();
    for candidate in scored {
        let entry = HeapEntry::new(candidate);
        if heap.len() < limit {
            heap.push(Reverse(entry));
        } else if let Some(Reverse(worst)) = heap.peek()
            && entry > *worst
        {
            heap.pop();
            heap.push(Reverse(entry));
        }
    }

    let dropped = total.saturating_sub(heap.len());
    let mut kept: Vec<HeapEntry> = heap.into_iter().map(|Reverse(entry)| entry).collect();
    kept.sort_unstable_by(|left, right| right.cmp(left));
    let ranked = kept.into_iter().map(|entry| entry.scored).collect();
    (ranked, dropped)
}

fn micros(score: f64) -> i64 {
    (score * 1_000_000.0).round() as i64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Scored, top_k};

    fn scored(id: &str, score: f64) -> Scored {
        Scored {
            declaration_id: id.to_owned(),
            score,
            families: BTreeMap::new(),
        }
    }

    #[test]
    fn keeps_best_and_reports_overflow() {
        let input = vec![scored("a", 1.0), scored("b", 3.0), scored("c", 2.0)];
        let (kept, dropped) = top_k(input, 2);
        let ids: Vec<&str> = kept.iter().map(|entry| entry.declaration_id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn ties_break_on_declaration_id() {
        let input = vec![scored("beta", 5.0), scored("alpha", 5.0)];
        let (kept, dropped) = top_k(input, 5);
        let ids: Vec<&str> = kept.iter().map(|entry| entry.declaration_id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "beta"]);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn zero_limit_drops_everything_without_panicking() {
        let input = vec![scored("a", 1.0)];
        let (kept, dropped) = top_k(input, 0);
        assert!(kept.is_empty());
        assert_eq!(dropped, 1);
    }
}
