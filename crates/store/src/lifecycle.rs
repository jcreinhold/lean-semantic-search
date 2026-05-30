//! Neutral filesystem primitives over published corpus directories: a
//! content-addressed layout, an atomic latest-pointer, and a caller-driven
//! cleanup.
//!
//! Nothing here opens a corpus or knows what one contains. A corpus is a
//! directory under a `root`, named by a caller-chosen content address the store
//! never interprets; its index file lives inside at a fixed, private name. The
//! `latest` pointer is a small file naming the active directory, repointed by an
//! atomic rename so a reader resolving it sees the old or the new target, never
//! a torn or half-written one. Cleanup keeps the directories the caller still
//! wants and the active pointer's target, and removes the rest only when asked.
//! See `docs/architecture/06-cache-lifecycle.md`.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::StoreError;

/// The file a build publishes its index to, inside a corpus directory. Private:
/// the on-disk layout is the store's to choose and change.
const INDEX_FILE: &str = "index.sqlite";

/// The pointer file naming the active corpus directory under a root.
const POINTER_FILE: &str = "latest";

/// The temp name a pointer update writes before renaming over [`POINTER_FILE`].
const POINTER_TEMP: &str = "latest.publishing";

/// The directory a corpus with content address `name` occupies under `root`.
/// The caller owns the name; the store never parses it.
#[must_use]
pub fn corpus_dir(root: &Path, name: &str) -> PathBuf {
    root.join(name)
}

/// The index file a builder writes to (and a reader opens) for corpus `name`.
/// Pass it to [`StoreBuilder::create`](crate::StoreBuilder::create) and
/// [`Store::open_fresh`](crate::Store::open_fresh).
#[must_use]
pub fn index_path(root: &Path, name: &str) -> PathBuf {
    corpus_dir(root, name).join(INDEX_FILE)
}

/// Atomically publish corpus `name` as the latest under `root`.
///
/// Writes the bare name to a temp file beside the pointer and renames it over
/// any existing pointer; on one filesystem the rename is atomic, so a
/// concurrent reader observes either the old target or the new one, never a
/// torn or empty pointer. The pointer is intentionally a separate file, not a
/// row in the index, so the index's metadata stays exactly its four facts.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if `root` cannot be created or the pointer cannot
/// be written or renamed.
pub fn set_latest(root: &Path, name: &str) -> Result<(), StoreError> {
    fs::create_dir_all(root)?;
    let temp = root.join(POINTER_TEMP);
    fs::write(&temp, name)?;
    fs::rename(&temp, root.join(POINTER_FILE))?;
    Ok(())
}

/// The content address the `latest` pointer under `root` names, or `None` if no
/// pointer is published or it is unreadable.
///
/// A pointer whose contents are not a single safe directory component (empty,
/// absolute, or containing a separator or `..`) reads as `None`: the name is
/// still treated as opaque for equality, but a value that is not one directory
/// component is refused rather than followed out of `root`.
#[must_use]
pub fn latest_name(root: &Path) -> Option<String> {
    let raw = fs::read_to_string(root.join(POINTER_FILE)).ok()?;
    let name = raw.trim();
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(only)), None) if only == name => Some(name.to_owned()),
        _ => None,
    }
}

/// The index path the `latest` pointer under `root` resolves to, ready to open,
/// or `None` if nothing is published.
#[must_use]
pub fn latest_index_path(root: &Path) -> Option<PathBuf> {
    latest_name(root).map(|name| index_path(root, &name))
}

/// Whether a [`cleanup`] reports its plan or carries it out.
#[derive(Debug, Clone, Copy)]
pub enum CleanupMode {
    /// Report what would be removed without touching the filesystem.
    DryRun,
    /// Remove the unprotected directories.
    Execute,
}

/// What a [`cleanup`] found and, if executed, did.
#[derive(Debug)]
pub struct CleanupReport {
    /// Whether the unprotected directories were actually removed.
    pub executed: bool,
    /// Directories not wanted by the caller and not the active pointer target.
    pub removable: Vec<CleanupEntry>,
    /// Directories kept: wanted by the caller, or the active pointer target.
    pub protected: Vec<CleanupEntry>,
    /// Total bytes the removable directories occupy.
    pub bytes_removable: u64,
}

/// One corpus directory a [`cleanup`] classified.
#[derive(Debug)]
pub struct CleanupEntry {
    /// The corpus directory.
    pub dir: PathBuf,
    /// Bytes it occupies on disk.
    pub bytes: u64,
    /// Why it was kept or marked removable, in stable words.
    pub reason: &'static str,
}

/// Remove every corpus directory under `root` except those the caller still
/// wants and the one the `latest` pointer targets.
///
/// `keep` lists the content addresses to retain; the active pointer's target is
/// always protected even if absent from `keep`, so a rebuild that publishes
/// before cleaning never strands a concurrent reader. [`CleanupMode::DryRun`]
/// computes the plan without deleting; [`CleanupMode::Execute`] removes the
/// removable directories. Either way the report lists removable and protected
/// directories and the bytes at stake, so a caller inspects before deleting.
///
/// # Errors
///
/// Returns [`StoreError::Io`] if `root` cannot be enumerated or a removal fails.
pub fn cleanup(root: &Path, keep: &[&str], mode: CleanupMode) -> Result<CleanupReport, StoreError> {
    let execute = matches!(mode, CleanupMode::Execute);
    let kept: HashSet<&str> = keep.iter().copied().collect();
    let active = latest_name(root);

    let mut removable = Vec::new();
    let mut protected = Vec::new();
    let mut bytes_removable: u64 = 0;

    if !root.exists() {
        return Ok(CleanupReport {
            executed: execute,
            removable,
            protected,
            bytes_removable,
        });
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let dir = entry.path();
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        let bytes = directory_bytes(&dir);
        if kept.contains(name.as_str()) {
            protected.push(CleanupEntry {
                dir,
                bytes,
                reason: "kept by caller",
            });
        } else if active.as_deref() == Some(name.as_str()) {
            protected.push(CleanupEntry {
                dir,
                bytes,
                reason: "active latest pointer",
            });
        } else {
            bytes_removable = bytes_removable.saturating_add(bytes);
            removable.push(CleanupEntry {
                dir,
                bytes,
                reason: "not wanted and not the latest pointer",
            });
        }
    }

    if execute {
        for entry in &removable {
            fs::remove_dir_all(&entry.dir)?;
        }
    }

    Ok(CleanupReport {
        executed: execute,
        removable,
        protected,
        bytes_removable,
    })
}

/// Sum of file sizes under `dir`, following the tree. Unreadable entries
/// contribute zero rather than failing — the figure is for a human's inspection,
/// not an accounting guarantee.
fn directory_bytes(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let Ok(entries) = fs::read_dir(dir) else {
        return total;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            total = total.saturating_add(directory_bytes(&entry.path()));
        } else if let Ok(metadata) = entry.metadata() {
            total = total.saturating_add(metadata.len());
        }
    }
    total
}
