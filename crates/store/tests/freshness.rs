//! The freshness verdict: a persisted corpus is reused only on a matching
//! opaque token and matching owned versions; every other case — token mismatch,
//! schema or policy drift, a missing file, a damaged file — is a structured
//! cache miss, never a hard error, and a rebuild then succeeds.

mod common;

use common::{build_store, declaration, fingerprints, rich_declaration, temp_path};
use lean_semantic_search_retrieval::Corpus;
use lean_semantic_search_store::{CacheMiss, CorpusLookup, Store};
use rusqlite::{Connection, params};

/// The verdict from an `open_fresh`, projected to the cache-miss reason (or a
/// marker that it opened) so tests can assert on it without the open `Store`.
fn miss(lookup: &CorpusLookup) -> Option<CacheMiss> {
    match lookup {
        CorpusLookup::Fresh(_) => None,
        CorpusLookup::Stale(reason) => Some(*reason),
    }
}

#[test]
fn matching_token_and_versions_open_fresh() -> Result<(), String> {
    let path = temp_path("fresh-hit");
    let store = build_store(&path, "tok", std::slice::from_ref(&rich_declaration("d")))?;
    drop(store);

    match Store::open_fresh(&path, "tok") {
        CorpusLookup::Fresh(store) => {
            assert_eq!(store.corpus_token(), "tok");
            assert_eq!(
                store.declaration_row("d").map(|r| r.declaration_id),
                Some("d".to_owned())
            );
        }
        CorpusLookup::Stale(reason) => return Err(format!("expected a fresh open, got {reason:?}")),
    }
    Ok(())
}

#[test]
fn mismatched_token_is_a_cache_miss() -> Result<(), String> {
    let path = temp_path("stale-token");
    let store = build_store(
        &path,
        "tok",
        std::slice::from_ref(&declaration("d", fingerprints("a"), Vec::new())),
    )?;
    drop(store);

    assert_eq!(
        miss(&Store::open_fresh(&path, "other-token")),
        Some(CacheMiss::TokenMismatch)
    );
    Ok(())
}

#[test]
fn absent_corpus_is_missing_not_an_error() {
    let path = temp_path("absent");
    assert_eq!(miss(&Store::open_fresh(&path, "tok")), Some(CacheMiss::Missing));
}

#[test]
fn schema_drift_is_a_cache_miss() -> Result<(), String> {
    let path = temp_path("schema-drift");
    let store = build_store(
        &path,
        "tok",
        std::slice::from_ref(&declaration("d", fingerprints("a"), Vec::new())),
    )?;
    drop(store);

    // Rewrite the stored schema version to a value this build does not read, as
    // if the file were written by an older store.
    tamper(&path, "schema_version", "some-older-store-schema")?;
    assert_eq!(miss(&Store::open_fresh(&path, "tok")), Some(CacheMiss::SchemaDrift));
    Ok(())
}

#[test]
fn policy_drift_is_a_cache_miss() -> Result<(), String> {
    let path = temp_path("policy-drift");
    let store = build_store(
        &path,
        "tok",
        std::slice::from_ref(&declaration("d", fingerprints("a"), Vec::new())),
    )?;
    drop(store);

    // Rewrite the stored retrieval policy version to a value this build does not
    // rank under, as if ranked under an older policy. The schema still matches,
    // so the file opens and the drift is caught by the freshness comparison.
    tamper(&path, "policy_version", "some-older-retrieval-policy")?;
    assert_eq!(miss(&Store::open_fresh(&path, "tok")), Some(CacheMiss::PolicyDrift));
    Ok(())
}

#[test]
fn garbage_file_is_corrupt_then_rebuild_succeeds() -> Result<(), String> {
    let path = temp_path("garbage");
    let row = rich_declaration("d");
    drop(build_store(&path, "tok", std::slice::from_ref(&row))?);

    // Overwrite the published corpus with bytes that are not a database.
    std::fs::write(&path, b"this is not a sqlite database").map_err(|e| e.to_string())?;
    assert_eq!(miss(&Store::open_fresh(&path, "tok")), Some(CacheMiss::Corrupt));

    // A rebuild at the same path recovers: the cache miss triggered it, and the
    // fresh corpus opens.
    drop(build_store(&path, "tok", std::slice::from_ref(&row))?);
    match Store::open_fresh(&path, "tok") {
        CorpusLookup::Fresh(store) => assert_eq!(store.declaration_row("d"), Some(row)),
        CorpusLookup::Stale(reason) => return Err(format!("rebuild should be fresh, got {reason:?}")),
    }
    Ok(())
}

#[test]
fn truncated_file_is_corrupt_not_a_panic() -> Result<(), String> {
    let path = temp_path("truncated");
    let rows: Vec<_> = (0..64)
        .map(|i| declaration(&format!("d{i}"), fingerprints(&format!("f{i}")), Vec::new()))
        .collect();
    drop(build_store(&path, "tok", &rows)?);

    // Truncate the file mid-database, leaving a malformed image.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.set_len(512).map_err(|e| e.to_string())?;
    drop(file);

    assert_eq!(miss(&Store::open_fresh(&path, "tok")), Some(CacheMiss::Corrupt));
    Ok(())
}

/// Open a published corpus read-write and overwrite one metadata value, to
/// simulate a file written under different versions.
fn tamper(path: &std::path::Path, key: &str, value: &str) -> Result<(), String> {
    let connection = Connection::open(path).map_err(|e| e.to_string())?;
    connection
        .execute("UPDATE metadata SET value = ?1 WHERE key = ?2", params![value, key])
        .map_err(|e| e.to_string())?;
    connection.close().map_err(|(_, e)| e.to_string())
}
