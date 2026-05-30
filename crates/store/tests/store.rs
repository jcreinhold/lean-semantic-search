//! Direct behavior of the `SQLite` store: fanout and posting reads, exact anchor
//! round-trips, order-agnostic pairing, and atomic publish.

mod common;

use common::{build_store, declaration, fingerprints, key, rich_declaration, role, temp_path};
use lean_semantic_search_contract::DeclarationFeatureRow;
use lean_semantic_search_retrieval::Corpus;
use lean_semantic_search_store::{Ingest, Store, StoreBuilder};

fn sample() -> Vec<DeclarationFeatureRow> {
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

#[test]
fn fanout_counts_match_direct_inspection() -> Result<(), String> {
    let store = build_store(&temp_path("fanout"), "tok", &sample())?;
    let keys = vec![
        key("canon-a-stmt"),   // carried by d1, d2
        key("shared-role"),    // carried by d1, d2
        key("canon-b-stmt"),   // carried by d3 only
        key("does-not-exist"), // carried by nobody
    ];
    assert_eq!(store.fanout(&keys), vec![2, 2, 1, 0]);
    assert_eq!(store.document_total(), 3);
    Ok(())
}

#[test]
fn postings_return_matching_id_sets() -> Result<(), String> {
    let store = build_store(&temp_path("postings"), "tok", &sample())?;

    let mut shared = store.postings(&key("canon-a-stmt"), 100);
    shared.sort();
    assert_eq!(shared, vec!["d1".to_owned(), "d2".to_owned()]);

    assert_eq!(store.postings(&key("canon-b-stmt"), 100), vec!["d3".to_owned()]);
    assert!(store.postings(&key("does-not-exist"), 100).is_empty());

    // The caller's limit bounds the scan.
    assert_eq!(store.postings(&key("shared-role"), 1).len(), 1);
    Ok(())
}

#[test]
fn declaration_row_round_trips_exactly() -> Result<(), String> {
    let row = rich_declaration("rich");
    let store = build_store(&temp_path("roundtrip"), "tok", std::slice::from_ref(&row))?;

    assert_eq!(store.declaration_row("rich"), Some(row));
    assert_eq!(store.declaration_row("absent"), None);
    Ok(())
}

/// Build a store feeding the items in exactly the given order, then open it.
fn build_in_order(tag: &str, items: Vec<Ingest>) -> Result<Store, String> {
    let mut builder = StoreBuilder::create(temp_path(tag), "tok").map_err(|e| e.to_string())?;
    for item in items {
        builder.accept(item).map_err(|e| e.to_string())?;
    }
    let published = builder.publish().map_err(|e| e.to_string())?;
    Store::open(published).map_err(|e| e.to_string())
}

#[test]
fn pairing_is_order_agnostic() -> Result<(), String> {
    let rows = sample();

    // Interleaved: declaration then feature, per row.
    let interleaved: Vec<Ingest> = rows
        .iter()
        .flat_map(|row| {
            [
                Ingest::Declaration(row.declaration_id.clone()),
                Ingest::Feature(row.clone()),
            ]
        })
        .collect();

    // Reversed halves: every feature first, then every announcement.
    let mut reversed: Vec<Ingest> = rows.iter().map(|row| Ingest::Feature(row.clone())).collect();
    reversed.extend(rows.iter().map(|row| Ingest::Declaration(row.declaration_id.clone())));

    let a = build_in_order("order-a", interleaved)?;
    let b = build_in_order("order-b", reversed)?;

    assert_eq!(a.document_total(), b.document_total());
    for row in &rows {
        let id = &row.declaration_id;
        assert_eq!(
            a.declaration_row(id),
            b.declaration_row(id),
            "row {id} differs by arrival order"
        );
    }
    let probe = vec![key("canon-a-stmt"), key("shared-role"), key("canon-b-stmt")];
    assert_eq!(a.fanout(&probe), b.fanout(&probe));
    Ok(())
}

#[test]
fn unpaired_halves_are_not_indexed() -> Result<(), String> {
    // A feature with no announcement, and an announcement with no feature: both
    // are incomplete and must not enter the corpus.
    let store = build_in_order(
        "unpaired",
        vec![
            Ingest::Feature(declaration("featured-only", fingerprints("x"), Vec::new())),
            Ingest::Declaration("announced-only".to_owned()),
        ],
    )?;
    assert_eq!(store.document_total(), 0);
    assert_eq!(store.declaration_row("featured-only"), None);
    Ok(())
}

#[test]
fn dropped_build_leaves_no_final_path() -> Result<(), String> {
    let path = temp_path("dropped");
    let temp = building_sibling(&path);
    {
        let mut builder = StoreBuilder::create(&path, "tok").map_err(|e| e.to_string())?;
        builder
            .accept(Ingest::Declaration("d1".to_owned()))
            .map_err(|e| e.to_string())?;
        builder
            .accept(Ingest::Feature(declaration("d1", fingerprints("a"), Vec::new())))
            .map_err(|e| e.to_string())?;
        // Drop without publishing.
    }
    assert!(!path.exists(), "an unpublished build must not create the final path");
    assert!(!temp.exists(), "an unpublished build must clean up its temp file");
    Ok(())
}

#[test]
fn interrupted_rebuild_leaves_prior_corpus_intact() -> Result<(), String> {
    let path = temp_path("rebuild");
    let temp = building_sibling(&path);

    // Publish an original corpus.
    let original = vec![declaration("orig", fingerprints("a"), Vec::new())];
    let store = build_store(&path, "v1", &original)?;
    assert_eq!(store.document_total(), 1);
    drop(store);

    // Start a replacement build at the same destination, then abandon it.
    {
        let mut builder = StoreBuilder::create(&path, "v2").map_err(|e| e.to_string())?;
        builder
            .accept(Ingest::Declaration("new".to_owned()))
            .map_err(|e| e.to_string())?;
        builder
            .accept(Ingest::Feature(declaration("new", fingerprints("b"), Vec::new())))
            .map_err(|e| e.to_string())?;
    }

    assert!(!temp.exists(), "abandoned rebuild must clean up its temp file");
    let reopened = Store::open(&path).map_err(|e| e.to_string())?;
    assert_eq!(
        reopened.corpus_token(),
        "v1",
        "prior corpus must survive an interrupted rebuild"
    );
    assert_eq!(
        reopened.declaration_row("orig").map(|r| r.declaration_id),
        Some("orig".to_owned())
    );
    assert_eq!(reopened.declaration_row("new"), None);
    Ok(())
}

/// Mirror of the writer's temp-path rule for assertions.
fn building_sibling(final_path: &std::path::Path) -> std::path::PathBuf {
    let mut name = final_path.file_name().map(std::ffi::OsString::from).unwrap_or_default();
    name.push(".building");
    final_path.with_file_name(name)
}
