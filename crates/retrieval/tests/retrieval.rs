//! Portable behavior tests for storage-neutral semantic retrieval.
//!
//! These exercise the public surface only: build an index from synthetic
//! contract rows, retrieve from a declaration or proof-goal anchor, and assert
//! on ranks, family explanations, and diagnostics. They stand in for the
//! retrieval behavior that previously lived inside `lean-dup`, without any of
//! its storage or duplicate-audit concerns.

use lean_semantic_search_contract::{
    DeclarationFeatureRow, Fingerprints, OpaqueFeatureKey, ProofGoalFeatureRow, RoleFeature, SEMANTIC_FEATURE_VERSION,
};
use lean_semantic_search_retrieval::{Anchor, Candidate, FeatureFamily, Retrieval, SemanticIndex};

fn key(value: &str) -> OpaqueFeatureKey {
    OpaqueFeatureKey::new(value)
}

/// Build a distinct fingerprint set namespaced by `prefix`, so two rows share
/// fingerprints only when they share a prefix.
fn fingerprints(prefix: &str) -> Fingerprints {
    Fingerprints {
        statement: key(&format!("{prefix}-stmt")),
        safe_binder_permutation: key(&format!("{prefix}-safe")),
        connective_shape: key(&format!("{prefix}-conn")),
        conclusion_shape: key(&format!("{prefix}-concl")),
    }
}

fn role(role: &str, key_value: &str, display: &str) -> RoleFeature {
    RoleFeature {
        role: role.to_owned(),
        key: key(key_value),
        display: Some(display.to_owned()),
    }
}

fn declaration(id: &str, fingerprints: Fingerprints, role_features: Vec<RoleFeature>) -> DeclarationFeatureRow {
    declaration_with_markers(id, fingerprints, role_features, Vec::new())
}

fn declaration_with_markers(
    id: &str,
    fingerprints: Fingerprints,
    role_features: Vec<RoleFeature>,
    low_signal_markers: Vec<String>,
) -> DeclarationFeatureRow {
    DeclarationFeatureRow {
        declaration_id: id.to_owned(),
        feature_version: SEMANTIC_FEATURE_VERSION.to_owned(),
        fingerprints,
        role_features,
        binder_count: 0,
        low_signal_markers,
        source: None,
    }
}

fn proof_goal(id: &str, fingerprints: Fingerprints, role_features: Vec<RoleFeature>) -> ProofGoalFeatureRow {
    ProofGoalFeatureRow {
        goal_id: id.to_owned(),
        feature_version: SEMANTIC_FEATURE_VERSION.to_owned(),
        fingerprints,
        role_features,
        low_signal_markers: Vec::new(),
    }
}

fn find<'a>(retrieval: &'a Retrieval, id: &str) -> Option<&'a Candidate> {
    retrieval
        .candidates
        .iter()
        .find(|candidate| candidate.declaration_id == id)
}

fn match_count(candidate: &Candidate, family: FeatureFamily) -> u32 {
    candidate
        .explanations
        .iter()
        .find(|explanation| explanation.family == family)
        .map_or(0, |explanation| explanation.match_count)
}

fn diagnostic_dropped(retrieval: &Retrieval, code: &str) -> Option<u64> {
    retrieval
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == code)
        .and_then(|diagnostic| diagnostic.details.as_ref())
        .and_then(|details| details.get("dropped"))
        .and_then(serde_json::Value::as_u64)
}

#[test]
fn role_feature_matching_aggregates_contributions() -> Result<(), String> {
    let anchor_row = declaration(
        "anchor",
        fingerprints("anchor"),
        vec![
            role("conclusion_const", "cc1", "Foo"),
            role("conclusion_const", "cc2", "Bar"),
            role("hypothesis_const", "hc1", "Baz"),
        ],
    );
    let candidate = declaration(
        "cand",
        fingerprints("cand"),
        vec![
            role("conclusion_const", "cc1", "Foo"),
            role("conclusion_const", "cc2", "Bar"),
            role("hypothesis_const", "hc1", "Baz"),
        ],
    );

    let index = SemanticIndex::from_declarations(&[candidate]);
    let retrieval = index.retrieve(&Anchor::from_declaration(&anchor_row), 10);

    let found = find(&retrieval, "cand").ok_or_else(|| "expected candidate `cand`".to_owned())?;
    assert_eq!(match_count(found, FeatureFamily::RoleConclusionConst), 2);
    assert_eq!(match_count(found, FeatureFamily::RoleHypothesisConst), 1);
    Ok(())
}

#[test]
fn broad_head_only_match_is_not_admitted() {
    let anchor_row = declaration_with_markers(
        "anchor",
        fingerprints("anchor"),
        vec![role("conclusion_head", "eq-key", "Eq")],
        vec!["broad_head:Eq".to_owned()],
    );
    // The candidate shares only the broad-head role key and nothing structural.
    let candidate = declaration(
        "cand",
        fingerprints("cand"),
        vec![role("conclusion_head", "eq-key", "Eq")],
    );

    let index = SemanticIndex::from_declarations(&[candidate]);
    let retrieval = index.retrieve(&Anchor::from_declaration(&anchor_row), 10);

    assert!(
        retrieval.candidates.is_empty(),
        "a broad-head-only match must not admit a candidate"
    );
}

#[test]
fn high_fanout_role_postings_are_pruned_with_stable_diagnostics() {
    // 513 candidates share one non-broad role key, exceeding the role posting
    // limit (512); the anchor matches them only on that key.
    let mut corpus = Vec::new();
    for index in 0..513 {
        corpus.push(declaration(
            &format!("cand-{index}"),
            fingerprints(&format!("cand-{index}")),
            vec![role("conclusion_const", "popular", "Popular")],
        ));
    }
    let anchor_row = declaration(
        "anchor",
        fingerprints("anchor"),
        vec![role("conclusion_const", "popular", "Popular")],
    );

    let index = SemanticIndex::from_declarations(&corpus);
    let retrieval = index.retrieve(&Anchor::from_declaration(&anchor_row), 50);

    let pruned = retrieval
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "retrieval.posting_pruned");
    assert!(
        pruned.is_some(),
        "an over-fanned posting must emit a pruning diagnostic"
    );
    if let Some(diagnostic) = pruned {
        let family = diagnostic
            .details
            .as_ref()
            .and_then(|details| details.get("feature_family"))
            .and_then(serde_json::Value::as_str);
        assert_eq!(family, Some("role_conclusion_const"));
    }
    // Pruned posting was the only shared feature, so nothing is admitted.
    assert!(retrieval.candidates.is_empty());
}

#[test]
fn bounded_top_k_records_saturation_without_panicking() {
    let mut corpus = Vec::new();
    for index in 0..5 {
        corpus.push(declaration(
            &format!("cand-{index}"),
            fingerprints(&format!("cand-{index}")),
            vec![role("conclusion_const", "shared", "Shared")],
        ));
    }
    let anchor_row = declaration(
        "anchor",
        fingerprints("anchor"),
        vec![role("conclusion_const", "shared", "Shared")],
    );

    let index = SemanticIndex::from_declarations(&corpus);
    let retrieval = index.retrieve(&Anchor::from_declaration(&anchor_row), 2);

    assert_eq!(
        retrieval.candidates.len(),
        2,
        "top-k must bound the returned candidates"
    );
    assert_eq!(diagnostic_dropped(&retrieval, "retrieval.top_k_saturated"), Some(3));
}

#[test]
fn rarity_weighting_orders_selective_features_above_broad() -> Result<(), String> {
    // `selective` shares a rare key; `broad` shares a common (non-broad-head)
    // key carried by many filler candidates. Rarity must rank `selective` first.
    let mut corpus = vec![
        declaration(
            "selective",
            fingerprints("selective"),
            vec![role("conclusion_const", "rare", "Rare")],
        ),
        declaration(
            "broad",
            fingerprints("broad"),
            vec![role("conclusion_const", "common", "Common")],
        ),
    ];
    for index in 0..50 {
        corpus.push(declaration(
            &format!("filler-{index}"),
            fingerprints(&format!("filler-{index}")),
            vec![role("conclusion_const", "common", "Common")],
        ));
    }
    let anchor_row = declaration(
        "anchor",
        fingerprints("anchor"),
        vec![
            role("conclusion_const", "rare", "Rare"),
            role("conclusion_const", "common", "Common"),
        ],
    );

    let index = SemanticIndex::from_declarations(&corpus);
    let retrieval = index.retrieve(&Anchor::from_declaration(&anchor_row), 100);

    let selective = find(&retrieval, "selective").ok_or_else(|| "expected `selective`".to_owned())?;
    let broad = find(&retrieval, "broad").ok_or_else(|| "expected `broad`".to_owned())?;
    assert!(
        selective.rank < broad.rank,
        "selective rank {} should beat broad rank {}",
        selective.rank,
        broad.rank
    );
    Ok(())
}

#[test]
fn proof_goal_anchor_retrieves_from_source_backed_rows() -> Result<(), String> {
    // A proof-goal anchor shares its statement fingerprint with a declaration.
    let goal = proof_goal("Mod:1:1:1:5", fingerprints("shared"), Vec::new());
    let candidate = declaration("lemma", fingerprints("shared"), Vec::new());

    let index = SemanticIndex::from_declarations(&[candidate]);
    let retrieval = index.retrieve(&Anchor::from_proof_goal(&goal), 10);

    let found = find(&retrieval, "lemma").ok_or_else(|| "expected `lemma`".to_owned())?;
    assert_eq!(found.rank, 1);
    assert_eq!(match_count(found, FeatureFamily::StatementFingerprint), 1);
    Ok(())
}

#[test]
fn public_results_expose_no_audit_vocabulary_or_raw_keys() -> Result<(), String> {
    let anchor_row = declaration(
        "anchor",
        fingerprints("SECRET-anchor"),
        vec![role("conclusion_const", "SECRET-role-key", "Foo")],
    );
    let candidate = declaration(
        "cand",
        fingerprints("SECRET-anchor"),
        vec![role("conclusion_const", "SECRET-role-key", "Foo")],
    );

    let index = SemanticIndex::from_declarations(&[candidate]);
    let retrieval = index.retrieve(&Anchor::from_declaration(&anchor_row), 10);
    let serialized = serde_json::to_string(&retrieval).map_err(|error| error.to_string())?;

    // Family labels are present; raw keys and display text are not.
    assert!(serialized.contains("role_conclusion_const"));
    for leaked in ["SECRET-anchor", "SECRET-role-key", "Foo", "-stmt", "-safe"] {
        assert!(!serialized.contains(leaked), "result leaked `{leaked}`: {serialized}");
    }
    // Downstream duplicate-audit and transport vocabulary must never appear.
    for forbidden in [
        "review",
        "baseline",
        "replacement",
        "sqlite",
        "vector",
        "embedding",
        "statement_text",
        "/Users/",
        "posting",
        "heap",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "result leaked `{forbidden}`: {serialized}"
        );
    }
    Ok(())
}
