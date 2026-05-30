//! The store holds the semantic index and nothing a consumer renders. This
//! proves it structurally: the published file's table names, column names, and
//! metadata keys carry no display, provenance, label, or audit vocabulary.

mod common;

use common::{build_store, rich_declaration, temp_path};
use rusqlite::{Connection, OpenFlags};

/// Vocabulary that belongs to a consumer, never to the semantic index.
const FORBIDDEN: &[&str] = &[
    "display",
    "module",
    "kind",
    "visibility",
    "qualified",
    "name",
    "origin",
    "provenance",
    "label",
    "mathlib",
    "probe",
    "evidence",
    "audit",
    "docstring",
    "statement_text",
    "source_span",
];

fn assert_clean(identifier: &str) {
    let lower = identifier.to_lowercase();
    for forbidden in FORBIDDEN {
        assert!(
            !lower.contains(forbidden),
            "stored identifier {identifier:?} leaks consumer vocabulary {forbidden:?}"
        );
    }
}

#[test]
fn schema_carries_no_consumer_vocabulary() -> Result<(), String> {
    let path = temp_path("boundary");
    let _store = build_store(&path, "tok", std::slice::from_ref(&rich_declaration("rich")))?;

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY;
    let connection = Connection::open_with_flags(&path, flags).map_err(|e| e.to_string())?;

    let mut tables_query = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .map_err(|e| e.to_string())?;
    let tables: Vec<String> = tables_query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    drop(tables_query);

    assert!(tables.contains(&"postings".to_owned()), "expected the postings table");
    for table in &tables {
        assert_clean(table);
        let mut columns_query = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|e| e.to_string())?;
        let columns: Vec<String> = columns_query
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for column in &columns {
            assert_clean(column);
        }
    }
    Ok(())
}

#[test]
fn metadata_keys_are_exactly_the_recorded_facts() -> Result<(), String> {
    let path = temp_path("boundary-meta");
    let _store = build_store(&path, "tok", std::slice::from_ref(&rich_declaration("rich")))?;

    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let mut query = connection
        .prepare("SELECT key FROM metadata ORDER BY key")
        .map_err(|e| e.to_string())?;
    let keys: Vec<String> = query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();

    assert_eq!(
        keys,
        vec![
            "corpus_token".to_owned(),
            "policy_version".to_owned(),
            "schema_version".to_owned(),
            "total_documents".to_owned(),
        ]
    );
    Ok(())
}
