//! Anchor planning: turning a feature row into the weighted keys a retrieval
//! looks up.
//!
//! An [`Anchor`] is the query side of retrieval. Declaration rows and
//! source-backed proof-goal rows share the same semantic facts — fingerprints,
//! role features, and low-signal markers — so both become the same internal
//! plan. The plan records, per key, how much a match is worth, how broad a
//! posting may be before it is pruned, and whether the key is selective enough
//! to admit a candidate on its own. Broad-head status is decided here from the
//! anchor's own low-signal markers, never from the corpus.

use std::collections::HashSet;

use lean_semantic_search_contract::{
    DeclarationFeatureRow, Fingerprints, OpaqueFeatureKey, ProofGoalFeatureRow, RoleFeature,
};

use crate::FeatureFamily;
use crate::explain::family_for_role;
use crate::policy;

/// One weighted lookup key derived from an anchor. The key stays an opaque
/// equality token end to end — it is what a [`Corpus`](crate::Corpus) is asked
/// for, never an encoding the crate inspects.
pub(crate) struct PlannedKey {
    pub(crate) key: OpaqueFeatureKey,
    pub(crate) family: FeatureFamily,
    pub(crate) base_weight: f64,
    pub(crate) posting_limit: usize,
    pub(crate) admits_candidate: bool,
}

/// A retrieval query built from one declaration or proof-goal feature row.
///
/// An anchor is the weighted set of semantic keys to look up. Build one with
/// [`Anchor::from_declaration`] or [`Anchor::from_proof_goal`], then pass it to
/// [`SemanticIndex::retrieve`](crate::SemanticIndex::retrieve).
pub struct Anchor {
    keys: Vec<PlannedKey>,
}

impl Anchor {
    /// Plan retrieval from a declaration feature row.
    #[must_use]
    pub fn from_declaration(row: &DeclarationFeatureRow) -> Self {
        Self::build(&row.fingerprints, &row.role_features, &row.low_signal_markers)
    }

    /// Plan retrieval from a source-backed proof-goal feature row.
    #[must_use]
    pub fn from_proof_goal(row: &ProofGoalFeatureRow) -> Self {
        Self::build(&row.fingerprints, &row.role_features, &row.low_signal_markers)
    }

    pub(crate) fn planned_keys(&self) -> &[PlannedKey] {
        &self.keys
    }

    fn build(fingerprints: &Fingerprints, role_features: &[RoleFeature], low_signal_markers: &[String]) -> Self {
        let broad_heads = broad_head_displays(low_signal_markers);
        let mut keys = Vec::with_capacity(role_features.len().saturating_add(4));
        push_fingerprints(fingerprints, &mut keys);
        push_role_features(role_features, &broad_heads, &mut keys);
        Self { keys }
    }
}

fn broad_head_displays(low_signal_markers: &[String]) -> HashSet<&str> {
    low_signal_markers
        .iter()
        .filter_map(|marker| marker.strip_prefix("broad_head:"))
        .collect()
}

fn push_fingerprints(fingerprints: &Fingerprints, out: &mut Vec<PlannedKey>) {
    let entries = [
        (
            &fingerprints.statement,
            FeatureFamily::StatementFingerprint,
            policy::STATEMENT_WEIGHT,
        ),
        (
            &fingerprints.safe_binder_permutation,
            FeatureFamily::SafePermutationFingerprint,
            policy::SAFE_PERMUTATION_WEIGHT,
        ),
        (
            &fingerprints.connective_shape,
            FeatureFamily::ConnectiveFingerprint,
            policy::CONNECTIVE_WEIGHT,
        ),
        (
            &fingerprints.conclusion_shape,
            FeatureFamily::ConclusionFingerprint,
            policy::CONCLUSION_WEIGHT,
        ),
    ];
    for (key, family, base_weight) in entries {
        out.push(PlannedKey {
            key: key.clone(),
            family,
            base_weight,
            // Exact structural keys are selective by construction; never pruned.
            posting_limit: usize::MAX,
            admits_candidate: true,
        });
    }
}

fn push_role_features(role_features: &[RoleFeature], broad_heads: &HashSet<&str>, out: &mut Vec<PlannedKey>) {
    for feature in role_features {
        let broad_head = feature
            .display
            .as_deref()
            .is_some_and(|display| broad_heads.contains(display));
        let posting_limit = if broad_head {
            policy::BROAD_HEAD_POSTING_LIMIT
        } else {
            policy::ROLE_POSTING_LIMIT
        };
        out.push(PlannedKey {
            key: feature.key.clone(),
            family: family_for_role(&feature.role),
            base_weight: policy::role_weight(&feature.role, broad_head),
            posting_limit,
            // A broad head alone is too common to admit a candidate.
            admits_candidate: !broad_head,
        });
    }
}

#[cfg(test)]
mod tests {
    use lean_semantic_search_contract::{OpaqueFeatureKey, RoleFeature};

    use super::{broad_head_displays, push_role_features};
    use crate::policy;

    fn role(role: &str, key: &str, display: &str) -> RoleFeature {
        RoleFeature {
            role: role.to_owned(),
            key: OpaqueFeatureKey::new(key),
            display: Some(display.to_owned()),
        }
    }

    #[test]
    fn broad_head_role_is_capped_and_does_not_admit() -> Result<(), String> {
        let markers = vec!["broad_head:Eq".to_owned()];
        let broad_heads = broad_head_displays(&markers);
        let mut planned = Vec::new();
        push_role_features(&[role("conclusion_head", "k", "Eq")], &broad_heads, &mut planned);
        let entry = planned.first().ok_or_else(|| "expected one planned key".to_owned())?;
        assert_eq!(entry.posting_limit, policy::BROAD_HEAD_POSTING_LIMIT);
        assert!(!entry.admits_candidate);
        Ok(())
    }

    #[test]
    fn ordinary_role_uses_role_limit_and_admits() -> Result<(), String> {
        let broad_heads = broad_head_displays(&[]);
        let mut planned = Vec::new();
        push_role_features(&[role("conclusion_const", "k", "Foo")], &broad_heads, &mut planned);
        let entry = planned.first().ok_or_else(|| "expected one planned key".to_owned())?;
        assert_eq!(entry.posting_limit, policy::ROLE_POSTING_LIMIT);
        assert!(entry.admits_candidate);
        Ok(())
    }
}
