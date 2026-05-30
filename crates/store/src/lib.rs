//! Persistent `SQLite`-backed semantic index implementing the retrieval
//! [`Corpus`](lean_semantic_search_retrieval::Corpus) seam.
//!
//! This crate is the large-corpus counterpart to the in-memory inverted index
//! in `lean-semantic-search-retrieval`. It owns the **semantic index only**:
//! opaque-key postings, per-key fanout, the document total, and the contract
//! [`DeclarationFeatureRow`](lean_semantic_search_contract::DeclarationFeatureRow)s
//! needed to rebuild an anchor from a corpus member. It carries no declaration
//! display text, module or kind fields, provenance, labels, probe caches, or any
//! duplicate-audit or proof-agent vocabulary — those stay with consumers.
//!
//! Build a corpus with [`StoreBuilder`], publishing it atomically; open it
//! read-only with [`Store`], which implements `Corpus` so retrieval ranks over a
//! persisted index without loading it into memory. The ranking algorithm, anchor
//! planning, policy, and output shape are unchanged: a `Store` is just another
//! `Corpus`, and `retrieve_across` fans one anchor across several of them.
//!
//! See `docs/architecture/05-sqlite-store.md` for the schema and the read/write
//! design.

mod read;
mod schema;
mod write;

pub use read::Store;
pub use schema::STORE_SCHEMA_VERSION;
pub use write::{Ingest, StoreBuilder};

use std::fmt;

/// An error from building or opening a persisted corpus.
///
/// The [`Corpus`](lean_semantic_search_retrieval::Corpus) read methods are
/// infallible by trait contract — a `Store` validates its schema and metadata at
/// [`Store::open`], so subsequent reads degrade to empty results rather than
/// surfacing an error. This type covers only the fallible build and open steps.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// A `SQLite` operation failed.
    Sqlite(rusqlite::Error),
    /// A filesystem operation failed.
    Io(std::io::Error),
    /// A feature row could not be serialized to JSON.
    Json(serde_json::Error),
    /// The opened store's schema version is not the one this build understands.
    SchemaMismatch {
        /// The schema version stored in the file.
        found: String,
        /// The schema version this build writes and reads.
        expected: &'static str,
    },
    /// A required metadata fact was absent from the opened store.
    MissingMetadata(String),
    /// A stored metadata value was present but unparseable.
    Corrupt(&'static str),
    /// The builder was used after it had already been published.
    Closed,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "sqlite error: {error}"),
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::Json(error) => write!(formatter, "json error: {error}"),
            Self::SchemaMismatch { found, expected } => {
                write!(formatter, "store schema version {found} is not the expected {expected}")
            }
            Self::MissingMetadata(key) => write!(formatter, "store is missing required metadata: {key}"),
            Self::Corrupt(key) => write!(formatter, "store metadata value is corrupt: {key}"),
            Self::Closed => write!(formatter, "store builder has already been published"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::SchemaMismatch { .. } | Self::MissingMetadata(_) | Self::Corrupt(_) | Self::Closed => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
