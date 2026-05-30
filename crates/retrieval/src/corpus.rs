//! The corpus seam and the ranking it serves.
//!
//! A [`Corpus`] is everything the ranking loop needs from a candidate source and
//! nothing more: the document total for rarity weighting, batched fanout counts
//! so pruning can judge selectivity without hydrating, the postings behind a
//! surviving key, and — only for using a corpus member as an anchor — that
//! member's feature row. The trait is expressible by an in-memory map and by a
//! per-key SQL query alike, so a persistent store can back retrieval without the
//! ranking algorithm learning where rows live.
//!
//! [`rank`] is the deep operation, written once and parameterized over a slice
//! of corpora. It plans the anchor, walks each key's postings in each corpus,
//! prunes postings that fan out too widely, scores survivors by base weight
//! times rarity, accumulates by `declaration_id` so a backend need hold no
//! contiguous row array, drops candidates that matched only broad heads, and
//! bounds each recall lane before unioning. Postings, document frequencies,
//! scores, and the per-lane heaps all stay private; the result is ranked
//! candidates explained in feature-family terms and key-free diagnostics.

use std::collections::{BTreeMap, HashMap};

use lean_semantic_search_contract::{DeclarationFeatureRow, Diagnostic, DiagnosticSeverity, OpaqueFeatureKey};
use serde_json::json;

use crate::plan::Anchor;
use crate::policy::{self, Lane, POLICY_VERSION};
use crate::select::{self, LaneSaturation, Scored};
use crate::{Candidate, FeatureFamily, MatchExplanation, Retrieval};

/// A candidate corpus retrieval can rank against.
///
/// The seam carries only the semantic index needed to generate candidates —
/// opaque-key postings, per-key fanout counts, the document total, and the
/// feature rows needed to rebuild an anchor. It never carries display or
/// hydration metadata, provenance meaning, labels, or audit policy: those stay
/// with the caller. Keys are opaque equality tokens throughout; the corpus is
/// asked for a key's matches, never asked to interpret a key.
pub trait Corpus {
    /// Total number of documents in the corpus, for rarity weighting.
    fn document_total(&self) -> usize;

    /// Match count for each key, returned aligned to `keys`. Batched so a SQL
    /// backend answers in one query rather than one per key, and so posting and
    /// broad-head pruning can judge a key's selectivity without hydrating any
    /// candidate.
    fn fanout(&self, keys: &[OpaqueFeatureKey]) -> Vec<usize>;

    /// Declaration ids carrying `key`, bounded by `limit` so a SQL backend can
    /// `LIMIT` the scan. Called only for keys that survived fanout pruning.
    fn postings(&self, key: &OpaqueFeatureKey, limit: usize) -> Vec<String>;

    /// Reconstruct the feature row of a corpus member so it can serve as an
    /// anchor — self-audit and corpus-versus-corpus. Returns `None` when the
    /// corpus holds no such declaration. Proof-goal anchors come from a live row
    /// and never call this.
    fn declaration_row(&self, declaration_id: &str) -> Option<DeclarationFeatureRow>;
}

/// Rank one anchor against a slice of corpora and merge the results into one
/// bounded, ranked candidate list. Single-corpus retrieval is the one-element
/// case. Each corpus weights by its own document total and fanout; candidates
/// that appear in more than one corpus merge by `declaration_id`, summing their
/// contributions. Corpus identity stays with the caller — a candidate carries
/// only its id, rank, and family explanations.
pub(crate) fn rank(corpora: &[&dyn Corpus], anchor: &Anchor, limit: usize) -> Retrieval {
    let mut diagnostics = Vec::new();
    let mut accumulators: HashMap<String, Accumulator> = HashMap::new();

    let planned = anchor.planned_keys();
    let keys: Vec<OpaqueFeatureKey> = planned.iter().map(|plan| plan.key.clone()).collect();

    for corpus in corpora {
        let total_documents = corpus.document_total();
        let fanouts = corpus.fanout(&keys);
        for (plan, fanout) in planned.iter().zip(fanouts) {
            if fanout == 0 {
                continue;
            }
            if fanout > plan.posting_limit {
                diagnostics.push(posting_pruned(plan.family, fanout, plan.posting_limit));
                continue;
            }
            let weight = plan.base_weight * policy::rarity_weight(total_documents, fanout);
            let lane = policy::lane_for_family(plan.family);
            for declaration_id in corpus.postings(&plan.key, plan.posting_limit) {
                let accumulator = accumulators.entry(declaration_id).or_default();
                match lane {
                    Lane::FingerprintStatement => accumulator.fp_score += weight,
                    Lane::RoleBinder => accumulator.role_score += weight,
                }
                accumulator.admitted = accumulator.admitted || plan.admits_candidate;
                let count = accumulator.families.entry(plan.family).or_insert(0);
                *count = count.saturating_add(1);
            }
        }
    }

    let scored: Vec<Scored> = accumulators
        .into_iter()
        .filter(|(_, accumulator)| accumulator.admitted)
        .map(|(declaration_id, accumulator)| Scored {
            declaration_id,
            fp_score: accumulator.fp_score,
            role_score: accumulator.role_score,
            families: accumulator.families,
        })
        .collect();

    let (ranked, saturations) = select::select_lanes(scored, limit);
    for LaneSaturation { lane, dropped } in saturations {
        diagnostics.push(top_k_saturated(lane, limit, dropped));
    }

    let candidates = ranked
        .into_iter()
        .enumerate()
        .map(|(position, candidate)| Candidate {
            declaration_id: candidate.declaration_id,
            rank: position.saturating_add(1) as u32,
            explanations: explanations(candidate.families),
        })
        .collect();

    Retrieval {
        policy_version: POLICY_VERSION.to_owned(),
        candidates,
        diagnostics,
    }
}

#[derive(Default)]
struct Accumulator {
    fp_score: f64,
    role_score: f64,
    admitted: bool,
    families: BTreeMap<FeatureFamily, u32>,
}

fn explanations(families: BTreeMap<FeatureFamily, u32>) -> Vec<MatchExplanation> {
    families
        .into_iter()
        .map(|(family, match_count)| MatchExplanation { family, match_count })
        .collect()
}

fn posting_pruned(family: FeatureFamily, fanout: usize, posting_limit: usize) -> Diagnostic {
    Diagnostic::new(
        DiagnosticSeverity::Warning,
        "retrieval.posting_pruned",
        format!(
            "pruned a {} posting whose fanout {fanout} exceeded the limit {posting_limit}",
            family.label()
        ),
        Some(json!({
            "feature_family": family.label(),
            "fanout": fanout,
            "posting_limit": posting_limit,
        })),
    )
}

fn top_k_saturated(lane: Lane, limit: usize, dropped: usize) -> Diagnostic {
    Diagnostic::new(
        DiagnosticSeverity::Warning,
        "retrieval.top_k_saturated",
        format!(
            "the {} lane retained {limit} candidates and dropped {dropped} beyond the limit",
            lane.identity()
        ),
        Some(json!({
            "lane": lane.identity(),
            "limit": limit,
            "dropped": dropped,
        })),
    )
}
