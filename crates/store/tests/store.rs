//! Direct behavior of the `SQLite` store: fanout and posting reads, exact anchor
//! round-trips, order-agnostic pairing, and atomic publish, including two builds
//! of one destination at once.

mod common;

use std::path::PathBuf;
use std::sync::Barrier;

use common::{
    build_store, declaration, fingerprints, key, rich_declaration, role, start_build, stray_files, temp_path,
};
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
    let path = temp_path("fanout")?;
    let store = build_store(&path, "tok", &sample())?;
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
    let path = temp_path("postings")?;
    let store = build_store(&path, "tok", &sample())?;

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
    let path = temp_path("roundtrip")?;
    let store = build_store(&path, "tok", std::slice::from_ref(&row))?;

    assert_eq!(store.declaration_row("rich"), Some(row));
    assert_eq!(store.declaration_row("absent"), None);
    Ok(())
}

/// Build a store at `path` feeding the items in exactly the given order, then
/// open it.
fn build_in_order(path: &std::path::Path, items: Vec<Ingest>) -> Result<Store, String> {
    let mut builder = StoreBuilder::create(path, "tok").map_err(|e| e.to_string())?;
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

    let path_a = temp_path("order-a")?;
    let path_b = temp_path("order-b")?;
    let a = build_in_order(&path_a, interleaved)?;
    let b = build_in_order(&path_b, reversed)?;

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
    let path = temp_path("unpaired")?;
    let store = build_in_order(
        &path,
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
    let path = temp_path("dropped")?;
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
    let stray = stray_files(&path)?;
    assert!(
        stray.is_empty(),
        "an unpublished build must clean up its temp file: {stray:?}"
    );
    Ok(())
}

#[test]
fn interrupted_rebuild_leaves_prior_corpus_intact() -> Result<(), String> {
    let path = temp_path("rebuild")?;

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

    let stray = stray_files(&path)?;
    assert!(
        stray.is_empty(),
        "abandoned rebuild must clean up its temp file: {stray:?}"
    );
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

#[test]
fn concurrent_builds_of_one_path_both_publish() -> Result<(), String> {
    let path = temp_path("concurrent")?;
    let left: Vec<DeclarationFeatureRow> = (0..3)
        .map(|n| declaration(&format!("left-{n}"), fingerprints("l"), Vec::new()))
        .collect();
    let right: Vec<DeclarationFeatureRow> = (0..5)
        .map(|n| declaration(&format!("right-{n}"), fingerprints("r"), Vec::new()))
        .collect();

    // Hold both builds open at once: each ingests, waits for the other, then
    // publishes. Builds that shared a temp file would break each other here.
    let both_open = Barrier::new(2);
    let build = |corpus_token: &str, rows: &[DeclarationFeatureRow]| -> Result<PathBuf, String> {
        let builder = start_build(&path, corpus_token, rows);
        both_open.wait();
        builder?.publish().map_err(|error| error.to_string())
    };
    let results = std::thread::scope(|scope| {
        let left_build = scope.spawn(|| build("left", &left));
        let right_build = scope.spawn(|| build("right", &right));
        [left_build.join(), right_build.join()]
    });
    for result in results {
        let published = result.map_err(|_| "a build thread panicked".to_owned())??;
        assert_eq!(published.as_path(), &*path);
    }
    let stray = stray_files(&path)?;
    assert!(
        stray.is_empty(),
        "both builds must clean up their temp files: {stray:?}"
    );

    // The last rename wins, and what it leaves is one whole corpus, not a mix.
    let store = Store::open(&path).map_err(|e| e.to_string())?;
    let (expected, absent) = match store.corpus_token() {
        "left" => (&left, &right),
        "right" => (&right, &left),
        other => return Err(format!("unexpected corpus token {other}")),
    };
    assert_eq!(store.document_total(), expected.len());
    for row in expected {
        assert_eq!(store.declaration_row(&row.declaration_id).as_ref(), Some(row));
    }
    for row in absent {
        assert_eq!(store.declaration_row(&row.declaration_id), None);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn published_store_has_the_mode_sqlite_gives_a_new_database() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    // A database `SQLite` creates itself in the same directory shows the mode
    // a published store should have under the current umask.
    let path = temp_path("mode")?;
    drop(build_store(&path, "tok", &sample())?);
    let reference = path.with_file_name("reference.sqlite");
    rusqlite::Connection::open(&reference)
        .and_then(|connection| connection.execute_batch("CREATE TABLE t (x)"))
        .map_err(|e| e.to_string())?;
    let mode = |file: &std::path::Path| -> Result<u32, String> {
        let metadata = std::fs::metadata(file).map_err(|e| e.to_string())?;
        Ok(metadata.permissions().mode() & 0o777)
    };
    assert_eq!(mode(&path)?, mode(&reference)?);
    Ok(())
}
