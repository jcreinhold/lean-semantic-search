//! Package-owned runtime for the `LeanSemanticSearch` Lean package.
//!
//! Downstream hosts depend on this crate instead of copying Lean source into
//! their own repositories. The crate owns the runtime payload, its source
//! digest, cache materialization, the provenance sidecar, and the Lake build
//! of the package's modules under the SDK shared writer. Lake decides what is
//! stale. Callers run Lean against the built modules; this crate starts no
//! Lean process for them and chooses no imports.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, ExitStatus, Output};
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use lean_toolchain::with_materialized_source_package_with_writer;
use lean_toolchain::{
    GeneratedSourceFile, SourcePackageError, SourcePackageManifestPolicy, SourcePackageMaterializationRequest,
    materialize_source_package as materialize_with_toolchain,
};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

const RUNTIME_SOURCE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/runtime");
const SIDECAR_FILE_NAME: &str = "semantic-search-runtime.json";
const CACHE_SCHEMA_VERSION: u32 = 1;
const PACKAGE_NAME: &str = "lean-semantic-search";
const MATERIALIZED_PACKAGE_NAME: &str = "lean_semantic_search";
/// Limit on one `lake build` run, the same as the SDK's own Lake builds.
#[cfg(unix)]
const LAKE_BUILD_TIMEOUT: Duration = Duration::from_secs(120);
/// Characters of Lake output kept in a build error.
#[cfg(unix)]
const LAKE_DETAIL_CHARS: usize = 2000;

/// Runtime source revision packaged by this crate.
pub const SOURCE_REVISION: &str = "c6cfa0dc75934d3487f8e018ad154cb104e23642";
/// Digest of the runtime Lean payload, using the file-set rule in
/// `lean/VENDORING.md`.
pub const RUNTIME_SOURCE_DIGEST: &str = "325560f96c429bc6edd2ec5c34f3358b8e8b9694f7c59afc4ce9815e0e17ed1b";
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
    /// Lean sysroot whose `bin/lake` runs the build, with `LEAN_SYSROOT` set
    /// to it.
    pub lean_sysroot: PathBuf,
}

/// Request to materialize the packaged runtime as a Lake source package.
///
/// This exists for downstream Lake projects, such as `lean-dup`, that
/// `require` `LeanSemanticSearch` and build it themselves. It does not build
/// the package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSearchSourcePackageRequest {
    /// Caller-owned cache root.
    pub cache_root: PathBuf,
    /// Lean toolchain label to write into the generated `lean-toolchain`.
    pub toolchain_label: String,
}

/// Built `LeanSemanticSearch` modules plus package provenance.
///
/// The paths come from a Lake build that finished under writer custody. Put
/// [`module_dir`](Self::module_dir) on `LEAN_PATH` to import the modules.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSearchRuntime {
    package_root: PathBuf,
    module_dir: PathBuf,
    provenance: SemanticSearchRuntimeProvenance,
}

impl SemanticSearchRuntime {
    /// Canonical root of the built Lake package. Treat it as an opaque
    /// directory owned by this crate; a host that guards reads by directory
    /// guards this one.
    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    /// Canonical directory of the built `.olean` files, inside
    /// [`package_root`](Self::package_root).
    pub fn module_dir(&self) -> &Path {
        &self.module_dir
    }

    /// Provenance recorded beside the package.
    pub fn provenance(&self) -> &SemanticSearchRuntimeProvenance {
        &self.provenance
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
    /// Lake package name in the materialized package: a plain Lean identifier
    /// that downstream Lake projects `require`.
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

/// Failure to materialize or build the runtime package.
///
/// The message names the step that failed. When the SDK shared writer reports
/// how far a command got, the source chain carries the SDK's
/// [`WriterFailure`](lean_toolchain::shared_writer::WriterFailure), directly
/// or inside an [`std::io::Error`] (read it with `WriterFailure::from_io`).
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(#[from] ErrorKind);

impl From<SourcePackageError> for Error {
    fn from(source: SourcePackageError) -> Self {
        Self(source.into())
    }
}

#[derive(Debug, thiserror::Error)]
enum ErrorKind {
    #[error("{action} {}: {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{action} {}: {source}", path.display())]
    Json {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid semantic-search runtime payload: {0}")]
    InvalidRuntimePayload(String),
    #[error("semantic-search runtime source materialization failed: {source}")]
    SourcePackage {
        #[from]
        source: SourcePackageError,
    },
    #[cfg(unix)]
    #[error("semantic-search runtime writer failed in {}: {source}", package_root.display())]
    Writer {
        package_root: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(unix)]
    #[error("`lake build {LIBRARY_NAME}` failed in {} with {status}: {detail}", package_root.display())]
    Lake {
        package_root: PathBuf,
        status: ExitStatus,
        detail: String,
    },
}

/// Build the packaged runtime's Lean modules for one toolchain under a
/// caller-selected SDK shared-writer helper.
///
/// The helper takes custody of the cache entry and project locks before
/// materialization and keeps it while `lake build LeanSemanticSearch` runs.
/// The caller must select an executable that speaks the SDK shared-writer
/// protocol. The result grants no permission to load or query the modules.
///
/// `lock_timeout` bounds admission, not the whole request; the Lake run has
/// its own two-minute limit. Cancellation stops further stages and asks the
/// SDK to stop owned work. An unavailable result does not permit a retry, and
/// unknown cleanup stays unknown: see [`Error`] for the SDK evidence it keeps.
///
/// # Errors
///
/// Returns [`Error`] on admission, cancellation, materialization or Lake
/// failure.
#[cfg(unix)]
pub fn build_cached_with_writer(
    input: SemanticSearchRuntimeBuild,
    writer_executable: &Path,
    lock_timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> Result<SemanticSearchRuntime, Error> {
    ensure_runtime_payload_source(Path::new(RUNTIME_SOURCE_ROOT))?;
    let provenance = SemanticSearchRuntimeProvenance::new(&input.toolchain_label);
    let request = source_package_request(input.cache_root, &input.toolchain_label, &provenance)?;
    with_materialized_source_package_with_writer(
        &request,
        writer_executable,
        lock_timeout,
        &cancelled,
        |materialized, writer| {
            let package_root = canonicalize(&materialized.project_root, "resolve semantic-search runtime package")?;
            let mut lake = Command::new(input.lean_sysroot.join("bin/lake"));
            lake.env("LEAN_SYSROOT", &input.lean_sysroot)
                .args(["build", LIBRARY_NAME])
                .current_dir(&package_root);
            let output = writer
                .run_cancellable(&lake, LAKE_BUILD_TIMEOUT, &cancelled)
                .map_err(|source| ErrorKind::Writer {
                    package_root: package_root.clone(),
                    source,
                })?;
            if !output.status.success() {
                return Err(ErrorKind::Lake {
                    package_root,
                    status: output.status,
                    detail: lake_detail(&output),
                }
                .into());
            }
            let module_dir = canonicalize(
                &package_root.join(".lake/build/lib/lean"),
                "find built semantic-search modules",
            )?;
            Ok(SemanticSearchRuntime {
                package_root,
                module_dir,
                provenance,
            })
        },
    )
}

/// Lake's output on one line, cut to its last [`LAKE_DETAIL_CHARS`]
/// characters, where Lake prints its error summary.
#[cfg(unix)]
fn lake_detail(output: &Output) -> String {
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let skip = line.chars().count().saturating_sub(LAKE_DETAIL_CHARS);
    line.chars().skip(skip).collect()
}

/// Materialize the packaged runtime source package without building it.
///
/// It starts no writer or compiler.
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
    let bytes = serde_json::to_vec_pretty(provenance).map_err(|source| ErrorKind::Json {
        action: "encode semantic-search runtime provenance sidecar",
        path: PathBuf::from(SIDECAR_FILE_NAME),
        source,
    })?;
    Ok(bytes)
}

fn ensure_zero_package_manifest(path: &Path) -> Result<(), Error> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&read_file(path, "read semantic-search runtime lake-manifest.json")?).map_err(
            |source| ErrorKind::Json {
                action: "decode semantic-search runtime lake-manifest.json",
                path: path.to_path_buf(),
                source,
            },
        )?;
    let packages = manifest
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            ErrorKind::InvalidRuntimePayload("lake-manifest.json must contain an array `packages`".to_owned())
        })?;
    if !packages.is_empty() {
        return Err(ErrorKind::InvalidRuntimePayload(
            "lake-manifest.json must remain zero-dependency (`packages: []`)".to_owned(),
        )
        .into());
    }
    Ok(())
}

fn materialized_lakefile_bytes() -> Result<Vec<u8>, Error> {
    let lakefile = Path::new(RUNTIME_SOURCE_ROOT).join("lakefile.lean");
    let lakefile_text = read_to_string(&lakefile, "read materialized semantic-search lakefile")?;
    Ok(lakefile_text
        .replace(
            "package «lean-semantic-search» where",
            "package lean_semantic_search where",
        )
        .into_bytes())
}

fn materialized_manifest_bytes() -> Result<Vec<u8>, Error> {
    let manifest_path = Path::new(RUNTIME_SOURCE_ROOT).join("lake-manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&read_file(
        &manifest_path,
        "read materialized semantic-search manifest",
    )?)
    .map_err(|source| ErrorKind::Json {
        action: "decode materialized semantic-search manifest",
        path: manifest_path.clone(),
        source,
    })?;
    let object = manifest
        .as_object_mut()
        .ok_or_else(|| ErrorKind::InvalidRuntimePayload("lake-manifest.json root must be a JSON object".to_owned()))?;
    object.insert(
        "name".to_owned(),
        serde_json::Value::String(MATERIALIZED_PACKAGE_NAME.to_owned()),
    );
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|source| ErrorKind::Json {
        action: "encode materialized semantic-search manifest",
        path: manifest_path.clone(),
        source,
    })?;
    ensure_zero_package_manifest(&manifest_path)?;
    Ok(bytes)
}

fn ensure_runtime_payload_source(source_root: &Path) -> Result<(), Error> {
    for entry in WalkDir::new(source_root) {
        let entry =
            entry.map_err(|source| ErrorKind::InvalidRuntimePayload(format!("walk runtime payload: {source}")))?;
        let source_path = entry.path();
        let relative = source_path.strip_prefix(source_root).map_err(|source| {
            ErrorKind::InvalidRuntimePayload(format!(
                "compute runtime payload relative path for {}: {source}",
                source_path.display()
            ))
        })?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        if excluded_runtime_path(relative) {
            return Err(ErrorKind::InvalidRuntimePayload(format!(
                "packaged runtime contains excluded path {}",
                relative.display()
            ))
            .into());
        }
    }
    ensure_zero_package_manifest(&source_root.join("lake-manifest.json"))?;
    Ok(())
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

fn read_file(path: &Path, action: &'static str) -> Result<Vec<u8>, Error> {
    Ok(fs::read(path).map_err(|source| ErrorKind::Io {
        action,
        path: path.to_path_buf(),
        source,
    })?)
}

fn read_to_string(path: &Path, action: &'static str) -> Result<String, Error> {
    Ok(fs::read_to_string(path).map_err(|source| ErrorKind::Io {
        action,
        path: path.to_path_buf(),
        source,
    })?)
}

#[cfg(unix)]
fn canonicalize(path: &Path, action: &'static str) -> Result<PathBuf, Error> {
    Ok(fs::canonicalize(path).map_err(|source| ErrorKind::Io {
        action,
        path: path.to_path_buf(),
        source,
    })?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::thread;

    use lean_toolchain::shared_writer::{WriterDisposition, WriterFailure};

    use sha2::{Digest, Sha256};
    use walkdir::WalkDir;

    use super::{
        Error, ErrorKind, LIBRARY_NAME, MATERIALIZED_PACKAGE_NAME, PACKAGE_NAME, RUNTIME_SOURCE_DIGEST,
        RUNTIME_SOURCE_ROOT, SIDECAR_FILE_NAME, SemanticSearchRuntimeProvenance, SemanticSearchSourcePackageRequest,
        excluded_runtime_path, materialize_source_package,
    };

    fn temp_cache() -> Result<tempfile::TempDir, String> {
        tempfile::tempdir().map_err(|error| error.to_string())
    }

    fn read_string(path: &Path) -> Result<String, String> {
        fs::read_to_string(path).map_err(|error| error.to_string())
    }

    /// Digest of the payload by the `lean/VENDORING.md` rule: SHA-256 over the
    /// path-sorted `shasum -a 256` lines of each digested file, named by its
    /// repository path.
    fn runtime_source_digest(source_root: &Path) -> Result<String, String> {
        let mut lines = Vec::new();
        for entry in WalkDir::new(source_root) {
            let entry = entry.map_err(|error| error.to_string())?;
            let relative = entry
                .path()
                .strip_prefix(source_root)
                .map_err(|error| error.to_string())?;
            let name = relative.to_string_lossy();
            if !entry.file_type().is_file()
                || excluded_runtime_path(relative)
                || name == "README.md"
                || name == "VENDORING.md"
            {
                continue;
            }
            let path = if name == "LICENSE-APACHE" || name == "LICENSE-MIT" {
                name.into_owned()
            } else {
                format!("lean/{name}")
            };
            let bytes = fs::read(entry.path()).map_err(|error| error.to_string())?;
            lines.push((path, hex_lower(&Sha256::digest(bytes))));
        }
        lines.sort();
        let mut outer = Sha256::new();
        for (path, digest) in lines {
            outer.update(format!("{digest}  {path}\n"));
        }
        Ok(hex_lower(&outer.finalize()))
    }

    fn hex_lower(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len().saturating_mul(2));
        for byte in bytes {
            for nibble in [byte >> 4, byte & 0x0f] {
                out.push(DIGITS.get(usize::from(nibble)).map_or('?', |digit| char::from(*digit)));
            }
        }
        out
    }

    #[test]
    fn runtime_source_digest_matches_recorded_value() -> Result<(), String> {
        let digest = runtime_source_digest(Path::new(RUNTIME_SOURCE_ROOT))?;
        assert_eq!(digest, RUNTIME_SOURCE_DIGEST);
        Ok(())
    }

    #[test]
    fn materialization_writes_toolchain_manifest_and_provenance() -> Result<(), String> {
        let temp = temp_cache()?;
        let toolchain = "leanprover/lean4:v4.36.0".to_owned();
        let package = materialize_source_package(SemanticSearchSourcePackageRequest {
            cache_root: temp.path().to_path_buf(),
            toolchain_label: toolchain.clone(),
        })
        .map_err(|error| error.to_string())?;

        assert_eq!(
            read_string(&package.project_root.join("lean-toolchain"))?.trim(),
            toolchain
        );
        assert!(
            read_string(&package.project_root.join("lakefile.lean"))?.contains("package lean_semantic_search where")
        );
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
    fn warm_materialization_reuses_existing_entry() -> Result<(), String> {
        let temp = temp_cache()?;
        let request = SemanticSearchSourcePackageRequest {
            cache_root: temp.path().to_path_buf(),
            toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
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
            toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
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
                toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
                lean_sysroot: temp.path().join("unused-sysroot"),
            },
            &temp.path().join("must-not-launch"),
            std::time::Duration::from_secs(1),
            || true,
        );
        assert!(matches!(
            result,
            Err(Error(ErrorKind::SourcePackage {
                source: lean_toolchain::SourcePackageError::Io { source, .. },
            })) if source.kind() == std::io::ErrorKind::Interrupted
        ));
        let entries = fs::read_dir(temp.path().join(RUNTIME_SOURCE_DIGEST)).map_err(|error| error.to_string())?;
        for entry in entries {
            assert!(!entry.map_err(|error| error.to_string())?.path().is_dir());
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn supervised_build_failed_admission_preserves_stale_source() -> Result<(), String> {
        let temp = temp_cache()?;
        let label = "leanprover/lean4:v4.36.0".to_owned();
        let provenance = SemanticSearchRuntimeProvenance::new(&label);
        let request = super::source_package_request(temp.path().to_path_buf(), &label, &provenance)
            .map_err(|error| error.to_string())?;
        let package = lean_toolchain::materialize_source_package(&request).map_err(|error| error.to_string())?;
        let sentinel = package.project_root.join(SIDECAR_FILE_NAME);
        fs::write(&sentinel, "active source must survive failed custody").map_err(|error| error.to_string())?;
        let result = super::build_cached_with_writer(
            super::SemanticSearchRuntimeBuild {
                cache_root: temp.path().to_path_buf(),
                toolchain_label: label,
                lean_sysroot: temp.path().join("unused-sysroot"),
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
    #[cfg(unix)]
    fn lake_detail_keeps_the_end_of_the_output_on_one_line() {
        use std::os::unix::process::ExitStatusExt as _;
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(256),
            stdout: format!("{}\nerror: build failed\n", "progress ".repeat(1000)).into_bytes(),
            stderr: b"stderr detail\n".to_vec(),
        };
        let detail = super::lake_detail(&output);
        assert!(!detail.contains('\n'));
        assert_eq!(detail.chars().count(), super::LAKE_DETAIL_CHARS);
        assert!(detail.ends_with("error: build failed stderr detail"));
    }

    fn writer_failure<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a WriterFailure> {
        let mut current = Some(error);
        while let Some(source) = current {
            if let Some(failure) = source.downcast_ref::<WriterFailure>() {
                return Some(failure);
            }
            if let Some(failure) = source.downcast_ref::<std::io::Error>().and_then(WriterFailure::from_io) {
                return Some(failure);
            }
            current = source.source();
        }
        None
    }

    #[test]
    fn sdk_writer_failures_retain_disposition_and_stop_receipt() -> Result<(), String> {
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
            let mut errors = vec![Error::from(lean_toolchain::SourcePackageError::Io {
                action: "test source failure",
                path: PathBuf::from("source"),
                source: std::io::Error::other(expected.clone()),
            })];
            #[cfg(unix)]
            errors.push(Error::from(ErrorKind::Writer {
                package_root: PathBuf::from("source"),
                source: std::io::Error::other(expected.clone()),
            }));
            for error in errors {
                let actual =
                    writer_failure(&error).ok_or_else(|| "SDK execution evidence lost in source chain".to_owned())?;
                assert_eq!(actual, &expected);
            }
        }
        Ok(())
    }

    #[test]
    fn concurrent_materialization_serializes_same_entry() -> Result<(), String> {
        let temp = temp_cache()?;
        let cache_root = temp.path().to_path_buf();
        let toolchain = "leanprover/lean4:v4.36.0".to_owned();
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
