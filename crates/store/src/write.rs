//! The streaming, order-agnostic build path.
//!
//! A [`StoreBuilder`] ingests two kinds of [`Ingest`] item — a declaration
//! identity announcement and a feature row — that may arrive in any order or
//! interleaved. A declaration is written only once *both* halves have arrived,
//! paired by `declaration_id`; the unpaired halves wait in two small maps and
//! are evicted the moment they pair, so the resident set during a build is
//! bounded by the number of concurrently unpaired declarations, never by the
//! corpus. `SQLite` holds the rows already written.
//!
//! The whole build runs in one transaction against a temp file with aggressive
//! write pragmas; on [`publish`](StoreBuilder::publish) the transaction commits,
//! the connection closes, and the temp file is renamed into place atomically. A
//! builder dropped before publishing removes its temp file, so an interrupted
//! build leaves any prior corpus at the destination untouched and the new path
//! absent.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use lean_semantic_search_contract::DeclarationFeatureRow;
use lean_semantic_search_retrieval::RETRIEVAL_POLICY_VERSION;
use rusqlite::{Connection, params};

use crate::StoreError;
use crate::schema::{
    self, META_CORPUS_TOKEN, META_POLICY_VERSION, META_SCHEMA_VERSION, META_TOTAL_DOCUMENTS, STORE_SCHEMA_VERSION,
};

/// One item in the build stream.
///
/// The two variants are paired by `declaration_id`: a declaration enters the
/// corpus only when its identity has been announced *and* its feature row has
/// arrived. The store keeps only the feature row; the announcement carries no
/// data of its own — it records that the extractor accepted the declaration,
/// so a feature row for a later-filtered declaration is never indexed alone.
pub enum Ingest {
    /// A declaration the extractor accepted, identified by its opaque id.
    Declaration(String),
    /// A declaration's feature row.
    Feature(DeclarationFeatureRow),
}

/// Builds a persisted corpus, then publishes it atomically.
pub struct StoreBuilder {
    connection: Option<Connection>,
    temp_path: PathBuf,
    final_path: PathBuf,
    corpus_token: String,
    announced: HashSet<String>,
    awaiting: HashMap<String, DeclarationFeatureRow>,
    total_documents: usize,
}

impl StoreBuilder {
    /// Open a fresh build at a temp path beside `path`, ready to ingest.
    ///
    /// `corpus_token` is an opaque, caller-supplied content identity stored
    /// verbatim; the store never interprets it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] if a stale temp file cannot be cleared, or
    /// [`StoreError::Sqlite`] if the connection, pragmas, schema, or opening
    /// transaction fail.
    pub fn create(path: impl AsRef<Path>, corpus_token: impl Into<String>) -> Result<Self, StoreError> {
        let final_path = path.as_ref().to_path_buf();
        let temp_path = building_path(&final_path);
        if temp_path.exists() {
            std::fs::remove_file(&temp_path)?;
        }
        let connection = Connection::open(&temp_path)?;
        schema::initialize(&connection)?;
        // One transaction for the whole build; commit happens at publish.
        connection.execute_batch("BEGIN")?;
        Ok(Self {
            connection: Some(connection),
            temp_path,
            final_path,
            corpus_token: corpus_token.into(),
            announced: HashSet::new(),
            awaiting: HashMap::new(),
            total_documents: 0,
        })
    }

    /// Ingest one stream item, writing a declaration as soon as both its halves
    /// have arrived.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Json`] if a feature row cannot be serialized, or
    /// [`StoreError::Sqlite`] if an insert fails. [`StoreError::Closed`] if the
    /// builder has already been published.
    pub fn accept(&mut self, item: Ingest) -> Result<(), StoreError> {
        match item {
            Ingest::Declaration(declaration_id) => {
                if let Some(row) = self.awaiting.remove(&declaration_id) {
                    self.write_row(&row)?;
                } else {
                    self.announced.insert(declaration_id);
                }
            }
            Ingest::Feature(row) => {
                if self.announced.remove(&row.declaration_id) {
                    self.write_row(&row)?;
                } else {
                    self.awaiting.insert(row.declaration_id.clone(), row);
                }
            }
        }
        Ok(())
    }

    /// Finalize: write metadata, commit, close, and rename the temp file into
    /// place. Returns the published path.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Sqlite`] if the metadata write or commit fails, or
    /// [`StoreError::Io`] if closing or renaming fails. On any error the temp
    /// build is discarded and the destination is left untouched.
    pub fn publish(mut self) -> Result<PathBuf, StoreError> {
        let connection = self.connection.take().ok_or(StoreError::Closed)?;
        write_metadata(&connection, META_SCHEMA_VERSION, STORE_SCHEMA_VERSION)?;
        write_metadata(&connection, META_POLICY_VERSION, RETRIEVAL_POLICY_VERSION)?;
        write_metadata(&connection, META_CORPUS_TOKEN, &self.corpus_token)?;
        write_metadata(&connection, META_TOTAL_DOCUMENTS, &self.total_documents.to_string())?;
        connection.execute_batch("COMMIT")?;
        connection.close().map_err(|(_, error)| StoreError::Sqlite(error))?;
        std::fs::rename(&self.temp_path, &self.final_path)?;
        Ok(self.final_path.clone())
    }

    fn write_row(&mut self, row: &DeclarationFeatureRow) -> Result<(), StoreError> {
        let connection = self.connection.as_ref().ok_or(StoreError::Closed)?;
        let row_json = serde_json::to_string(row)?;
        connection
            .prepare_cached(
                "INSERT OR REPLACE INTO feature_rows(declaration_id, feature_version, row_json) VALUES (?1, ?2, ?3)",
            )?
            .execute(params![row.declaration_id, row.feature_version, row_json])?;
        let mut posting =
            connection.prepare_cached("INSERT OR IGNORE INTO postings(key, declaration_id) VALUES (?1, ?2)")?;
        for key in posting_keys(row) {
            posting.execute(params![key, row.declaration_id])?;
        }
        self.total_documents = self.total_documents.saturating_add(1);
        Ok(())
    }
}

impl Drop for StoreBuilder {
    fn drop(&mut self) {
        // Close any still-open build connection so its rollback completes and
        // the handle is released before the temp file is touched.
        if let Some(connection) = self.connection.take() {
            connection.close().ok();
        }
        // Remove the temp build unless it was already renamed away by publish.
        std::fs::remove_file(&self.temp_path).ok();
    }
}

/// The unique opaque keys a row contributes to the inverted index: its four
/// fingerprints and every role-feature key, deduplicated exactly as the
/// in-memory reference index does so fanout counts match.
fn posting_keys(row: &DeclarationFeatureRow) -> HashSet<&str> {
    let mut keys: HashSet<&str> = HashSet::with_capacity(row.role_features.len().saturating_add(4));
    keys.insert(row.fingerprints.statement.as_str());
    keys.insert(row.fingerprints.safe_binder_permutation.as_str());
    keys.insert(row.fingerprints.connective_shape.as_str());
    keys.insert(row.fingerprints.conclusion_shape.as_str());
    for feature in &row.role_features {
        keys.insert(feature.key.as_str());
    }
    keys
}

fn write_metadata(connection: &Connection, key: &str, value: &str) -> Result<(), StoreError> {
    connection
        .prepare_cached("INSERT OR REPLACE INTO metadata(key, value) VALUES (?1, ?2)")?
        .execute(params![key, value])?;
    Ok(())
}

/// The sibling temp path a build writes to before it is renamed into place.
fn building_path(final_path: &Path) -> PathBuf {
    let mut name = final_path.file_name().map_or_else(OsString::new, OsString::from);
    name.push(".building");
    final_path.with_file_name(name)
}
