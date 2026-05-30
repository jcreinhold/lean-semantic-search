//! Private mapping from Lean role labels to stable public feature families.
//!
//! Heads collapse to a single family on purpose: callers should reason about
//! "a head matched", not about which head encoding the Lean extractor used. The
//! per-role weighting that still distinguishes heads lives in `policy`, hidden
//! from the explanation surface.

use crate::FeatureFamily;

pub(crate) fn family_for_role(role: &str) -> FeatureFamily {
    match role {
        "conclusion_const" => FeatureFamily::RoleConclusionConst,
        "hypothesis_const" => FeatureFamily::RoleHypothesisConst,
        "conclusion_head" | "hypothesis_head" | "binder_domain_head" => FeatureFamily::RoleHead,
        _ => FeatureFamily::RoleOther,
    }
}

#[cfg(test)]
mod tests {
    use super::family_for_role;
    use crate::FeatureFamily;

    #[test]
    fn heads_collapse_to_a_single_family() {
        assert_eq!(family_for_role("conclusion_head"), FeatureFamily::RoleHead);
        assert_eq!(family_for_role("hypothesis_head"), FeatureFamily::RoleHead);
        assert_eq!(family_for_role("binder_domain_head"), FeatureFamily::RoleHead);
    }

    #[test]
    fn constants_keep_distinct_families() {
        assert_eq!(family_for_role("conclusion_const"), FeatureFamily::RoleConclusionConst);
        assert_eq!(family_for_role("hypothesis_const"), FeatureFamily::RoleHypothesisConst);
        assert_eq!(family_for_role("something_new"), FeatureFamily::RoleOther);
    }
}
