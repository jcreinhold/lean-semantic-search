//! Package-owned runtime for the `LeanSemanticSearch` Lean package.
//!
//! Downstream hosts depend on this crate instead of copying Lean source into
//! their own repositories. The crate owns the runtime payload, its source
//! digest, cache materialization, the provenance sidecar, and the Lake build
//! of the package's modules. Lake decides what is stale. Callers run Lean
//! against the built modules; this crate starts no Lean process for them and
//! chooses no imports.

use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, ExitStatus, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use lean_toolchain::process_group::OwnedCommandGroup;
#[cfg(unix)]
use lean_toolchain::with_materialized_source_package;
use lean_toolchain::{
    GeneratedSourceFile, SourcePackageError, SourcePackageManifestPolicy, SourcePackageMaterializationRequest,
    materialize_source_package as materialize_with_toolchain,
};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

const RUNTIME_SOURCE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/runtime");
const SIDECAR_FILE_NAME: &str = "semantic-search-runtime.json";
const CACHE_SCHEMA_VERSION: u32 = 2;
const PACKAGE_NAME: &str = "lean-semantic-search";
const MATERIALIZED_PACKAGE_NAME: &str = "lean_semantic_search";
/// Limit on one Lake run, and on the wait for the cache entry's lock.
#[cfg(unix)]
const LAKE_TIMEOUT: Duration = Duration::from_secs(120);
/// Time a stopped Lake run has to exit before the call gives up on it.
#[cfg(unix)]
const STOP_GRACE: Duration = Duration::from_secs(10);
/// Interval between checks on a running Lake process.
#[cfg(unix)]
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Lake's combined output, kept in the package root after each run.
#[cfg(unix)]
const LAKE_LOG_FILE_NAME: &str = "lake-output.log";
/// Characters of Lake output kept in an error.
#[cfg(unix)]
const LAKE_DETAIL_CHARS: usize = 2000;

/// Digest of the runtime Lean payload, using the file-set rule in
/// `lean/VENDORING.md`.
pub const RUNTIME_SOURCE_DIGEST: &str = "d6824cce457af1f69d05287e02f94946c2896ece19f364581828a641fd7850d5";
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
/// The paths come from a Lake build that finished, or from Lake's report that
/// the modules are current. Put [`module_dir`](Self::module_dir) on
/// `LEAN_PATH` to import the modules.
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

/// Failure to materialize, build or open the runtime package.
///
/// The message names the step that failed and ends with the tail of Lake's
/// output when Lake ran.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(#[from] ErrorKind);

impl Error {
    /// Whether [`open`] found the modules missing or stale, so that [`build`]
    /// must run first.
    #[must_use]
    pub const fn needs_build(&self) -> bool {
        #[cfg(unix)]
        {
            matches!(self.0, ErrorKind::NeedsBuild { .. })
        }
        #[cfg(not(unix))]
        {
            false
        }
    }
}

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
    #[error("could not run `lake {command}` in {}: {source}", package_root.display())]
    Spawn {
        command: String,
        package_root: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[cfg(unix)]
    #[error("`lake {command}` in {} {outcome}: {detail}", package_root.display())]
    Lake {
        command: String,
        package_root: PathBuf,
        outcome: LakeOutcome,
        detail: String,
    },
    #[cfg(unix)]
    #[error("`lake build --no-build {LIBRARY_NAME}` reports that {} needs a build", package_root.display())]
    NeedsBuild { package_root: PathBuf },
}

/// How a Lake run that did not succeed ended.
#[cfg(unix)]
#[derive(Debug)]
enum LakeOutcome {
    Failed(ExitStatus),
    Cancelled,
    TimedOut,
    DidNotStop,
}

#[cfg(unix)]
impl std::fmt::Display for LakeOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(status) => write!(formatter, "failed with {status}"),
            Self::Cancelled => formatter.write_str("was cancelled and stopped"),
            Self::TimedOut => write!(
                formatter,
                "ran past its {} s limit and was stopped",
                LAKE_TIMEOUT.as_secs()
            ),
            Self::DidNotStop => write!(
                formatter,
                "did not stop within {} s of a stop request and may still run",
                STOP_GRACE.as_secs()
            ),
        }
    }
}

/// Build the packaged runtime's Lean modules for one toolchain.
///
/// This materializes the package under the cache root and runs
/// `lake build LeanSemanticSearch` there with the Lake of `lean_sysroot`. A
/// file lock on the cache entry makes calls on one cache take turns. Nothing
/// may load the modules while a build runs, so a host builds before it serves
/// and then uses [`open`]. The Lake run has a two-minute limit, and
/// `cancelled` stops it sooner.
///
/// # Errors
///
/// Returns [`Error`] on cancellation, materialization, timeout or Lake failure.
#[cfg(unix)]
pub fn build(input: SemanticSearchRuntimeBuild, cancelled: impl Fn() -> bool) -> Result<SemanticSearchRuntime, Error> {
    run_in_package(input, &["build", LIBRARY_NAME], &cancelled)
}

/// Open the runtime that [`build`] made, without building it.
///
/// Lake decides whether the modules are current: this runs
/// `lake build --no-build LeanSemanticSearch`, which builds nothing. If the
/// cache entry is missing, this writes its source files first.
///
/// # Errors
///
/// Returns [`Error`] on materialization or Lake failure. When the modules are
/// missing or stale, [`Error::needs_build`] is true.
#[cfg(unix)]
pub fn open(input: SemanticSearchRuntimeBuild) -> Result<SemanticSearchRuntime, Error> {
    run_in_package(input, &["build", "--no-build", LIBRARY_NAME], &|| false)
}

#[cfg(unix)]
fn run_in_package(
    input: SemanticSearchRuntimeBuild,
    args: &[&str],
    cancelled: &impl Fn() -> bool,
) -> Result<SemanticSearchRuntime, Error> {
    ensure_runtime_payload_source(Path::new(RUNTIME_SOURCE_ROOT))?;
    let provenance = SemanticSearchRuntimeProvenance::new(&input.toolchain_label);
    let request = source_package_request(input.cache_root, &input.toolchain_label, &provenance)?;
    with_materialized_source_package(&request, LAKE_TIMEOUT, cancelled, |materialized| {
        let package_root = canonicalize(&materialized.project_root, "resolve semantic-search runtime package")?;
        let mut lake = Command::new(input.lean_sysroot.join("bin/lake"));
        lake.env("LEAN_SYSROOT", &input.lean_sysroot)
            .args(args)
            .current_dir(&package_root);
        let command = args.join(" ");
        let status = run_lake(lake, &command, &package_root, cancelled)?;
        if status.code() == Some(3) && args.contains(&"--no-build") {
            return Err(ErrorKind::NeedsBuild { package_root }.into());
        }
        if !status.success() {
            return Err(lake_error(&command, &package_root, LakeOutcome::Failed(status)).into());
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
    })
}

/// Run Lake in its own process group with output to the package's log, and
/// return its exit status. A timeout or cancellation stops the whole group.
#[cfg(unix)]
fn run_lake(
    mut lake: Command,
    command: &str,
    package_root: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<ExitStatus, ErrorKind> {
    let spawn_error = |source| ErrorKind::Spawn {
        command: command.to_owned(),
        package_root: package_root.to_path_buf(),
        source,
    };
    let log = fs::File::create(package_root.join(LAKE_LOG_FILE_NAME)).map_err(spawn_error)?;
    lake.stdin(Stdio::null())
        .stdout(log.try_clone().map_err(spawn_error)?)
        .stderr(log);
    let mut group = OwnedCommandGroup::spawn(lake).map_err(spawn_error)?;
    let started = Instant::now();
    // When a stop was asked for, and the outcome to report once the group
    // closes. `None` means Lake itself exited and only leftovers were stopped.
    let mut stopping: Option<(Instant, Option<LakeOutcome>)> = None;
    loop {
        if let Some(status) = group.try_wait().map_err(spawn_error)? {
            return match stopping {
                Some((_, Some(outcome))) => Err(lake_error(command, package_root, outcome)),
                _ => Ok(status),
            };
        }
        match &stopping {
            Some((asked, _)) if asked.elapsed() >= STOP_GRACE => {
                return Err(lake_error(command, package_root, LakeOutcome::DidNotStop));
            }
            Some(_) => {}
            None => {
                let reason = if cancelled() {
                    Some(Some(LakeOutcome::Cancelled))
                } else if started.elapsed() >= LAKE_TIMEOUT {
                    Some(Some(LakeOutcome::TimedOut))
                } else {
                    group.cleanup_pending().then_some(None)
                };
                if let Some(reason) = reason {
                    group.request_stop().map_err(spawn_error)?;
                    stopping = Some((Instant::now(), reason));
                }
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(unix)]
fn lake_error(command: &str, package_root: &Path, outcome: LakeOutcome) -> ErrorKind {
    let log = fs::read(package_root.join(LAKE_LOG_FILE_NAME)).unwrap_or_default();
    ErrorKind::Lake {
        command: command.to_owned(),
        package_root: package_root.to_path_buf(),
        outcome,
        detail: lake_detail(&String::from_utf8_lossy(&log)),
    }
}

/// Lake's output on one line, cut to its last [`LAKE_DETAIL_CHARS`]
/// characters, where Lake prints its error summary.
#[cfg(unix)]
fn lake_detail(output: &str) -> String {
    let line = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let skip = line.chars().count().saturating_sub(LAKE_DETAIL_CHARS);
    line.chars().skip(skip).collect()
}

/// Materialize the packaged runtime source package without building it.
///
/// It starts no compiler.
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
        // The SDK asks for a source identity beside the digest. The digest is
        // the only one this crate keeps, so it serves as both.
        source_revision: RUNTIME_SOURCE_DIGEST.to_owned(),
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

    /// A Lean sysroot whose `bin/lake` is the given shell script.
    #[cfg(unix)]
    fn fake_sysroot(root: &Path, script: &str) -> Result<PathBuf, String> {
        use std::os::unix::fs::PermissionsExt as _;
        let bin = root.join("sysroot/bin");
        fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
        let lake = bin.join("lake");
        fs::write(&lake, format!("#!/bin/sh\n{script}\n")).map_err(|error| error.to_string())?;
        fs::set_permissions(&lake, fs::Permissions::from_mode(0o755)).map_err(|error| error.to_string())?;
        Ok(root.join("sysroot"))
    }

    #[cfg(unix)]
    fn runtime_build(root: &Path, lean_sysroot: PathBuf) -> super::SemanticSearchRuntimeBuild {
        super::SemanticSearchRuntimeBuild {
            cache_root: root.join("cache"),
            toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
            lean_sysroot,
        }
    }

    #[test]
    #[cfg(unix)]
    fn cancelled_build_never_materializes_or_launches() -> Result<(), String> {
        let temp = temp_cache()?;
        let sysroot = temp.path().join("missing-sysroot");
        let result = super::build(runtime_build(temp.path(), sysroot), || true);
        assert!(matches!(
            result,
            Err(Error(ErrorKind::SourcePackage {
                source: lean_toolchain::SourcePackageError::Io { source, .. },
            })) if source.kind() == std::io::ErrorKind::Interrupted
        ));
        let entries =
            fs::read_dir(temp.path().join("cache").join(RUNTIME_SOURCE_DIGEST)).map_err(|error| error.to_string())?;
        for entry in entries {
            assert!(!entry.map_err(|error| error.to_string())?.path().is_dir());
        }
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn open_returns_current_modules_and_reports_a_needed_build() -> Result<(), String> {
        let temp = temp_cache()?;
        let current = fake_sysroot(temp.path(), "mkdir -p .lake/build/lib/lean && exit 0")?;
        let runtime = super::open(runtime_build(temp.path(), current)).map_err(|error| error.to_string())?;
        assert!(runtime.module_dir().starts_with(runtime.package_root()));
        let log = read_string(&runtime.package_root().join(super::LAKE_LOG_FILE_NAME))?;
        assert!(log.is_empty());

        let stale = fake_sysroot(temp.path(), "echo \"$@\"; exit 3")?;
        let error = super::open(runtime_build(temp.path(), stale))
            .err()
            .ok_or("Lake reports a needed build")?;
        assert!(error.needs_build(), "{error}");
        let log = read_string(&runtime.package_root().join(super::LAKE_LOG_FILE_NAME))?;
        assert_eq!(log.trim(), "build --no-build LeanSemanticSearch");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn a_failed_build_reports_the_end_of_lake_output() -> Result<(), String> {
        let temp = temp_cache()?;
        let failing = fake_sysroot(temp.path(), "echo progress; echo 'error: build failed' >&2; exit 1")?;
        let error = super::build(runtime_build(temp.path(), failing), || false)
            .err()
            .ok_or("Lake fails")?;
        assert!(!error.needs_build());
        let message = error.to_string();
        assert!(message.contains("`lake build LeanSemanticSearch`"), "{message}");
        assert!(message.contains("failed with exit status: 1"), "{message}");
        assert!(message.ends_with("progress error: build failed"), "{message}");
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_stops_a_running_lake() -> Result<(), String> {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let temp = temp_cache()?;
        let slow = fake_sysroot(temp.path(), "sleep 30")?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancelled);
        let canceller = thread::spawn(move || {
            thread::sleep(std::time::Duration::from_millis(300));
            signal.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let error = super::build(runtime_build(temp.path(), slow), || cancelled.load(Ordering::SeqCst))
            .err()
            .ok_or("the run is cancelled")?;
        canceller.join().map_err(|_| "canceller panicked".to_owned())?;
        assert!(error.to_string().contains("was cancelled and stopped"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn lake_detail_keeps_the_end_of_the_output_on_one_line() {
        let output = format!("{}\nerror: build failed\nstderr detail\n", "progress ".repeat(1000));
        let detail = super::lake_detail(&output);
        assert!(!detail.contains('\n'));
        assert_eq!(detail.chars().count(), super::LAKE_DETAIL_CHARS);
        assert!(detail.ends_with("error: build failed stderr detail"));
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
