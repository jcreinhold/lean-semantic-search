//! The in-memory reference [`Corpus`]: an inverted view of a candidate corpus.
//!
//! [`SemanticIndex`] maps each opaque feature key to the declarations that carry
//! it, keeps the rows themselves so a member can be rebuilt into an anchor, and
//! records the document count rarity weighting needs. It is built entirely from
//! declaration feature rows held in memory; it owns no storage, no cache, and no
//! on-disk layout, so any caller can build one from rows it obtained however it
//! likes. The dense row indices are this backend's private bookkeeping — the
//! [`Corpus`] surface speaks only document totals, fanout counts, declaration
//! ids, and feature rows, all of which a per-key SQL backend can answer too.

use std::collections::{HashMap, HashSet};

use lean_semantic_search_contract::{DeclarationFeatureRow, OpaqueFeatureKey};

use crate::Corpus;

/// An in-memory semantic index over candidate declarations.
///
/// Build one with [`SemanticIndex::from_declarations`] and rank against it with
/// [`retrieve_across`](crate::retrieve_across). The index is storage-neutral: it
/// is a view over rows the caller already holds, never a database.
pub struct SemanticIndex {
    rows: Vec<DeclarationFeatureRow>,
    postings: HashMap<OpaqueFeatureKey, Vec<usize>>,
    by_id: HashMap<String, usize>,
    total_documents: usize,
}

impl SemanticIndex {
    /// Build an index from candidate declaration feature rows. Each row's four
    /// fingerprints and every role-feature key become lookup keys; per key, a
    /// declaration is counted once. The rows are retained so a member can be
    /// rebuilt into an anchor.
    #[must_use]
    pub fn from_declarations(rows: &[DeclarationFeatureRow]) -> Self {
        let mut postings: HashMap<OpaqueFeatureKey, Vec<usize>> = HashMap::new();
        let mut by_id: HashMap<String, usize> = HashMap::new();

        for (index, row) in rows.iter().enumerate() {
            by_id.insert(row.declaration_id.clone(), index);

            let mut keys: HashSet<&OpaqueFeatureKey> = HashSet::new();
            keys.insert(&row.fingerprints.statement);
            keys.insert(&row.fingerprints.safe_binder_permutation);
            keys.insert(&row.fingerprints.connective_shape);
            keys.insert(&row.fingerprints.conclusion_shape);
            for feature in &row.role_features {
                keys.insert(&feature.key);
            }
            for key in keys {
                postings.entry(key.clone()).or_default().push(index);
            }
        }

        let total_documents = rows.len();
        Self {
            rows: rows.to_vec(),
            postings,
            by_id,
            total_documents,
        }
    }

    /// Number of candidate declarations in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the index holds no candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl Corpus for SemanticIndex {
    fn document_total(&self) -> usize {
        self.total_documents
    }

    fn fanout(&self, keys: &[OpaqueFeatureKey]) -> Vec<usize> {
        keys.iter()
            .map(|key| self.postings.get(key).map_or(0, Vec::len))
            .collect()
    }

    fn postings(&self, key: &OpaqueFeatureKey, limit: usize) -> Vec<String> {
        let Some(indices) = self.postings.get(key) else {
            return Vec::new();
        };
        indices
            .iter()
            .take(limit)
            .filter_map(|&index| self.rows.get(index))
            .map(|row| row.declaration_id.clone())
            .collect()
    }

    fn declaration_row(&self, declaration_id: &str) -> Option<DeclarationFeatureRow> {
        let index = *self.by_id.get(declaration_id)?;
        self.rows.get(index).cloned()
    }
}
