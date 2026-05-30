//! The in-memory semantic index and the retrieval it serves.
//!
//! [`SemanticIndex`] holds an inverted view of a candidate corpus: each opaque
//! feature key maps to the declarations that carry it, plus the document count
//! needed for rarity weighting. It is built entirely from declaration feature
//! rows held in memory; it owns no storage, no cache, and no on-disk layout, so
//! any caller can build one from rows it obtained however it likes.
//!
//! [`SemanticIndex::retrieve`] is the deep operation. It plans the anchor, walks
//! each key's postings, prunes postings that fan out too widely, scores the
//! survivors by base weight times rarity, drops candidates that matched only
//! broad heads, and keeps a bounded top-k. The postings, the document-frequency
//! table, the scores, and the heap all stay private; the result is ranked
//! candidates explained in feature-family terms and key-free diagnostics.

use std::collections::{BTreeMap, HashMap, HashSet};

use lean_semantic_search_contract::{DeclarationFeatureRow, Diagnostic, DiagnosticSeverity};
use serde_json::json;

use crate::plan::Anchor;
use crate::policy::{self, POLICY_VERSION};
use crate::select::{self, Scored};
use crate::{Candidate, FeatureFamily, MatchExplanation, Retrieval};

/// An in-memory semantic index over candidate declarations.
///
/// Build one with [`SemanticIndex::from_declarations`] and query it with an
/// [`Anchor`]. The index is storage-neutral: it is a view over rows the caller
/// already holds, never a database.
pub struct SemanticIndex {
    declaration_ids: Vec<String>,
    postings: HashMap<String, Vec<usize>>,
    total_documents: usize,
}

impl SemanticIndex {
    /// Build an index from candidate declaration feature rows. Each row's four
    /// fingerprints and every role-feature key become lookup keys; per key, a
    /// declaration is counted once.
    #[must_use]
    pub fn from_declarations(rows: &[DeclarationFeatureRow]) -> Self {
        let mut declaration_ids = Vec::with_capacity(rows.len());
        let mut postings: HashMap<String, Vec<usize>> = HashMap::new();

        for (index, row) in rows.iter().enumerate() {
            declaration_ids.push(row.declaration_id.clone());

            let mut keys: HashSet<&str> = HashSet::new();
            keys.insert(row.fingerprints.statement.as_str());
            keys.insert(row.fingerprints.safe_binder_permutation.as_str());
            keys.insert(row.fingerprints.connective_shape.as_str());
            keys.insert(row.fingerprints.conclusion_shape.as_str());
            for feature in &row.role_features {
                keys.insert(feature.key.as_str());
            }
            for key in keys {
                postings.entry(key.to_owned()).or_default().push(index);
            }
        }

        let total_documents = declaration_ids.len();
        Self {
            declaration_ids,
            postings,
            total_documents,
        }
    }

    /// Number of candidate declarations in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.declaration_ids.len()
    }

    /// Whether the index holds no candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declaration_ids.is_empty()
    }

    /// Retrieve the best `limit` candidates for an anchor.
    ///
    /// The ranking and the pruning are policy decisions hidden behind this call.
    /// Diagnostics report when a posting was pruned for fanning out too widely
    /// and when the bounded top-k saturated, both in stable, key-free terms.
    #[must_use]
    pub fn retrieve(&self, anchor: &Anchor, limit: usize) -> Retrieval {
        let mut diagnostics = Vec::new();
        let mut accumulators: HashMap<usize, Accumulator> = HashMap::new();

        for planned in anchor.planned_keys() {
            let Some(posting) = self.postings.get(planned.key.as_str()) else {
                continue;
            };
            let fanout = posting.len();
            if fanout > planned.posting_limit {
                diagnostics.push(posting_pruned(planned.family, fanout, planned.posting_limit));
                continue;
            }
            let weight = planned.base_weight * policy::rarity_weight(self.total_documents, fanout);
            for &candidate in posting {
                let accumulator = accumulators.entry(candidate).or_default();
                accumulator.score += weight;
                accumulator.admitted = accumulator.admitted || planned.admits_candidate;
                let count = accumulator.families.entry(planned.family).or_insert(0);
                *count = count.saturating_add(1);
            }
        }

        let scored: Vec<Scored> = accumulators
            .into_iter()
            .filter(|(_, accumulator)| accumulator.admitted)
            .filter_map(|(index, accumulator)| {
                self.declaration_ids.get(index).map(|declaration_id| Scored {
                    declaration_id: declaration_id.clone(),
                    score: accumulator.score,
                    families: accumulator.families,
                })
            })
            .collect();

        let (ranked, dropped) = select::top_k(scored, limit);
        if dropped > 0 {
            diagnostics.push(top_k_saturated(limit, dropped));
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
}

#[derive(Default)]
struct Accumulator {
    score: f64,
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

fn top_k_saturated(limit: usize, dropped: usize) -> Diagnostic {
    Diagnostic::new(
        DiagnosticSeverity::Warning,
        "retrieval.top_k_saturated",
        format!("retained {limit} candidates and dropped {dropped} beyond the limit"),
        Some(json!({
            "limit": limit,
            "dropped": dropped,
        })),
    )
}
