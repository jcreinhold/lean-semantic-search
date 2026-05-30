//! Private retrieval policy: the weights, rarity curve, fanout limits, and
//! recall lanes that decide how anchor features score, which postings are too
//! broad to expand, and which matches a bounded selection must not crowd out.
//!
//! These calibration choices are the crate's hidden decision. Nothing here
//! reaches the public surface except indirectly, as a rank ordering or a
//! key-free diagnostic. Changing a value here is a policy change and must move
//! `POLICY_VERSION` with it.

use crate::FeatureFamily;

/// Identity of the active retrieval policy. Ranks are comparable only between
/// retrievals that report the same version.
///
/// `v2` folds the multi-lane recall guarantee into shared policy: a bounded
/// selection now bounds a fingerprint/statement lane and a role/binder lane
/// separately and unions them, so a selective role match is not crowded out
/// behind a fingerprint cohort. The bump lets a downstream consumer stop
/// reimplementing candidate selection.
pub(crate) const POLICY_VERSION: &str = "lean-semantic-search.retrieval.v2";

/// A recall lane: a slice of feature families bounded on its own so a single
/// combined top-k cannot evict every match of one kind behind another. Ranks
/// from one lane say nothing about the other; selection unions the two.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Lane {
    /// Structural fingerprint families: full statement down to conclusion shape.
    FingerprintStatement,
    /// Role-aware families: conclusion/hypothesis constants, heads, binders.
    RoleBinder,
}

impl Lane {
    /// Both lanes, in a stable order.
    pub(crate) const ALL: [Self; 2] = [Self::FingerprintStatement, Self::RoleBinder];

    /// Stable, key-free identity recorded in saturation diagnostics. Never an
    /// encoding of a feature key.
    pub(crate) fn identity(self) -> &'static str {
        match self {
            Self::FingerprintStatement => "fingerprint_statement",
            Self::RoleBinder => "role_binder",
        }
    }
}

/// The recall lane a feature family contributes to. Fingerprint families weigh
/// far more than role families, so without separate lanes a fingerprint cohort
/// would crowd selective role matches out of any combined top-k.
pub(crate) fn lane_for_family(family: FeatureFamily) -> Lane {
    match family {
        FeatureFamily::StatementFingerprint
        | FeatureFamily::SafePermutationFingerprint
        | FeatureFamily::ConnectiveFingerprint
        | FeatureFamily::ConclusionFingerprint => Lane::FingerprintStatement,
        FeatureFamily::RoleConclusionConst
        | FeatureFamily::RoleHypothesisConst
        | FeatureFamily::RoleHead
        | FeatureFamily::RoleOther => Lane::RoleBinder,
    }
}

/// Posting length above which a non-broad role feature is too common to expand.
pub(crate) const ROLE_POSTING_LIMIT: usize = 512;

/// Posting length above which a broad-head role feature is too common to expand.
pub(crate) const BROAD_HEAD_POSTING_LIMIT: usize = 64;

// Fingerprint base weights, strongest structural evidence first. Fingerprints
// are never fanout-limited: an exact structural key is selective by construction.
pub(crate) const STATEMENT_WEIGHT: f64 = 100.0;
pub(crate) const SAFE_PERMUTATION_WEIGHT: f64 = 85.0;
pub(crate) const CONNECTIVE_WEIGHT: f64 = 65.0;
pub(crate) const CONCLUSION_WEIGHT: f64 = 45.0;

/// Base weight for one role feature given its role label and whether the Lean
/// extractor marked it a broad head. Conclusion evidence outweighs hypothesis
/// evidence, constants outweigh heads, and broad heads collapse to near zero so
/// a match on `Eq` or `And` alone cannot carry a candidate.
pub(crate) fn role_weight(role: &str, broad_head: bool) -> f64 {
    if broad_head {
        return 1.0;
    }
    match role {
        "conclusion_const" => 18.0,
        "hypothesis_const" => 10.0,
        "conclusion_head" => 8.0,
        "hypothesis_head" => 4.0,
        "binder_domain_head" => 3.0,
        _ => 2.0,
    }
}

/// Inverse-document-frequency rarity multiplier. A feature shared by few
/// candidates outweighs one shared by many. Floored at `1.0` so a match never
/// shrinks below its own base weight, and pinned to `1.0` when there is no
/// corpus to compare against.
pub(crate) fn rarity_weight(total_documents: usize, document_frequency: usize) -> f64 {
    if total_documents == 0 || document_frequency == 0 {
        return 1.0;
    }
    let total = total_documents as f64 + 1.0;
    let frequency = document_frequency as f64 + 1.0;
    (1.0 + (total / frequency).ln()).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::{rarity_weight, role_weight};

    #[test]
    fn rarity_rewards_selective_features() {
        let selective = rarity_weight(1000, 1);
        let broad = rarity_weight(1000, 900);
        assert!(
            selective > broad,
            "rarer features must weigh more: {selective} !> {broad}"
        );
    }

    #[test]
    fn rarity_is_floored_at_one() {
        assert!(rarity_weight(10, 10) >= 1.0);
        assert!((rarity_weight(0, 0) - 1.0).abs() < f64::EPSILON);
        assert!((rarity_weight(5, 0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn broad_heads_are_downweighted_below_any_real_role() {
        assert!(role_weight("conclusion_const", true) < role_weight("binder_domain_head", false));
        assert!(role_weight("conclusion_const", false) > role_weight("hypothesis_const", false));
    }
}
