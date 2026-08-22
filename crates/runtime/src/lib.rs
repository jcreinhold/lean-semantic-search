//! Package-owned runtime for the `LeanSemanticSearch` Lean capability.
//!
//! Downstream hosts depend on this crate instead of copying Lean source into
//! their own repositories. The crate owns the runtime payload, source digest,
//! cache materialization, provenance sidecar, and explicit-sysroot Lake build.
//! It does not own worker sessions or import policy; callers load the returned
//! [`LeanBuiltCapability`] with their
//! chosen `lean-rs-worker-parent` configuration.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "cache tests and digest accounting use bounded local path and byte counts"
)]

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use lean_rs_worker_protocol::worker_exports::{
    doctor_signature, json_command_signature, metadata_signature, streaming_command_signature,
};
use lean_toolchain::{
    CargoLeanCapability, GeneratedSourceFile, LeanBuiltCapability, LeanBuiltCapabilityError, LeanExportSignature,
    LeanLibraryDependency, LinkDiagnostics, SourcePackageError, SourcePackageManifestPolicy,
    SourcePackageMaterializationRequest, materialize_source_package as materialize_with_toolchain,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const RUNTIME_SOURCE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/runtime");
const SIDECAR_FILE_NAME: &str = "semantic-search-runtime.json";
const CACHE_SCHEMA_VERSION: u32 = 1;

/// Runtime source revision packaged by this crate.
pub const SOURCE_REVISION: &str = "f504ad7616de785fe5cbf6f9d41684f9bd552e23";
/// Digest of the runtime Lean payload, using the file-set rule in
/// `lean/VENDORING.md`.
pub const RUNTIME_SOURCE_DIGEST: &str = "b190343cbf8b34fa754a6e2488e5bc5d22bb360900dd91b5bfa9eb431da193fc";
/// Lake package name owned by the runtime payload.
pub const PACKAGE_NAME: &str = "lean-semantic-search";
const MATERIALIZED_PACKAGE_NAME: &str = "lean_semantic_search";
/// Lean library and root module name owned by the runtime payload.
pub const LIBRARY_NAME: &str = "LeanSemanticSearch";

/// Request to build the packaged runtime for one Lean toolchain.
///
/// `cache_root` is caller-owned. This crate owns the layout below it, keyed by
/// source digest and requested toolchain label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSearchRuntimeBuild {
    /// Caller-owned cache root.
    pub cache_root: PathBuf,
    /// Lean toolchain label to write into the generated `lean-toolchain`.
    pub toolchain_label: String,
    /// Lean sysroot whose `bin/lake` and `LEAN_SYSROOT` must be used for the
    /// Lake build.
    pub lean_sysroot: PathBuf,
}

/// Request to materialize the packaged runtime as a Lake source package.
///
/// This exists for downstream capabilities, such as `lean-dup`, that need to
/// depend on `LeanSemanticSearch` from their own Lake project. It does not
/// build the package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSearchSourcePackageRequest {
    /// Caller-owned cache root.
    pub cache_root: PathBuf,
    /// Lean toolchain label to write into the generated `lean-toolchain`.
    pub toolchain_label: String,
}

/// Built semantic-search runtime capability plus package provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSearchRuntime {
    /// Built Lean capability descriptor for `LeanSemanticSearch`.
    pub built: LeanBuiltCapability,
    /// Runtime payload provenance for diagnostics and cache validation.
    pub provenance: SemanticSearchRuntimeProvenance,
}

impl SemanticSearchRuntime {
    /// Return the loader dependency descriptor for hosts that link another
    /// capability against `LeanSemanticSearch`.
    ///
    /// This keeps the materialized package identifier, root module, and dylib
    /// path as runtime-owned facts instead of making consumers reconstruct the
    /// descriptor from provenance fields.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the built capability descriptor cannot resolve
    /// its dylib path.
    pub fn dependency(&self) -> Result<LeanLibraryDependency, Error> {
        let dylib_path = self
            .built
            .dylib_path()
            .map_err(|source| Error::BuiltCapability { source })?;
        Ok(LeanLibraryDependency::path(dylib_path)
            .export_symbols_for_dependents()
            .initializer(MATERIALIZED_PACKAGE_NAME, LIBRARY_NAME))
    }
}

/// Materialized semantic-search source package plus provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSearchSourcePackage {
    /// Materialized Lake project root. The caller should treat the path as an
    /// opaque dependency root owned by this crate.
    pub project_root: PathBuf,
    /// Runtime payload provenance for diagnostics and cache validation.
    pub provenance: SemanticSearchRuntimeProvenance,
}

/// Provenance recorded beside each materialized runtime package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SemanticSearchRuntimeProvenance {
    /// Sidecar schema version.
    pub schema_version: u32,
    /// Upstream source revision.
    pub source_revision: String,
    /// Runtime source digest.
    pub runtime_source_digest: String,
    /// Lake package name.
    pub package: String,
    /// Cache-private Lake package identifier used for building the materialized
    /// package with current `lean-toolchain` output resolution.
    pub materialized_package: String,
    /// Lean library/root module name.
    pub library: String,
    /// Requested Lean toolchain label.
    pub toolchain_label: String,
    /// Runtime crate version.
    pub crate_version: String,
    /// Whether `lean-toolchain` was generated during materialization.
    pub generated_toolchain_file: bool,
}

impl SemanticSearchRuntimeProvenance {
    fn new(toolchain_label: &str) -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            source_revision: SOURCE_REVISION.to_owned(),
            runtime_source_digest: RUNTIME_SOURCE_DIGEST.to_owned(),
            package: PACKAGE_NAME.to_owned(),
            materialized_package: MATERIALIZED_PACKAGE_NAME.to_owned(),
            library: LIBRARY_NAME.to_owned(),
            toolchain_label: toolchain_label.to_owned(),
            crate_version: env!("CARGO_PKG_VERSION").to_owned(),
            generated_toolchain_file: true,
        }
    }
}

/// Runtime crate errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Filesystem operation failed.
    #[error("{action} {}: {source}", path.display())]
    Io {
        /// Operation being attempted.
        action: &'static str,
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// JSON sidecar or manifest operation failed.
    #[error("{action} {}: {source}", path.display())]
    Json {
        /// Operation being attempted.
        action: &'static str,
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// Runtime payload invariant failed.
    #[error("invalid semantic-search runtime payload: {0}")]
    InvalidRuntimePayload(String),
    /// Shared source-package materialization failed.
    #[error("semantic-search runtime source materialization failed: {source}")]
    SourcePackage {
        /// Source-package materialization error.
        #[from]
        source: SourcePackageError,
    },
    /// Lake capability build failed.
    #[error("semantic-search runtime build failed for toolchain {toolchain_label}: {source}")]
    Build {
        /// Requested toolchain label.
        toolchain_label: String,
        /// Lean/Lake diagnostic.
        #[source]
        source: LinkDiagnostics,
    },
    /// Built runtime descriptor was incomplete or invalid.
    #[error("semantic-search runtime built capability descriptor is invalid: {source}")]
    BuiltCapability {
        /// Descriptor resolution failure.
        #[source]
        source: LeanBuiltCapabilityError,
    },
}

/// Build the packaged semantic-search runtime capability for one toolchain.
///
/// # Errors
///
/// Returns [`Error`] when materialization, cache validation, or the explicit
/// sysroot Lake build fails.
pub fn build_cached(input: SemanticSearchRuntimeBuild) -> Result<SemanticSearchRuntime, Error> {
    let package = materialize_source_package(SemanticSearchSourcePackageRequest {
        cache_root: input.cache_root,
        toolchain_label: input.toolchain_label.clone(),
    })?;
    let _build_lock = lock_runtime_build(&package.project_root)?;
    let mut builder = CargoLeanCapability::new(&package.project_root, LIBRARY_NAME)
        .package(MATERIALIZED_PACKAGE_NAME)
        .module(LIBRARY_NAME)
        .lean_sysroot(input.lean_sysroot);
    for signature in export_signatures() {
        builder = builder.export_signature(signature);
    }
    let built = builder.build_quiet().map_err(|source| Error::Build {
        toolchain_label: input.toolchain_label,
        source,
    })?;
    Ok(SemanticSearchRuntime {
        built: (&built).into(),
        provenance: package.provenance,
    })
}

/// Materialize the packaged runtime source package without building it.
///
/// # Errors
///
/// Returns [`Error`] when cache materialization or provenance validation fails.
pub fn materialize_source_package(
    input: SemanticSearchSourcePackageRequest,
) -> Result<SemanticSearchSourcePackage, Error> {
    ensure_runtime_payload_source(Path::new(RUNTIME_SOURCE_ROOT))?;
    let provenance = SemanticSearchRuntimeProvenance::new(&input.toolchain_label);
    let request = source_package_request(input.cache_root, &input.toolchain_label, &provenance)?;
    let materialized = materialize_with_toolchain(&request)?;
    Ok(SemanticSearchSourcePackage {
        project_root: materialized.project_root,
        provenance,
    })
}

/// Compute the packaged runtime source digest using the `lean/VENDORING.md`
/// file-set rule.
///
/// # Errors
///
/// Returns [`Error`] if the packaged runtime files cannot be read.
pub fn compute_runtime_source_digest() -> Result<String, Error> {
    compute_runtime_source_digest_from(Path::new(RUNTIME_SOURCE_ROOT))
}

fn export_signatures() -> [LeanExportSignature; 5] {
    [
        metadata_signature(lean_semantic_search_capability::METADATA_EXPORT),
        doctor_signature(lean_semantic_search_capability::DOCTOR_EXPORT),
        json_command_signature(lean_semantic_search_capability::DECLARATION_FEATURES_EXPORT),
        json_command_signature(lean_semantic_search_capability::PROOF_GOAL_FEATURES_EXPORT),
        streaming_command_signature(lean_semantic_search_capability::STREAM_DECLARATION_FEATURES_EXPORT),
    ]
}

struct BuildLock {
    _file: File,
}

fn lock_runtime_build(project_root: &Path) -> Result<BuildLock, Error> {
    let path = project_root.join(".semantic-search-runtime-build.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| Error::Io {
            action: "open semantic-search runtime build lock",
            path: path.clone(),
            source,
        })?;
    fs4::FileExt::lock(&file).map_err(|source| Error::Io {
        action: "lock semantic-search runtime build",
        path,
        source,
    })?;
    Ok(BuildLock { _file: file })
}

fn source_package_request(
    cache_root: PathBuf,
    toolchain_label: &str,
    provenance: &SemanticSearchRuntimeProvenance,
) -> Result<SourcePackageMaterializationRequest, Error> {
    Ok(SourcePackageMaterializationRequest {
        source_root: PathBuf::from(RUNTIME_SOURCE_ROOT),
        cache_root,
        package_name: PACKAGE_NAME.to_owned(),
        materialized_package_name: MATERIALIZED_PACKAGE_NAME.to_owned(),
        library_name: LIBRARY_NAME.to_owned(),
        source_digest: RUNTIME_SOURCE_DIGEST.to_owned(),
        source_revision: SOURCE_REVISION.to_owned(),
        crate_name: env!("CARGO_PKG_NAME").to_owned(),
        crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        toolchain_label: toolchain_label.to_owned(),
        include_paths: vec![PathBuf::from(".")],
        generated_files: vec![
            GeneratedSourceFile {
                relative_path: PathBuf::from("lakefile.lean"),
                contents: materialized_lakefile_bytes()?,
            },
            GeneratedSourceFile {
                relative_path: PathBuf::from("lake-manifest.json"),
                contents: materialized_manifest_bytes()?,
            },
            GeneratedSourceFile {
                relative_path: PathBuf::from(SIDECAR_FILE_NAME),
                contents: semantic_sidecar_bytes(provenance)?,
            },
        ],
        sentinel_files: [
            "lakefile.lean",
            "lake-manifest.json",
            "LeanSemanticSearch.lean",
            "LeanSemanticSearch/Capability.lean",
            "README.md",
            "VENDORING.md",
            "LICENSE-APACHE",
            "LICENSE-MIT",
            SIDECAR_FILE_NAME,
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        manifest_policy: SourcePackageManifestPolicy::ZeroPackages,
    })
}

fn semantic_sidecar_bytes(provenance: &SemanticSearchRuntimeProvenance) -> Result<Vec<u8>, Error> {
    let path = PathBuf::from(SIDECAR_FILE_NAME);
    let bytes = serde_json::to_vec_pretty(provenance).map_err(|source| Error::Json {
        action: "encode semantic-search runtime provenance sidecar",
        path,
        source,
    })?;
    Ok(bytes)
}

fn ensure_zero_package_manifest(path: &Path) -> Result<(), Error> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&read_file(path, "read semantic-search runtime lake-manifest.json")?).map_err(
            |source| Error::Json {
                action: "decode semantic-search runtime lake-manifest.json",
                path: path.to_path_buf(),
                source,
            },
        )?;
    let packages = manifest
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            Error::InvalidRuntimePayload("lake-manifest.json must contain an array `packages`".to_owned())
        })?;
    if !packages.is_empty() {
        return Err(Error::InvalidRuntimePayload(
            "lake-manifest.json must remain zero-dependency (`packages: []`)".to_owned(),
        ));
    }
    Ok(())
}

fn materialized_lakefile_bytes() -> Result<Vec<u8>, Error> {
    let lakefile = Path::new(RUNTIME_SOURCE_ROOT).join("lakefile.lean");
    let lakefile_text = read_to_string(&lakefile, "read materialized semantic-search lakefile")?;
    let rewritten = lakefile_text.replace(
        "package «lean-semantic-search» where",
        "package lean_semantic_search where",
    );
    Ok(rewritten.into_bytes())
}

fn materialized_manifest_bytes() -> Result<Vec<u8>, Error> {
    let manifest_path = Path::new(RUNTIME_SOURCE_ROOT).join("lake-manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&read_file(
        &manifest_path,
        "read materialized semantic-search manifest",
    )?)
    .map_err(|source| Error::Json {
        action: "decode materialized semantic-search manifest",
        path: manifest_path.clone(),
        source,
    })?;
    let object = manifest
        .as_object_mut()
        .ok_or_else(|| Error::InvalidRuntimePayload("lake-manifest.json root must be a JSON object".to_owned()))?;
    object.insert(
        "name".to_owned(),
        serde_json::Value::String(MATERIALIZED_PACKAGE_NAME.to_owned()),
    );
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|source| Error::Json {
        action: "encode materialized semantic-search manifest",
        path: manifest_path.clone(),
        source,
    })?;
    ensure_zero_package_manifest(&manifest_path)?;
    Ok(bytes)
}

fn ensure_runtime_payload_source(source_root: &Path) -> Result<(), Error> {
    for entry in WalkDir::new(source_root) {
        let entry = entry.map_err(|source| Error::InvalidRuntimePayload(format!("walk runtime payload: {source}")))?;
        let source_path = entry.path();
        let relative = source_path.strip_prefix(source_root).map_err(|source| {
            Error::InvalidRuntimePayload(format!(
                "compute runtime payload relative path for {}: {source}",
                source_path.display()
            ))
        })?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        if excluded_runtime_path(relative) {
            return Err(Error::InvalidRuntimePayload(format!(
                "packaged runtime contains excluded path {}",
                relative.display()
            )));
        }
    }
    ensure_zero_package_manifest(&source_root.join("lake-manifest.json"))?;
    Ok(())
}

fn compute_runtime_source_digest_from(source_root: &Path) -> Result<String, Error> {
    let mut entries = digest_entries(source_root)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut outer = Sha256::new();
    for (canonical_path, digest) in entries {
        outer.update(digest.as_bytes());
        outer.update(b"  ");
        outer.update(canonical_path.as_bytes());
        outer.update(b"\n");
    }
    Ok(hex_lower(&outer.finalize()))
}

fn digest_entries(source_root: &Path) -> Result<Vec<(String, String)>, Error> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(source_root) {
        let entry = entry.map_err(|source| Error::InvalidRuntimePayload(format!("walk runtime payload: {source}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let source_path = entry.path();
        let relative = source_path.strip_prefix(source_root).map_err(|source| {
            Error::InvalidRuntimePayload(format!(
                "compute runtime payload relative path for {}: {source}",
                source_path.display()
            ))
        })?;
        if excluded_runtime_path(relative) || matches!(relative.to_str(), Some("README.md" | "VENDORING.md")) {
            continue;
        }
        let canonical = canonical_digest_path(relative)?;
        let bytes = read_file(source_path, "read semantic-search runtime payload file for digest")?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        entries.push((canonical, hex_lower(&hasher.finalize())));
    }
    Ok(entries)
}

fn canonical_digest_path(relative: &Path) -> Result<String, Error> {
    let text = relative.to_string_lossy();
    if text == "LICENSE-APACHE" || text == "LICENSE-MIT" {
        return Ok(text.into_owned());
    }
    if text == "lakefile.lean" {
        return Ok("lean/lakefile.lean".to_owned());
    }
    if text == "lake-manifest.json" {
        return Ok("lean/lake-manifest.json".to_owned());
    }
    if text == "LeanSemanticSearch.lean" || text.starts_with("LeanSemanticSearch/") {
        return Ok(format!("lean/{text}"));
    }
    Err(Error::InvalidRuntimePayload(format!(
        "unexpected runtime payload path {}",
        relative.display()
    )))
}

fn excluded_runtime_path(relative: &Path) -> bool {
    relative
        .components()
        .any(|component| component.as_os_str() == ".lake" || component.as_os_str() == "LeanSemanticSearchTest")
        || relative == Path::new("lean-toolchain")
        || relative == Path::new("Main.lean")
        || relative == Path::new("LeanSemanticSearchTest.lean")
        || relative
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "olean" | "ilean" | "c" | "so" | "dylib" | "a"))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(hex_nibble(byte >> 4));
        out.push(hex_nibble(byte & 0x0f));
    }
    out
}

fn hex_nibble(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'a' + (nibble - 10)),
        _ => '?',
    }
}

fn read_file(path: &Path, action: &'static str) -> Result<Vec<u8>, Error> {
    fs::read(path).map_err(|source| Error::Io {
        action,
        path: path.to_path_buf(),
        source,
    })
}

fn read_to_string(path: &Path, action: &'static str) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|source| Error::Io {
        action,
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;

    use super::{
        LIBRARY_NAME, MATERIALIZED_PACKAGE_NAME, PACKAGE_NAME, RUNTIME_SOURCE_DIGEST, SIDECAR_FILE_NAME,
        SemanticSearchRuntimeProvenance, SemanticSearchSourcePackageRequest, compute_runtime_source_digest,
        materialize_source_package,
    };

    fn temp_cache() -> Result<tempfile::TempDir, String> {
        tempfile::tempdir().map_err(|error| error.to_string())
    }

    fn read_string(path: &Path) -> Result<String, String> {
        fs::read_to_string(path).map_err(|error| error.to_string())
    }

    #[test]
    fn runtime_source_digest_matches_recorded_value() -> Result<(), String> {
        let digest = compute_runtime_source_digest().map_err(|error| error.to_string())?;
        assert_eq!(digest, RUNTIME_SOURCE_DIGEST);
        Ok(())
    }

    #[test]
    fn materialization_writes_toolchain_manifest_and_provenance() -> Result<(), String> {
        let temp = temp_cache()?;
        let toolchain = "leanprover/lean4:v4.34.0-rc2".to_owned();
        let package = materialize_source_package(SemanticSearchSourcePackageRequest {
            cache_root: temp.path().to_path_buf(),
            toolchain_label: toolchain.clone(),
        })
        .map_err(|error| error.to_string())?;

        assert_eq!(
            read_string(&package.project_root.join("lean-toolchain"))?.trim(),
            toolchain
        );
        assert!(package.project_root.join("lakefile.lean").is_file());
        assert!(
            package
                .project_root
                .join("LeanSemanticSearch/Capability.lean")
                .is_file()
        );
        assert!(!package.project_root.join("Main.lean").exists());
        assert!(!package.project_root.join("LeanSemanticSearchTest.lean").exists());
        assert!(!package.project_root.join("LeanSemanticSearchTest").exists());

        let manifest: serde_json::Value =
            serde_json::from_str(&read_string(&package.project_root.join("lake-manifest.json"))?)
                .map_err(|error| error.to_string())?;
        let packages = manifest
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| "manifest packages must be an array".to_owned())?;
        assert!(packages.is_empty(), "runtime package must remain zero-dependency");

        let sidecar: SemanticSearchRuntimeProvenance =
            serde_json::from_str(&read_string(&package.project_root.join(SIDECAR_FILE_NAME))?)
                .map_err(|error| error.to_string())?;
        assert_eq!(sidecar.source_revision, super::SOURCE_REVISION);
        assert_eq!(sidecar.runtime_source_digest, RUNTIME_SOURCE_DIGEST);
        assert_eq!(sidecar.package, PACKAGE_NAME);
        assert_eq!(sidecar.materialized_package, MATERIALIZED_PACKAGE_NAME);
        assert_eq!(sidecar.library, LIBRARY_NAME);
        assert_eq!(sidecar.toolchain_label, toolchain);
        assert_eq!(sidecar, package.provenance);
        Ok(())
    }

    #[test]
    fn runtime_dependency_descriptor_is_owned_by_runtime() -> Result<(), String> {
        let dylib = PathBuf::from("/tmp/libLeanSemanticSearch.dylib");
        let runtime = super::SemanticSearchRuntime {
            built: lean_toolchain::LeanBuiltCapability::path(&dylib)
                .package(MATERIALIZED_PACKAGE_NAME)
                .module(LIBRARY_NAME),
            provenance: SemanticSearchRuntimeProvenance::new("leanprover/lean4:v4.34.0-rc2"),
        };
        let dependency = runtime.dependency().map_err(|error| error.to_string())?;

        assert_eq!(dependency.path_ref(), dylib.as_path());
        assert!(dependency.exports_symbols_for_dependents());
        let initializer = dependency
            .module_initializer()
            .ok_or_else(|| "dependency should carry module initializer".to_owned())?;
        assert_eq!(initializer.package_name(), MATERIALIZED_PACKAGE_NAME);
        assert_eq!(initializer.module_name(), LIBRARY_NAME);
        Ok(())
    }

    #[test]
    fn warm_materialization_reuses_existing_entry() -> Result<(), String> {
        let temp = temp_cache()?;
        let request = SemanticSearchSourcePackageRequest {
            cache_root: temp.path().to_path_buf(),
            toolchain_label: "leanprover/lean4:v4.34.0-rc2".to_owned(),
        };
        let first = materialize_source_package(request.clone()).map_err(|error| error.to_string())?;
        let marker = first.project_root.join("warm-marker");
        fs::write(&marker, b"warm").map_err(|error| error.to_string())?;
        let second = materialize_source_package(request).map_err(|error| error.to_string())?;
        assert_eq!(first.project_root, second.project_root);
        assert!(marker.is_file(), "warm cache entry should not be recopied");
        Ok(())
    }

    #[test]
    fn semantic_provenance_sidecar_mismatch_rematerializes_entry() -> Result<(), String> {
        let temp = temp_cache()?;
        let request = SemanticSearchSourcePackageRequest {
            cache_root: temp.path().to_path_buf(),
            toolchain_label: "leanprover/lean4:v4.34.0-rc2".to_owned(),
        };
        let first = materialize_source_package(request.clone()).map_err(|error| error.to_string())?;
        let marker = first.project_root.join("warm-marker");
        fs::write(&marker, b"warm").map_err(|error| error.to_string())?;
        fs::write(first.project_root.join(SIDECAR_FILE_NAME), b"{}").map_err(|error| error.to_string())?;

        let second = materialize_source_package(request).map_err(|error| error.to_string())?;
        assert_eq!(first.project_root, second.project_root);
        assert!(
            !marker.exists(),
            "semantic provenance mismatch should force rematerialization"
        );
        let sidecar: SemanticSearchRuntimeProvenance =
            serde_json::from_str(&read_string(&second.project_root.join(SIDECAR_FILE_NAME))?)
                .map_err(|error| error.to_string())?;
        assert_eq!(sidecar, second.provenance);
        Ok(())
    }

    #[test]
    fn concurrent_materialization_serializes_same_entry() -> Result<(), String> {
        let temp = temp_cache()?;
        let cache_root = temp.path().to_path_buf();
        let toolchain = "leanprover/lean4:v4.34.0-rc2".to_owned();
        let handles = (0..8)
            .map(|_| {
                let cache_root = cache_root.clone();
                let toolchain = toolchain.clone();
                thread::spawn(move || {
                    materialize_source_package(SemanticSearchSourcePackageRequest {
                        cache_root,
                        toolchain_label: toolchain,
                    })
                    .map_err(|error| error.to_string())
                    .map(|package| package.project_root)
                })
            })
            .collect::<Vec<_>>();

        let mut roots = Vec::<PathBuf>::new();
        for handle in handles {
            let root = handle
                .join()
                .map_err(|_| "materialization thread panicked".to_owned())??;
            roots.push(root);
        }
        let first = roots
            .first()
            .ok_or_else(|| "expected at least one materialization result".to_owned())?;
        assert!(roots.iter().all(|root| root == first));
        assert!(first.join(SIDECAR_FILE_NAME).is_file());
        assert!(first.join("LeanSemanticSearch/Capability.lean").is_file());
        Ok(())
    }
}
