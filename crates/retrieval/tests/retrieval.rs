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
use lean_semantic_search_retrieval::{
    Anchor, Candidate, Corpus, FeatureFamily, Retrieval, SemanticIndex, retrieve_across,
};

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 10);

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 10);

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 50);

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 2);

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 100);

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_proof_goal(&goal), 10);

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
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 10);
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

#[test]
fn corpus_answers_fanout_postings_and_anchor_reconstruction() -> Result<(), String> {
    // `a` and `b` share a statement fingerprint; only `a` carries the role key.
    let a = declaration(
        "a",
        fingerprints("shared"),
        vec![role("conclusion_const", "role-k", "Foo")],
    );
    let b = declaration("b", fingerprints("shared"), Vec::new());
    let index = SemanticIndex::from_declarations(&[a.clone(), b]);

    assert_eq!(index.document_total(), 2, "document total counts every row once");

    // Batched fanout matches a direct count of who carries each key, in order.
    let shared_statement = key("shared-stmt");
    let role_key = key("role-k");
    let absent = key("carried-by-nobody");
    assert_eq!(
        index.fanout(&[shared_statement.clone(), role_key.clone(), absent]),
        vec![2, 1, 0]
    );

    // Postings name exactly the declarations carrying the key.
    let mut shared_ids = index.postings(&shared_statement, 10);
    shared_ids.sort();
    assert_eq!(shared_ids, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(index.postings(&role_key, 10), vec!["a".to_owned()]);
    // A tighter limit bounds the scan.
    assert_eq!(index.postings(&shared_statement, 1).len(), 1);

    // A corpus member rebuilds into the exact row it was built from.
    let rebuilt = index
        .declaration_row("a")
        .ok_or_else(|| "expected row `a`".to_owned())?;
    assert_eq!(rebuilt, a);
    assert!(index.declaration_row("not-in-corpus").is_none());
    Ok(())
}

#[test]
fn fan_out_merges_candidates_from_every_corpus() -> Result<(), String> {
    // Each corpus shares a different fingerprint with the anchor, so each
    // contributes its own candidate; the merged list is bounded and ranked.
    let anchor_row = declaration(
        "anchor",
        Fingerprints {
            statement: key("alpha-stmt"),
            safe_binder_permutation: key("beta-safe"),
            connective_shape: key("anchor-conn"),
            conclusion_shape: key("anchor-concl"),
        },
        Vec::new(),
    );
    let left = SemanticIndex::from_declarations(&[declaration("from-left", fingerprints("alpha"), Vec::new())]);
    let right = SemanticIndex::from_declarations(&[declaration("from-right", fingerprints("beta"), Vec::new())]);

    let retrieval = retrieve_across(&[&left, &right], &Anchor::from_declaration(&anchor_row), 10);
    let ids: Vec<&str> = retrieval
        .candidates
        .iter()
        .map(|candidate| candidate.declaration_id.as_str())
        .collect();
    assert!(ids.contains(&"from-left"), "missing left corpus candidate: {ids:?}");
    assert!(ids.contains(&"from-right"), "missing right corpus candidate: {ids:?}");

    // Order is deterministic across runs and stable under corpus order.
    let reversed = retrieve_across(&[&right, &left], &Anchor::from_declaration(&anchor_row), 10);
    assert_eq!(retrieval, reversed, "merged order must not depend on corpus order");
    Ok(())
}

#[test]
fn shared_declaration_across_corpora_merges_once_with_summed_contributions() -> Result<(), String> {
    // The same declaration id sits in two corpora, each matching the anchor on a
    // different fingerprint. It must appear once, crediting both families.
    let anchor_row = declaration("anchor", fingerprints("shared"), Vec::new());
    // Corpus members that share the anchor's statement / conclusion fingerprint
    // respectively but under the same id.
    let statement_side = DeclarationFeatureRow {
        fingerprints: Fingerprints {
            statement: key("shared-stmt"),
            safe_binder_permutation: key("x-safe"),
            connective_shape: key("x-conn"),
            conclusion_shape: key("x-concl"),
        },
        ..declaration("dup", fingerprints("x"), Vec::new())
    };
    let conclusion_side = DeclarationFeatureRow {
        fingerprints: Fingerprints {
            statement: key("y-stmt"),
            safe_binder_permutation: key("y-safe"),
            connective_shape: key("y-conn"),
            conclusion_shape: key("shared-concl"),
        },
        ..declaration("dup", fingerprints("y"), Vec::new())
    };
    let left = SemanticIndex::from_declarations(&[statement_side]);
    let right = SemanticIndex::from_declarations(&[conclusion_side]);

    let retrieval = retrieve_across(&[&left, &right], &Anchor::from_declaration(&anchor_row), 10);
    let dup: Vec<&Candidate> = retrieval
        .candidates
        .iter()
        .filter(|candidate| candidate.declaration_id == "dup")
        .collect();
    assert_eq!(dup.len(), 1, "a shared id must merge into a single candidate");
    let dup = dup
        .first()
        .ok_or_else(|| "expected the merged `dup` candidate".to_owned())?;
    assert_eq!(match_count(dup, FeatureFamily::StatementFingerprint), 1);
    assert_eq!(match_count(dup, FeatureFamily::ConclusionFingerprint), 1);
    Ok(())
}

#[test]
fn role_lane_keeps_a_selective_match_a_fingerprint_cohort_would_evict() -> Result<(), String> {
    // A cohort of candidates shares all of the anchor's fingerprints, so their
    // total scores dwarf a lone candidate that matches only a selective role key.
    // A single combined top-k would evict the role match; the role lane keeps it.
    let mut corpus = Vec::new();
    for index in 0..8 {
        corpus.push(declaration(&format!("fp-{index}"), fingerprints("anchor"), Vec::new()));
    }
    corpus.push(declaration(
        "role-only",
        fingerprints("unrelated"),
        vec![role("conclusion_const", "rare-role", "Rare")],
    ));
    let anchor_row = declaration(
        "anchor",
        fingerprints("anchor"),
        vec![role("conclusion_const", "rare-role", "Rare")],
    );

    let index = SemanticIndex::from_declarations(&corpus);
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 2);

    assert!(
        find(&retrieval, "role-only").is_some(),
        "the role lane must rescue the selective role match: {:?}",
        retrieval
            .candidates
            .iter()
            .map(|candidate| candidate.declaration_id.as_str())
            .collect::<Vec<_>>()
    );
    // The fingerprint cohort is still represented too.
    assert!(
        retrieval.candidates.iter().any(|c| c.declaration_id.starts_with("fp-")),
        "the fingerprint lane must still contribute its cohort"
    );
    Ok(())
}

#[test]
fn accumulation_by_id_ranks_a_stronger_total_first() -> Result<(), String> {
    // `strong` shares the anchor's statement fingerprint and a role key; `weak`
    // shares only the role key. Accumulating per declaration id, `strong` must
    // outrank `weak`, and each id appears exactly once.
    let anchor_row = declaration(
        "anchor",
        fingerprints("shared"),
        vec![role("conclusion_const", "role-k", "Foo")],
    );
    let strong = declaration(
        "strong",
        fingerprints("shared"),
        vec![role("conclusion_const", "role-k", "Foo")],
    );
    let weak = declaration(
        "weak",
        fingerprints("unrelated"),
        vec![role("conclusion_const", "role-k", "Foo")],
    );

    let index = SemanticIndex::from_declarations(&[strong, weak]);
    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 10);

    let strong = find(&retrieval, "strong").ok_or_else(|| "expected `strong`".to_owned())?;
    let weak = find(&retrieval, "weak").ok_or_else(|| "expected `weak`".to_owned())?;
    assert_eq!(strong.rank, 1, "the stronger total must rank first");
    assert_eq!(weak.rank, 2);
    assert_eq!(retrieval.candidates.len(), 2, "each id appears exactly once");
    Ok(())
}

#[test]
fn fan_out_path_exposes_no_raw_keys_or_audit_vocabulary() -> Result<(), String> {
    let anchor_row = declaration(
        "anchor",
        fingerprints("SECRET-anchor"),
        vec![role("conclusion_const", "SECRET-role-key", "Foo")],
    );
    let index = SemanticIndex::from_declarations(&[declaration(
        "cand",
        fingerprints("SECRET-anchor"),
        vec![role("conclusion_const", "SECRET-role-key", "Foo")],
    )]);

    let retrieval = retrieve_across(&[&index], &Anchor::from_declaration(&anchor_row), 10);
    let serialized = serde_json::to_string(&retrieval).map_err(|error| error.to_string())?;
    for leaked in ["SECRET-anchor", "SECRET-role-key", "Foo", "-stmt", "posting", "heap"] {
        assert!(
            !serialized.contains(leaked),
            "fan-out result leaked `{leaked}`: {serialized}"
        );
    }
    Ok(())
}
