//! The `SQLite` `Store` and the in-memory `SemanticIndex` must rank identically.
//! These tests run the same rows through both backends via `retrieve_across`
//! and assert the full `Retrieval` (candidate ids, ranks, family explanations,
//! and diagnostics) is byte-for-byte equal — including the multi-lane recall
//! guarantee and multi-corpus fan-out.

mod common;

use common::{build_store, declaration, fingerprints, role, temp_path};
use lean_semantic_search_contract::{DeclarationFeatureRow, ProofGoalFeatureRow, SEMANTIC_FEATURE_VERSION};
use lean_semantic_search_retrieval::{Anchor, SemanticIndex, retrieve_across};

/// A corpus where a fingerprint cohort competes with a candidate that matches
/// only through a rare, selective role key — the multi-lane recall case.
fn fixture() -> Vec<DeclarationFeatureRow> {
    let mut rows = Vec::new();
    for tag in ["fp1", "fp2", "fp3", "fp4", "fp5"] {
        rows.push(declaration(tag, fingerprints("shared"), Vec::new()));
    }
    rows.push(declaration(
        "role-only",
        fingerprints("unique"),
        vec![role("conclusion_const", "rare-role-key", "Rare")],
    ));
    rows
}

fn anchor_row() -> DeclarationFeatureRow {
    declaration(
        "anchor",
        fingerprints("shared"),
        vec![role("conclusion_const", "rare-role-key", "Rare")],
    )
}

#[test]
fn single_corpus_ranking_matches_in_memory() -> Result<(), String> {
    let rows = fixture();
    let anchor = Anchor::from_declaration(&anchor_row());

    let index = SemanticIndex::from_declarations(&rows);
    let store = build_store(&temp_path("parity-single"), "token-a", &rows)?;

    let from_index = retrieve_across(&[&index], &anchor, 3);
    let from_store = retrieve_across(&[&store], &anchor, 3);

    assert_eq!(from_index, from_store);

    // The fixture must actually exercise the lane it claims to: the role-only
    // candidate survives a fingerprint cohort that fills the limit.
    let ids: Vec<&str> = from_store
        .candidates
        .iter()
        .map(|c| c.declaration_id.as_str())
        .collect();
    assert!(
        ids.contains(&"role-only"),
        "multi-lane guarantee did not surface role-only: {ids:?}"
    );
    assert!(
        ids.len() > 3,
        "expected the role lane to push the union past the limit: {ids:?}"
    );
    Ok(())
}

#[test]
fn multi_corpus_fan_out_matches_in_memory() -> Result<(), String> {
    // Split the cohort across two corpora, with role-only present in both so
    // the merge-by-declaration-id path is exercised.
    let left = vec![
        declaration("fp1", fingerprints("shared"), Vec::new()),
        declaration("fp2", fingerprints("shared"), Vec::new()),
        declaration(
            "role-only",
            fingerprints("unique"),
            vec![role("conclusion_const", "rare-role-key", "Rare")],
        ),
    ];
    let right = vec![
        declaration("fp3", fingerprints("shared"), Vec::new()),
        declaration("fp4", fingerprints("shared"), Vec::new()),
        declaration(
            "role-only",
            fingerprints("unique"),
            vec![role("conclusion_const", "rare-role-key", "Rare")],
        ),
    ];
    let anchor = Anchor::from_declaration(&anchor_row());

    let index_left = SemanticIndex::from_declarations(&left);
    let index_right = SemanticIndex::from_declarations(&right);
    let store_left = build_store(&temp_path("parity-left"), "token-l", &left)?;
    let store_right = build_store(&temp_path("parity-right"), "token-r", &right)?;

    let from_index = retrieve_across(&[&index_left, &index_right], &anchor, 5);
    let from_store = retrieve_across(&[&store_left, &store_right], &anchor, 5);

    assert_eq!(from_index, from_store);
    Ok(())
}

#[test]
fn proof_goal_anchor_matches_in_memory() -> Result<(), String> {
    let rows = fixture();
    let goal = ProofGoalFeatureRow {
        goal_id: "goal".to_owned(),
        feature_version: SEMANTIC_FEATURE_VERSION.to_owned(),
        fingerprints: fingerprints("shared"),
        role_features: vec![role("conclusion_const", "rare-role-key", "Rare")],
        low_signal_markers: Vec::new(),
    };
    let anchor = Anchor::from_proof_goal(&goal);

    let index = SemanticIndex::from_declarations(&rows);
    let store = build_store(&temp_path("parity-goal"), "token-g", &rows)?;

    assert_eq!(
        retrieve_across(&[&index], &anchor, 4),
        retrieve_across(&[&store], &anchor, 4)
    );
    Ok(())
}
