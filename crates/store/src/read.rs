//! The read path: a read-only connection that answers the [`Corpus`] seam.
//!
//! [`Store::open`] validates the schema and caches the four metadata facts once,
//! so [`Corpus`] methods are total and `document_total` is a field read. The
//! connection is opened read-only and single-threaded; nothing here loads the
//! corpus into memory. `fanout` answers all requested keys in one batched
//! `COUNT … GROUP BY` (chunked under `SQLite`'s bind-parameter ceiling); `postings`
//! is a per-key indexed scan bounded by the caller's limit; `declaration_row`
//! deserializes one stored row. None hydrate display data — the store holds
//! none.

use std::path::Path;

use lean_semantic_search_contract::{DeclarationFeatureRow, OpaqueFeatureKey};
use lean_semantic_search_retrieval::Corpus;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter};

use crate::StoreError;
use crate::schema::{
    META_CORPUS_TOKEN, META_POLICY_VERSION, META_SCHEMA_VERSION, META_TOTAL_DOCUMENTS, STORE_SCHEMA_VERSION,
};

/// `SQLite` binds at most 999 parameters per statement by default; stay well
/// under it so a fanout over many keys chunks rather than fails.
const FANOUT_CHUNK: usize = 900;

/// A persisted corpus opened read-only.
///
/// Implements [`Corpus`] over `SQLite` so retrieval ranks against a large,
/// on-disk index without materializing it. The opaque `corpus_token`,
/// `schema_version`, and `policy_version` are exposed as read-only facts; the
/// store records and compares them but never interprets them.
pub struct Store {
    connection: Connection,
    total_documents: usize,
    schema_version: String,
    policy_version: String,
    corpus_token: String,
}

impl Store {
    /// Open a persisted corpus read-only and cache its metadata.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Sqlite`] if the file cannot be opened,
    /// [`StoreError::MissingMetadata`] if a required metadata fact is absent,
    /// [`StoreError::SchemaMismatch`] if the stored schema version is not the
    /// one this build understands, or [`StoreError::Corrupt`] if the document
    /// total is unparseable.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags)?;
        connection.execute_batch("PRAGMA query_only = true;")?;

        let schema_version = read_metadata(&connection, META_SCHEMA_VERSION)?;
        if schema_version != STORE_SCHEMA_VERSION {
            return Err(StoreError::SchemaMismatch {
                found: schema_version,
                expected: STORE_SCHEMA_VERSION,
            });
        }
        let policy_version = read_metadata(&connection, META_POLICY_VERSION)?;
        let corpus_token = read_metadata(&connection, META_CORPUS_TOKEN)?;
        let total_documents = read_metadata(&connection, META_TOTAL_DOCUMENTS)?
            .parse::<usize>()
            .map_err(|_| StoreError::Corrupt(META_TOTAL_DOCUMENTS))?;

        Ok(Self {
            connection,
            total_documents,
            schema_version,
            policy_version,
            corpus_token,
        })
    }

    /// The opaque, caller-supplied content identity recorded at build time.
    /// Compared but never interpreted by the store.
    #[must_use]
    pub fn corpus_token(&self) -> &str {
        &self.corpus_token
    }

    /// The store's own on-disk schema identity.
    #[must_use]
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    /// The retrieval policy version active when this corpus was built.
    #[must_use]
    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }

    /// The number of documents in the corpus (also the [`Corpus`] total).
    #[must_use]
    pub fn document_total(&self) -> usize {
        self.total_documents
    }
}

impl Corpus for Store {
    fn document_total(&self) -> usize {
        self.total_documents
    }

    fn fanout(&self, keys: &[OpaqueFeatureKey]) -> Vec<usize> {
        if keys.is_empty() {
            return Vec::new();
        }
        let mut distinct: Vec<&str> = keys.iter().map(OpaqueFeatureKey::as_str).collect();
        distinct.sort_unstable();
        distinct.dedup();

        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for chunk in distinct.chunks(FANOUT_CHUNK) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!("SELECT key, COUNT(*) FROM postings WHERE key IN ({placeholders}) GROUP BY key");
            let Ok(mut statement) = self.connection.prepare_cached(&sql) else {
                continue;
            };
            let Ok(rows) = statement.query_map(params_from_iter(chunk.iter().copied()), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            }) else {
                continue;
            };
            for (key, count) in rows.flatten() {
                counts.insert(key, usize::try_from(count).unwrap_or(0));
            }
        }
        keys.iter()
            .map(|key| counts.get(key.as_str()).copied().unwrap_or(0))
            .collect()
    }

    fn postings(&self, key: &OpaqueFeatureKey, limit: usize) -> Vec<String> {
        // `usize::MAX` (used for never-pruned fingerprints) cannot exceed the
        // SQL limit; clamp to i64 so the bind is valid. Safe because the caller
        // only reaches here when fanout already fits the limit.
        let bound = i64::try_from(limit).unwrap_or(i64::MAX);
        let Ok(mut statement) = self
            .connection
            .prepare_cached("SELECT declaration_id FROM postings WHERE key = ?1 ORDER BY declaration_id LIMIT ?2")
        else {
            return Vec::new();
        };
        let Ok(rows) = statement.query_map(params![key.as_str(), bound], |row| row.get::<_, String>(0)) else {
            return Vec::new();
        };
        rows.flatten().collect()
    }

    fn declaration_row(&self, declaration_id: &str) -> Option<DeclarationFeatureRow> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT row_json FROM feature_rows WHERE declaration_id = ?1")
            .ok()?;
        let row_json: String = statement.query_row(params![declaration_id], |row| row.get(0)).ok()?;
        serde_json::from_str(&row_json).ok()
    }
}

fn read_metadata(connection: &Connection, key: &str) -> Result<String, StoreError> {
    connection
        .prepare_cached("SELECT value FROM metadata WHERE key = ?1")?
        .query_row(params![key], |row| row.get::<_, String>(0))
        .optional()?
        .ok_or_else(|| StoreError::MissingMetadata(key.to_owned()))
}
