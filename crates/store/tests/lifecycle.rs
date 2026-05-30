//! The neutral lifecycle primitives: a content-addressed layout, an atomic
//! latest-pointer that never strands a reader, multi-reader safety against a
//! concurrent build, and a caller-driven cleanup that protects the active
//! pointer and defaults to reporting before deleting.

mod common;

use std::thread;

use common::{build_store, declaration, fingerprints, role, temp_root};
use lean_semantic_search_retrieval::{Anchor, retrieve_across};
use lean_semantic_search_store::{
    CleanupMode, CorpusLookup, Ingest, Store, StoreBuilder, cleanup, index_path, latest_index_path, latest_name,
    open_latest_fresh, set_latest,
};

/// A small corpus where `d1`/`d2` share the anchor's keys and `d3` is unique.
fn sample() -> Vec<lean_semantic_search_contract::DeclarationFeatureRow> {
    vec![
        declaration(
            "d1",
            fingerprints("a"),
            vec![role("conclusion_const", "shared-role", "S")],
        ),
        declaration(
            "d2",
            fingerprints("a"),
            vec![role("conclusion_const", "shared-role", "S")],
        ),
        declaration("d3", fingerprints("b"), Vec::new()),
    ]
}

fn anchor() -> Anchor {
    Anchor::from_declaration(&declaration(
        "anchor",
        fingerprints("a"),
        vec![role("conclusion_const", "shared-role", "S")],
    ))
}

/// Build and publish corpus `name` into its content-addressed directory under
/// `root`, without flipping the latest pointer.
fn publish_corpus(root: &std::path::Path, name: &str, token: &str) -> Result<(), String> {
    let store = build_store(&index_path(root, name), token, &sample())?;
    drop(store);
    Ok(())
}

#[test]
fn open_latest_resolves_the_published_corpus() -> Result<(), String> {
    let root = temp_root("open-latest");
    publish_corpus(&root, "c1", "tok")?;
    set_latest(&root, "c1").map_err(|e| e.to_string())?;

    match open_latest_fresh(&root, "tok") {
        CorpusLookup::Fresh(store) => {
            let retrieval = retrieve_across(&[&store], &anchor(), 20);
            assert!(!retrieval.candidates.is_empty(), "the resolved corpus should retrieve");
        }
        CorpusLookup::Stale(reason) => return Err(format!("expected fresh latest, got {reason:?}")),
    }
    Ok(())
}

#[test]
fn an_unset_pointer_is_a_cache_miss() {
    let root = temp_root("no-pointer");
    assert!(matches!(open_latest_fresh(&root, "tok"), CorpusLookup::Stale(_)));
}

#[test]
fn abandoned_rebuild_leaves_the_prior_corpus_published() -> Result<(), String> {
    let root = temp_root("abandoned");

    // Publish v1 and point latest at it.
    publish_corpus(&root, "v1", "v1")?;
    set_latest(&root, "v1").map_err(|e| e.to_string())?;

    // Start building v2 into its own directory, then abandon it before
    // publishing the file or flipping the pointer.
    {
        let mut builder = StoreBuilder::create(index_path(&root, "v2"), "v2").map_err(|e| e.to_string())?;
        builder
            .accept(Ingest::Declaration("new".to_owned()))
            .map_err(|e| e.to_string())?;
        builder
            .accept(Ingest::Feature(declaration("new", fingerprints("z"), Vec::new())))
            .map_err(|e| e.to_string())?;
        // dropped without publish
    }

    // The latest pointer still names v1, the abandoned v2 index is absent, and a
    // reader resolving latest sees the intact prior corpus.
    assert_eq!(latest_name(&root).as_deref(), Some("v1"));
    assert!(
        !index_path(&root, "v2").exists(),
        "abandoned build must leave no index file"
    );
    match open_latest_fresh(&root, "v1") {
        CorpusLookup::Fresh(store) => assert_eq!(store.corpus_token(), "v1"),
        CorpusLookup::Stale(reason) => return Err(format!("prior corpus must stay openable, got {reason:?}")),
    }
    Ok(())
}

#[test]
fn published_corpus_serves_concurrent_readers_during_a_build() -> Result<(), String> {
    let root = temp_root("concurrent");
    publish_corpus(&root, "c1", "tok")?;
    set_latest(&root, "c1").map_err(|e| e.to_string())?;

    let index = index_path(&root, "c1");
    let reader_a = Store::open(&index).map_err(|e| e.to_string())?;
    let reader_b = Store::open(&index).map_err(|e| e.to_string())?;

    // A third corpus builds to its own directory in another thread while the two
    // readers serve queries against the published one.
    let build_root = root.clone();
    let builder = thread::spawn(move || -> Result<(), String> {
        let store = build_store(&index_path(&build_root, "c2"), "tok2", &sample())?;
        drop(store);
        Ok(())
    });

    let probe = anchor();
    let mut last = None;
    for _ in 0..50 {
        let a = retrieve_across(&[&reader_a], &probe, 20).candidates.len();
        let b = retrieve_across(&[&reader_b], &probe, 20).candidates.len();
        assert_eq!(a, b, "both readers must agree under a concurrent build");
        last = Some(a);
    }

    builder.join().map_err(|_| "builder thread panicked".to_owned())??;

    assert_eq!(last, Some(2), "readers should match the d1/d2 cohort throughout");
    // The concurrent build published its own corpus without disturbing the readers'.
    assert!(index_path(&root, "c2").exists());
    Ok(())
}

#[test]
fn cleanup_protects_the_latest_and_defaults_to_reporting() -> Result<(), String> {
    let root = temp_root("cleanup");
    for name in ["a", "b", "c"] {
        publish_corpus(&root, name, name)?;
    }
    set_latest(&root, "a").map_err(|e| e.to_string())?;

    // Dry run: c is the only thing not kept and not the latest target, and
    // nothing is deleted.
    let plan = cleanup(&root, &["b"], CleanupMode::DryRun).map_err(|e| e.to_string())?;
    assert!(!plan.executed);
    assert!(plan.bytes_removable > 0, "the removable directory should report bytes");
    let removable: Vec<_> = plan
        .removable
        .iter()
        .filter_map(|e| e.dir.file_name()?.to_str())
        .collect();
    assert_eq!(removable, vec!["c"]);
    let mut protected: Vec<_> = plan
        .protected
        .iter()
        .filter_map(|e| e.dir.file_name()?.to_str())
        .collect();
    protected.sort_unstable();
    assert_eq!(protected, vec!["a", "b"]);
    // The active latest target is never offered for removal.
    assert!(
        plan.removable
            .iter()
            .all(|e| e.dir.file_name().and_then(|n| n.to_str()) != Some("a"))
    );
    // Dry run touched nothing.
    for name in ["a", "b", "c"] {
        assert!(index_path(&root, name).exists(), "dry run must not delete {name}");
    }

    // Execute: only the unexpected directory is removed; the latest stays openable.
    let report = cleanup(&root, &["b"], CleanupMode::Execute).map_err(|e| e.to_string())?;
    assert!(report.executed);
    assert!(!index_path(&root, "c").exists(), "execute should remove c");
    assert!(index_path(&root, "a").exists() && index_path(&root, "b").exists());
    assert!(latest_index_path(&root).is_some_and(|p| p.exists()));
    assert!(matches!(open_latest_fresh(&root, "a"), CorpusLookup::Fresh(_)));
    Ok(())
}
