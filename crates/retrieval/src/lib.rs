//! Storage-neutral semantic candidate generation over Lean feature rows.
//!
//! This crate consumes the opaque declaration and proof-goal feature rows
//! defined by `lean-semantic-search-contract` and produces a ranked set of
//! candidate declarations for an anchor. It hides every retrieval decision —
//! role and rarity weighting, broad-head pruning, posting fanout limits, and
//! bounded top-k selection — behind a small surface: build an index, build an
//! anchor, retrieve.
//!
//! Callers see ranked candidates explained in stable feature-family terms and
//! structured diagnostics. They never see postings, heaps, scores, weights, or
//! raw feature keys: the only keys the crate touches are the opaque equality
//! tokens it ingests from the contract rows. Proof-goal retrieval starts from
//! source-backed feature rows, the same rows the Lean extractor emits from
//! elaborated expressions, never from rendered goal text.
//!
//! ```no_run
//! use lean_semantic_search_retrieval::{Anchor, SemanticIndex, retrieve_across};
//! # use lean_semantic_search_contract::{DeclarationFeatureRow, ProofGoalFeatureRow};
//! # fn demo(corpus: &[DeclarationFeatureRow], goal: &ProofGoalFeatureRow) {
//! let index = SemanticIndex::from_declarations(corpus);
//! let retrieval = retrieve_across(&[&index], &Anchor::from_proof_goal(goal), 20);
//! for candidate in &retrieval.candidates {
//!     println!("{} (rank {})", candidate.declaration_id, candidate.rank);
//! }
//! # }
//! ```

mod corpus;
mod explain;
mod index;
mod plan;
mod policy;
mod select;

pub use corpus::Corpus;
pub use index::SemanticIndex;
pub use plan::Anchor;

/// Rank one anchor against a slice of corpora, merged into a single bounded,
/// ranked candidate list.
///
/// Single-corpus retrieval is the one-element case:
/// `retrieve_across(&[&index], &anchor, limit)`. Each corpus weights by its own
/// document total and fanout; a candidate found in more than one corpus merges
/// by `declaration_id`. Corpus identity stays with the caller — the result names
/// only candidates, ranks, and feature families.
#[must_use]
pub fn retrieve_across(corpora: &[&dyn Corpus], anchor: &Anchor, limit: usize) -> Retrieval {
    corpus::rank(corpora, anchor, limit)
}

// Re-export the shared diagnostic so callers can read retrieval diagnostics
// without taking a separate dependency on the contract crate.
pub use lean_semantic_search_contract::Diagnostic;

use serde::{Deserialize, Serialize};

/// Identity of the active retrieval policy. Ranks and family contributions are
/// comparable only between retrievals that report the same version.
pub const RETRIEVAL_POLICY_VERSION: &str = policy::POLICY_VERSION;

/// The outcome of one retrieval: rank-ordered candidates and diagnostics.
///
/// `policy_version` pins the calibration that produced the ranking, so callers
/// can refuse to compare results across policy changes. Diagnostics describe
/// pruning and saturation in stable, key-free terms.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Retrieval {
    /// Calibration identity; always [`RETRIEVAL_POLICY_VERSION`] for this build.
    pub policy_version: String,
    /// Candidates ordered best-first and bounded by the requested limit.
    pub candidates: Vec<Candidate>,
    /// Structured retrieval diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// One retrieved candidate declaration.
///
/// `rank` is 1-based and reflects the hidden rarity-weighted score; a lower rank
/// is a stronger match. `explanations` say why the candidate matched using
/// stable feature families, never raw feature keys.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Candidate {
    /// Stable declaration identifier carried over from the candidate row.
    pub declaration_id: String,
    /// 1-based rank; lower is stronger.
    pub rank: u32,
    /// Why this candidate matched, grouped by feature family.
    pub explanations: Vec<MatchExplanation>,
}

/// One feature family's contribution to a candidate match.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MatchExplanation {
    /// The feature family that matched.
    pub family: FeatureFamily,
    /// How many of the anchor's features in this family matched the candidate.
    pub match_count: u32,
}

/// A stable, opaque-key-free label for a family of matched features.
///
/// Families are how callers reason about *why* a candidate matched without ever
/// touching the encoding of a feature key. Head roles intentionally collapse to
/// a single family; the weighting that still separates them is private.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FeatureFamily {
    /// Full statement fingerprint.
    StatementFingerprint,
    /// Binder-permutation-safe statement fingerprint.
    SafePermutationFingerprint,
    /// Connective-normalized statement fingerprint.
    ConnectiveFingerprint,
    /// Connective-normalized conclusion fingerprint.
    ConclusionFingerprint,
    /// Constant appearing in the conclusion.
    RoleConclusionConst,
    /// Constant appearing in a hypothesis.
    RoleHypothesisConst,
    /// Functor head of a conclusion, hypothesis, or binder domain.
    RoleHead,
    /// Any role the current policy does not name specifically.
    RoleOther,
}

impl FeatureFamily {
    /// The stable wire label for this family, matching its serialized form.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::StatementFingerprint => "statement_fingerprint",
            Self::SafePermutationFingerprint => "safe_permutation_fingerprint",
            Self::ConnectiveFingerprint => "connective_fingerprint",
            Self::ConclusionFingerprint => "conclusion_fingerprint",
            Self::RoleConclusionConst => "role_conclusion_const",
            Self::RoleHypothesisConst => "role_hypothesis_const",
            Self::RoleHead => "role_head",
            Self::RoleOther => "role_other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FeatureFamily;

    #[test]
    fn label_matches_serialized_form() -> Result<(), String> {
        let families = [
            FeatureFamily::StatementFingerprint,
            FeatureFamily::SafePermutationFingerprint,
            FeatureFamily::ConnectiveFingerprint,
            FeatureFamily::ConclusionFingerprint,
            FeatureFamily::RoleConclusionConst,
            FeatureFamily::RoleHypothesisConst,
            FeatureFamily::RoleHead,
            FeatureFamily::RoleOther,
        ];
        for family in families {
            let serialized = serde_json::to_string(&family).map_err(|error| error.to_string())?;
            assert_eq!(serialized, format!("\"{}\"", family.label()));
        }
        Ok(())
    }
}
