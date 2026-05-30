//! The freshness verdict: may a persisted corpus be reused for a request?
//!
//! Reuse is allowed only when the stored opaque `corpus_token` equals the
//! caller's supplied token *and* the store's own `schema_version` and the
//! retrieval `policy_version` equal the running ones. Any mismatch — and any
//! corruption that keeps the file from opening — is a structured [`CacheMiss`],
//! not an error: the caller rebuilds. The store compares the token but never
//! interprets it; the ingredients folded into it (Lake files, source digests,
//! toolchain, roots) belong to the caller. See
//! `docs/architecture/06-cache-lifecycle.md`.

use std::path::Path;

use lean_semantic_search_retrieval::RETRIEVAL_POLICY_VERSION;

use crate::StoreError;
use crate::lifecycle;
use crate::read::Store;

/// Why a persisted corpus cannot be reused for a request. Every variant is a
/// cache miss that tells the caller to rebuild — never a transport error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CacheMiss {
    /// No corpus exists at the path (or no corpus is published under the root).
    Missing,
    /// The stored `corpus_token` differs from the caller's supplied token.
    TokenMismatch,
    /// The stored `schema_version` is not the one this build reads.
    SchemaDrift,
    /// The stored retrieval `policy_version` is not the one this build ranks under.
    PolicyDrift,
    /// The file could not be opened or its required metadata could not be read.
    Corrupt,
}

/// The outcome of an open-or-reject: either a usable corpus or the reason it
/// was rejected.
pub enum CorpusLookup {
    /// The corpus is compatible and was opened.
    Fresh(Store),
    /// The corpus cannot be reused; the caller rebuilds.
    Stale(CacheMiss),
}

impl std::fmt::Debug for CorpusLookup {
    // `Store` wraps a connection and is not `Debug`; show the verdict, not the
    // open corpus, so a test can print a lookup without exposing the handle.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fresh(_) => formatter.write_str("Fresh(..)"),
            Self::Stale(miss) => write!(formatter, "Stale({miss:?})"),
        }
    }
}

impl Store {
    /// Open `path` and accept it only if its `corpus_token` matches
    /// `expected_token` and its `schema_version` and `policy_version` match the
    /// running ones.
    ///
    /// Returns [`CorpusLookup::Fresh`] with the opened store on a full match, or
    /// [`CorpusLookup::Stale`] with the specific [`CacheMiss`] otherwise. A
    /// missing file, a version or token mismatch, and a damaged file are all
    /// cache misses — this never errors and never panics, so the caller's only
    /// response is to rebuild.
    #[must_use]
    pub fn open_fresh(path: impl AsRef<Path>, expected_token: &str) -> CorpusLookup {
        let path = path.as_ref();
        if !path.exists() {
            return CorpusLookup::Stale(CacheMiss::Missing);
        }
        match Self::open(path) {
            Ok(store) => {
                if store.corpus_token() != expected_token {
                    CorpusLookup::Stale(CacheMiss::TokenMismatch)
                } else if store.policy_version() != RETRIEVAL_POLICY_VERSION {
                    CorpusLookup::Stale(CacheMiss::PolicyDrift)
                } else {
                    CorpusLookup::Fresh(store)
                }
            }
            Err(StoreError::SchemaMismatch { .. }) => CorpusLookup::Stale(CacheMiss::SchemaDrift),
            // A missing or unparseable metadata fact, an unreadable file, or any
            // other open failure means the artifact cannot be trusted: a cache
            // miss, never a hard error that strands the caller.
            Err(_) => CorpusLookup::Stale(CacheMiss::Corrupt),
        }
    }
}

/// Resolve the latest published corpus under `root` and open-or-reject it
/// against `expected_token`.
///
/// Composes the neutral latest-pointer primitive
/// ([`lifecycle::latest_index_path`]) with the store-side freshness verdict
/// ([`Store::open_fresh`]). An unset or unreadable pointer is
/// [`CacheMiss::Missing`] — the same rebuild-triggering cache miss as an absent
/// file.
#[must_use]
pub fn open_latest_fresh(root: &Path, expected_token: &str) -> CorpusLookup {
    match lifecycle::latest_index_path(root) {
        Some(index) => Store::open_fresh(index, expected_token),
        None => CorpusLookup::Stale(CacheMiss::Missing),
    }
}
