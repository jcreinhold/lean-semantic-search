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

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lean_rs_worker_protocol::worker_exports::{
    doctor_signature, json_command_signature, metadata_signature, streaming_command_signature,
};
#[cfg(unix)]
use lean_toolchain::with_materialized_source_package_with_writer;
use lean_toolchain::{
    CargoLeanCapability, GeneratedSourceFile, LeanBuiltCapability, LeanBuiltCapabilityError, LeanExportSignature,
    LeanLibraryDependency, LinkDiagnostics, SourcePackageError, SourcePackageManifestPolicy,
    SourcePackageMaterializationRequest, materialize_source_package as materialize_with_toolchain,
    with_materialized_source_package,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

const RUNTIME_SOURCE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/runtime");
const SIDECAR_FILE_NAME: &str = "semantic-search-runtime.json";
const CACHE_SCHEMA_VERSION: u32 = 1;

/// Runtime source revision packaged by this crate.
pub const SOURCE_REVISION: &str = "c6cfa0dc75934d3487f8e018ad154cb104e23642";
/// Digest of the runtime Lean payload, using the file-set rule in
/// `lean/VENDORING.md`.
pub const RUNTIME_SOURCE_DIGEST: &str = "325560f96c429bc6edd2ec5c34f3358b8e8b9694f7c59afc4ce9815e0e17ed1b";
/// Lake package name owned by the runtime payload.
pub const PACKAGE_NAME: &str = "lean-semantic-search";
const MATERIALIZED_PACKAGE_NAME: &str = "lean_semantic_search";
/// Lean library and root module name owned by the runtime payload.
pub const LIBRARY_NAME: &str = "LeanSemanticSearch";

/// Request to build the packaged runtime for one Lean toolchain.
///
/// `cache_root` is caller-owned. This crate owns the layout below it, keyed by
/// source digest and requested toolchain label, with a separate linked cache
/// for each selected macOS sysroot.
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
/// The SDK source-entry transaction covers materialization, Lake work and
/// capability-manifest publication. This convenience API uses the SDK's writer
/// selection and a shared 120-second lock-admission budget. The lexical entry
/// lock alone does not retain custody after controller death. Hosts requiring
/// that custody use `build_cached_with_writer` on Unix.
///
/// # Errors
///
/// Returns [`Error`] when materialization, cache validation, or the explicit
/// sysroot Lake build fails.
pub fn build_cached(input: SemanticSearchRuntimeBuild) -> Result<SemanticSearchRuntime, Error> {
    let started = Instant::now();
    let lock_timeout = Duration::from_secs(120);
    let linked = LinkedBuild::new(input.lean_sysroot, cfg!(target_os = "macos"))?;
    with_source_package(
        SemanticSearchSourcePackageRequest {
            cache_root: input.cache_root,
            toolchain_label: input.toolchain_label.clone(),
        },
        Some(&linked),
        lock_timeout.saturating_sub(started.elapsed()),
        || false,
        |package| {
            let built = capability_builder(&package.project_root, &linked.sysroot)
                .build_quiet_with_lock_wait(lock_timeout.saturating_sub(started.elapsed()), || false)
                .map_err(|source| Error::Build {
                    toolchain_label: input.toolchain_label,
                    source,
                })?;
            Ok(SemanticSearchRuntime {
                built: (&built).into(),
                provenance: package.provenance,
            })
        },
    )
}

/// Build with a caller-selected SDK shared-writer helper.
///
/// The helper acknowledges custody of the source-entry and project locks
/// before materialization and retains it through Lake work and publication.
/// The callback uses that same writer, avoiding a nested helper. The caller
/// must select an executable implementing the SDK shared-writer protocol;
/// this function does not grant permission to load or query the result.
///
/// `lock_timeout` bounds admission, not the whole request. Cancellation stops
/// further stages and requests SDK-owned command cleanup. An unavailable result
/// does not authorize replay. Typed SDK errors retain execution disposition and
/// any stop receipt; unknown cleanup remains unknown.
///
/// # Errors
///
/// Returns [`Error`] on admission, cancellation, materialization or build
/// failure. The original SDK failure remains available through its source chain.
#[cfg(unix)]
pub fn build_cached_with_writer(
    input: SemanticSearchRuntimeBuild,
    writer_executable: &Path,
    lock_timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> Result<SemanticSearchRuntime, Error> {
    let started = Instant::now();
    ensure_runtime_payload_source(Path::new(RUNTIME_SOURCE_ROOT))?;
    let provenance = SemanticSearchRuntimeProvenance::new(&input.toolchain_label);
    let linked = LinkedBuild::new(input.lean_sysroot, cfg!(target_os = "macos"))?;
    let request = source_package_request(input.cache_root, &input.toolchain_label, &provenance, Some(&linked))?;
    with_materialized_source_package_with_writer(
        &request,
        writer_executable,
        lock_timeout.saturating_sub(started.elapsed()),
        &cancelled,
        |materialized, writer| {
            let built = capability_builder(&materialized.project_root, &linked.sysroot)
                .build_quiet_with_writer(writer, &cancelled)
                .map_err(|source| Error::Build {
                    toolchain_label: input.toolchain_label,
                    source,
                })?;
            Ok(SemanticSearchRuntime {
                built: (&built).into(),
                provenance,
            })
        },
    )
}

fn capability_builder(project_root: &Path, sysroot: &Path) -> CargoLeanCapability {
    let mut builder = CargoLeanCapability::new(project_root, LIBRARY_NAME)
        .package(MATERIALIZED_PACKAGE_NAME)
        .module(LIBRARY_NAME)
        .lean_sysroot(sysroot);
    for signature in export_signatures() {
        builder = builder.export_signature(signature);
    }
    builder
}

fn with_source_package<T>(
    input: SemanticSearchSourcePackageRequest,
    linked: Option<&LinkedBuild>,
    lock_timeout: Duration,
    cancelled: impl Fn() -> bool,
    use_package: impl FnOnce(SemanticSearchSourcePackage) -> Result<T, Error>,
) -> Result<T, Error> {
    ensure_runtime_payload_source(Path::new(RUNTIME_SOURCE_ROOT))?;
    let provenance = SemanticSearchRuntimeProvenance::new(&input.toolchain_label);
    let request = source_package_request(input.cache_root, &input.toolchain_label, &provenance, linked)?;
    with_materialized_source_package(&request, lock_timeout, cancelled, |materialized| {
        use_package(SemanticSearchSourcePackage {
            project_root: materialized.project_root,
            provenance,
        })
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
    materialize_source_package_for_build(input, None)
}

fn materialize_source_package_for_build(
    input: SemanticSearchSourcePackageRequest,
    linked: Option<&LinkedBuild>,
) -> Result<SemanticSearchSourcePackage, Error> {
    ensure_runtime_payload_source(Path::new(RUNTIME_SOURCE_ROOT))?;
    let provenance = SemanticSearchRuntimeProvenance::new(&input.toolchain_label);
    let request = source_package_request(input.cache_root, &input.toolchain_label, &provenance, linked)?;
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

struct LinkedBuild {
    sysroot: PathBuf,
    runpath_literal: Option<String>,
}

impl LinkedBuild {
    fn new(sysroot: PathBuf, needs_runpath: bool) -> Result<Self, Error> {
        if !needs_runpath {
            return Ok(Self {
                sysroot,
                runpath_literal: None,
            });
        }
        let sysroot = fs::canonicalize(&sysroot).map_err(|source| Error::Io {
            action: "resolve selected Lean sysroot",
            path: sysroot,
            source,
        })?;
        let library_dir = sysroot.join("lib/lean");
        let action = "select Lean runtime library directory";
        let metadata = fs::metadata(&library_dir).map_err(|source| Error::Io {
            action,
            path: library_dir.clone(),
            source,
        })?;
        let Some(path) = library_dir
            .to_str()
            .filter(|path| metadata.is_dir() && !path.chars().any(char::is_control))
        else {
            return Err(Error::Io {
                action,
                path: library_dir,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "expected a directory path representable as a Lean string literal",
                ),
            });
        };
        // With control characters rejected, JSON and Lean string literals use
        // the same spelling for quotes, backslashes and the remaining Unicode.
        let runpath_literal = Some(serde_json::Value::String(path.to_owned()).to_string());
        Ok(Self {
            sysroot,
            runpath_literal,
        })
    }

    fn cache_root(&self, root: &Path) -> PathBuf {
        if self.runpath_literal.is_none() {
            return root.to_path_buf();
        }
        let digest = hex_lower(&Sha256::digest(self.sysroot.as_os_str().as_encoded_bytes()));
        root.join(format!("darwin-runpath-v1-{digest}"))
    }
}

fn source_package_request(
    cache_root: PathBuf,
    toolchain_label: &str,
    provenance: &SemanticSearchRuntimeProvenance,
    linked: Option<&LinkedBuild>,
) -> Result<SourcePackageMaterializationRequest, Error> {
    let cache_root = if let Some(build) = linked {
        build.cache_root(&cache_root)
    } else {
        cache_root
    };
    let runpath_literal = linked.and_then(|build| build.runpath_literal.as_deref());
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
                contents: materialized_lakefile_bytes(runpath_literal)?,
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

fn materialized_lakefile_bytes(runpath_literal: Option<&str>) -> Result<Vec<u8>, Error> {
    let lakefile = Path::new(RUNTIME_SOURCE_ROOT).join("lakefile.lean");
    let lakefile_text = read_to_string(&lakefile, "read materialized semantic-search lakefile")?;
    let rewritten = lakefile_text.replace(
        "package «lean-semantic-search» where",
        "package lean_semantic_search where",
    );
    let Some(runpath_literal) = runpath_literal else {
        return Ok(rewritten.into_bytes());
    };
    let anchor = "lean_lib LeanSemanticSearch where\n";
    if rewritten.matches(anchor).count() != 1 {
        return Err(Error::InvalidRuntimePayload(
            "expected exactly one LeanSemanticSearch library definition in the packaged lakefile".to_owned(),
        ));
    }
    let config = format!("{anchor}  moreLinkArgs := #[\"-Xlinker\", \"-rpath\", \"-Xlinker\", {runpath_literal}]\n");
    Ok(rewritten.replacen(anchor, &config, 1).into_bytes())
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

    fn fake_sysroot(path: PathBuf) -> Result<PathBuf, String> {
        fs::create_dir_all(path.join("lib/lean")).map_err(|error| error.to_string())?;
        Ok(path)
    }

    #[test]
    #[cfg(unix)]
    fn linked_cache_uses_canonical_sysroot_and_preserves_source_provenance() -> Result<(), String> {
        let temp = temp_cache()?;
        let first = fake_sysroot(temp.path().join(r#"Lean with spaces, "quotes" and \slash"#))?;
        let second = fake_sysroot(temp.path().join("other Lean"))?;
        let cache = temp.path().join("cache");
        let label = "leanprover/lean4:v4.36.0";
        let provenance = SemanticSearchRuntimeProvenance::new(label);
        let first_linked = super::LinkedBuild::new(first.clone(), true).map_err(|error| error.to_string())?;
        let alias = first.join("lib/..");
        let alias_linked = super::LinkedBuild::new(alias, true).map_err(|error| error.to_string())?;
        let second_linked = super::LinkedBuild::new(second, true).map_err(|error| error.to_string())?;
        assert_eq!(
            first_linked.sysroot,
            first.canonicalize().map_err(|error| error.to_string())?
        );
        assert_eq!(first_linked.cache_root(&cache), alias_linked.cache_root(&cache));
        assert_ne!(first_linked.cache_root(&cache), second_linked.cache_root(&cache));

        let request = super::source_package_request(cache, label, &provenance, Some(&first_linked))
            .map_err(|error| error.to_string())?;
        let materialized = lean_toolchain::materialize_source_package(&request).map_err(|error| error.to_string())?;
        let config = read_string(&materialized.project_root.join("lakefile.lean"))?;
        let literal = config
            .lines()
            .find_map(|line| line.strip_prefix("  moreLinkArgs := #[\"-Xlinker\", \"-rpath\", \"-Xlinker\", "))
            .and_then(|line| line.strip_suffix(']'))
            .ok_or_else(|| "missing generated link argument".to_owned())?;
        let decoded: String = serde_json::from_str(literal).map_err(|error| error.to_string())?;
        assert_eq!(Path::new(&decoded), first_linked.sysroot.join("lib/lean"));

        let sidecar: SemanticSearchRuntimeProvenance =
            serde_json::from_str(&read_string(&materialized.project_root.join(SIDECAR_FILE_NAME))?)
                .map_err(|error| error.to_string())?;
        assert_eq!(sidecar, provenance);
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn linked_sysroot_rejects_invalid_inputs_and_leaves_source_only_unchanged() -> Result<(), String> {
        let temp = temp_cache()?;
        let missing = temp.path().join("missing");
        assert!(matches!(
            super::LinkedBuild::new(missing.clone(), true),
            Err(super::Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound
        ));
        assert!(matches!(
            super::LinkedBuild::new(temp.path().to_path_buf(), true),
            Err(super::Error::Io { path, source, .. })
                if path.ends_with("lib/lean") && source.kind() == std::io::ErrorKind::NotFound
        ));
        let invalid = fake_sysroot(temp.path().join("path\nwith control"))?;
        assert!(matches!(
            super::LinkedBuild::new(invalid, true),
            Err(super::Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::InvalidInput
        ));

        let non_macos = super::LinkedBuild::new(missing.clone(), false).map_err(|error| error.to_string())?;
        assert_eq!(non_macos.sysroot, missing);
        let cache = temp.path().join("cache");
        let label = "leanprover/lean4:v4.36.0";
        let provenance = SemanticSearchRuntimeProvenance::new(label);
        let unlinked = super::source_package_request(cache.clone(), label, &provenance, None)
            .map_err(|error| error.to_string())?;
        let linked = super::source_package_request(cache.clone(), label, &provenance, Some(&non_macos))
            .map_err(|error| error.to_string())?;
        assert_eq!(unlinked.cache_root, cache);
        assert_eq!(linked.cache_root, cache);
        let source = lean_toolchain::materialize_source_package(&unlinked).map_err(|error| error.to_string())?;
        let other = lean_toolchain::materialize_source_package(&linked).map_err(|error| error.to_string())?;
        assert_eq!(source.project_root, other.project_root);
        assert!(!read_string(&source.project_root.join("lakefile.lean"))?.contains("moreLinkArgs"));
        Ok(())
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
        let toolchain = "leanprover/lean4:v4.35.0".to_owned();
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
            provenance: SemanticSearchRuntimeProvenance::new("leanprover/lean4:v4.35.0"),
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
            toolchain_label: "leanprover/lean4:v4.35.0".to_owned(),
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
            toolchain_label: "leanprover/lean4:v4.35.0".to_owned(),
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
    #[cfg(unix)]
    fn supervised_build_cancellation_never_materializes_or_launches() -> Result<(), String> {
        let temp = temp_cache()?;
        let result = super::build_cached_with_writer(
            super::SemanticSearchRuntimeBuild {
                cache_root: temp.path().to_path_buf(),
                toolchain_label: "leanprover/lean4:v4.35.0".to_owned(),
                lean_sysroot: fake_sysroot(temp.path().join("unused-sysroot"))?,
            },
            &temp.path().join("must-not-launch"),
            std::time::Duration::from_secs(1),
            || true,
        );
        assert!(matches!(
            result,
            Err(super::Error::SourcePackage {
                source: lean_toolchain::SourcePackageError::Io { source, .. },
            }) if source.kind() == std::io::ErrorKind::Interrupted
        ));
        let linked = super::LinkedBuild::new(temp.path().join("unused-sysroot"), cfg!(target_os = "macos"))
            .map_err(|error| error.to_string())?;
        let entries = fs::read_dir(linked.cache_root(temp.path()).join(RUNTIME_SOURCE_DIGEST))
            .map_err(|error| error.to_string())?;
        for entry in entries {
            assert!(!entry.map_err(|error| error.to_string())?.path().is_dir());
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn supervised_build_failed_admission_preserves_stale_source() -> Result<(), String> {
        let temp = temp_cache()?;
        let label = "leanprover/lean4:v4.35.0".to_owned();
        let sysroot = fake_sysroot(temp.path().join("unused-sysroot"))?;
        let linked =
            super::LinkedBuild::new(sysroot.clone(), cfg!(target_os = "macos")).map_err(|error| error.to_string())?;
        let provenance = SemanticSearchRuntimeProvenance::new(&label);
        let request = super::source_package_request(temp.path().to_path_buf(), &label, &provenance, Some(&linked))
            .map_err(|error| error.to_string())?;
        let package = lean_toolchain::materialize_source_package(&request).map_err(|error| error.to_string())?;
        let sentinel = package.project_root.join(SIDECAR_FILE_NAME);
        fs::write(&sentinel, "active source must survive failed custody").map_err(|error| error.to_string())?;
        let result = super::build_cached_with_writer(
            super::SemanticSearchRuntimeBuild {
                cache_root: temp.path().to_path_buf(),
                toolchain_label: label,
                lean_sysroot: sysroot,
            },
            &temp.path().join("missing-helper"),
            std::time::Duration::from_secs(1),
            || false,
        );
        assert!(result.is_err(), "an absent helper cannot authorize mutation");
        assert_eq!(read_string(&sentinel)?, "active source must survive failed custody");
        Ok(())
    }

    #[test]
    fn active_runtime_build_transaction_prevents_root_replacement() -> Result<(), String> {
        let temp = temp_cache()?;
        let input = SemanticSearchSourcePackageRequest {
            cache_root: temp.path().to_path_buf(),
            toolchain_label: "leanprover/lean4:v4.35.0".to_owned(),
        };
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let owner_input = input.clone();
        let owner = thread::spawn(move || {
            super::with_source_package(
                owner_input,
                None,
                std::time::Duration::from_secs(1),
                || false,
                |package| {
                    fs::write(package.project_root.join("active-build-marker"), b"building").map_err(|source| {
                        super::Error::Io {
                            action: "write regression marker",
                            path: package.project_root.clone(),
                            source,
                        }
                    })?;
                    fs::write(package.project_root.join(SIDECAR_FILE_NAME), b"{}").map_err(|source| {
                        super::Error::Io {
                            action: "invalidate regression provenance",
                            path: package.project_root.clone(),
                            source,
                        }
                    })?;
                    ready_tx
                        .send(package.project_root)
                        .map_err(|error| super::Error::InvalidRuntimePayload(error.to_string()))?;
                    release_rx
                        .recv_timeout(std::time::Duration::from_secs(3))
                        .map_err(|error| super::Error::InvalidRuntimePayload(error.to_string()))?;
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())
        });
        let active = ready_rx.recv_timeout(std::time::Duration::from_secs(3));
        let (timed_out, marker_kept) = if let Ok(active_root) = &active {
            let blocked = super::with_source_package(
                input.clone(),
                None,
                std::time::Duration::from_millis(30),
                || false,
                |_| Ok(()),
            );
            let timed_out = matches!(blocked,
                Err(super::Error::SourcePackage {
                    source: lean_toolchain::SourcePackageError::Io { source, .. },
                }) if source.kind() == std::io::ErrorKind::TimedOut
            );
            (timed_out, active_root.join("active-build-marker").is_file())
        } else {
            (false, false)
        };
        // Release and join before propagating observations or asserting them.
        let release = release_tx.send(());
        let owner_result = owner.join();
        let active_root = active.map_err(|error| error.to_string())?;
        release.map_err(|error| error.to_string())?;
        owner_result.map_err(|_| "runtime transaction thread panicked".to_owned())??;
        assert!(
            timed_out,
            "rematerialization must wait on the external transaction lock"
        );
        assert!(marker_kept, "active build root must not be replaced while locked");
        let refreshed = materialize_source_package(input).map_err(|error| error.to_string())?;
        assert_eq!(refreshed.project_root, active_root);
        assert!(!refreshed.project_root.join("active-build-marker").exists());
        Ok(())
    }

    fn writer_failure<'a>(
        error: &'a (dyn std::error::Error + 'static),
    ) -> Option<&'a lean_toolchain::shared_writer::WriterFailure> {
        let mut current = Some(error);
        while let Some(source) = current {
            if let Some(failure) = source.downcast_ref::<lean_toolchain::shared_writer::WriterFailure>() {
                return Some(failure);
            }
            if let Some(failure) = source
                .downcast_ref::<std::io::Error>()
                .and_then(lean_toolchain::shared_writer::WriterFailure::from_io)
            {
                return Some(failure);
            }
            current = source.source();
        }
        None
    }

    #[test]
    fn sdk_writer_failures_retain_disposition_and_stop_receipt() -> Result<(), String> {
        use lean_toolchain::shared_writer::{WriterDisposition, WriterFailure};
        for disposition in [
            WriterDisposition::NotStarted,
            WriterDisposition::Stopped,
            WriterDisposition::Unknown,
        ] {
            let expected = WriterFailure {
                disposition,
                detail: "original SDK failure".to_owned(),
                stop_receipt: (disposition == WriterDisposition::Stopped)
                    .then(|| serde_json::json!({"opaque_test_stop_receipt": {"retained": true}})),
            };
            let source_error = super::Error::from(lean_toolchain::SourcePackageError::Io {
                action: "test source failure",
                path: PathBuf::from("source"),
                source: std::io::Error::other(expected.clone()),
            });
            let build_error = super::Error::Build {
                toolchain_label: "test".to_owned(),
                source: lean_toolchain::LinkDiagnostics::LakeWriterFailure {
                    project_root: PathBuf::from("source"),
                    target_name: LIBRARY_NAME.to_owned(),
                    failure: Box::new(expected.clone()),
                },
            };
            for error in [source_error, build_error] {
                let actual =
                    writer_failure(&error).ok_or_else(|| "SDK execution evidence lost in source chain".to_owned())?;
                assert_eq!(actual, &expected);
            }
        }
        Ok(())
    }

    #[test]
    fn source_transaction_preserves_original_callback_failure() -> Result<(), String> {
        use lean_toolchain::shared_writer::{WriterDisposition, WriterFailure};
        let temp = temp_cache()?;
        let expected = WriterFailure {
            disposition: WriterDisposition::Unknown,
            detail: "original build result unavailable".to_owned(),
            stop_receipt: None,
        };
        let result: Result<(), super::Error> = super::with_source_package(
            SemanticSearchSourcePackageRequest {
                cache_root: temp.path().to_path_buf(),
                toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
            },
            None,
            std::time::Duration::from_secs(1),
            || false,
            |_| {
                Err(super::Error::Build {
                    toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
                    source: lean_toolchain::LinkDiagnostics::LakeWriterFailure {
                        project_root: temp.path().to_path_buf(),
                        target_name: LIBRARY_NAME.to_owned(),
                        failure: Box::new(expected.clone()),
                    },
                })
            },
        );
        let error = result
            .err()
            .ok_or_else(|| "callback failure became success".to_owned())?;
        let actual = writer_failure(&error).ok_or_else(|| "callback execution evidence lost".to_owned())?;
        assert_eq!(actual, &expected);
        Ok(())
    }

    #[test]
    fn concurrent_materialization_serializes_same_entry() -> Result<(), String> {
        let temp = temp_cache()?;
        let cache_root = temp.path().to_path_buf();
        let toolchain = "leanprover/lean4:v4.35.0".to_owned();
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
