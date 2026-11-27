//! Shared fixtures for the store's integration tests. Compiled into each test
//! binary separately, so some helpers are unused per binary.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lean_semantic_search_contract::{
    DeclarationFeatureRow, Fingerprints, OpaqueFeatureKey, RoleFeature, SEMANTIC_FEATURE_VERSION, SourcePosition,
    SourceSpan,
};
use lean_semantic_search_store::{Ingest, Store, StoreBuilder};

#[must_use]
pub(crate) fn key(value: &str) -> OpaqueFeatureKey {
    OpaqueFeatureKey::new(value)
}

#[must_use]
pub(crate) fn fingerprints(prefix: &str) -> Fingerprints {
    Fingerprints {
        statement: key(&format!("canon-{prefix}-stmt")),
        safe_binder_permutation: key(&format!("canon-{prefix}-safe")),
        connective_shape: key(&format!("canon-{prefix}-conn")),
        conclusion_shape: key(&format!("canon-{prefix}-concl")),
    }
}

#[must_use]
pub(crate) fn role(role: &str, key_value: &str, display: &str) -> RoleFeature {
    RoleFeature {
        role: role.to_owned(),
        key: key(key_value),
        display: Some(display.to_owned()),
    }
}

#[must_use]
pub(crate) fn declaration(
    id: &str,
    fingerprints: Fingerprints,
    role_features: Vec<RoleFeature>,
) -> DeclarationFeatureRow {
    DeclarationFeatureRow {
        declaration_id: id.to_owned(),
        feature_version: SEMANTIC_FEATURE_VERSION.to_owned(),
        fingerprints,
        role_features,
        binder_count: 0,
        low_signal_markers: Vec::new(),
        source: None,
    }
}

/// A row exercising every field — markers and a source span — so a round-trip
/// has something non-trivial to preserve.
#[must_use]
pub(crate) fn rich_declaration(id: &str) -> DeclarationFeatureRow {
    DeclarationFeatureRow {
        declaration_id: id.to_owned(),
        feature_version: SEMANTIC_FEATURE_VERSION.to_owned(),
        fingerprints: fingerprints(id),
        role_features: vec![
            role("conclusion_const", &format!("rk-{id}-c"), "Foo"),
            role("hypothesis_head", &format!("rk-{id}-h"), "Eq"),
        ],
        binder_count: 3,
        low_signal_markers: vec!["broad_head:Eq".to_owned()],
        source: Some(SourceSpan {
            start: SourcePosition { line: 12, column: 3 },
            end: SourcePosition { line: 14, column: 8 },
        }),
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique temp path for one test, isolated by process id and a counter so
/// parallel tests never collide.
#[must_use]
pub(crate) fn temp_path(tag: &str) -> PathBuf {
    let serial = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("lss-store-{}-{tag}-{serial}.sqlite", std::process::id()));
    path
}

/// Build and publish a corpus from rows (declaration announced, then featured),
/// then open it read-only.
///
/// # Errors
///
/// Returns the stringified store error if building, publishing, or opening fails.
pub(crate) fn build_store(path: &Path, corpus_token: &str, rows: &[DeclarationFeatureRow]) -> Result<Store, String> {
    let mut builder = StoreBuilder::create(path, corpus_token).map_err(|error| error.to_string())?;
    for row in rows {
        builder
            .accept(Ingest::Declaration(row.declaration_id.clone()))
            .map_err(|error| error.to_string())?;
        builder
            .accept(Ingest::Feature(row.clone()))
            .map_err(|error| error.to_string())?;
    }
    let published = builder.publish().map_err(|error| error.to_string())?;
    Store::open(published).map_err(|error| error.to_string())
}
