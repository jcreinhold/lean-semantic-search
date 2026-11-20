//! Opt-in release smoke over a caller-owned real semantic worker and prepared artifacts.
#![cfg(unix)]

use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lean_rs_worker_parent::{
    LeanWorkerCapability, LeanWorkerCapabilityBuilder, LeanWorkerChild, LeanWorkerError, LeanWorkerJsonCommand,
    LeanWorkerProcessTracker, LeanWorkerShutdownReport,
};
use lean_rs_worker_protocol::types::{LeanWorkerHostRuntimeExpectation, LeanWorkerPreparedHostArtifacts};
use lean_semantic_search_capability::{DECLARATION_FEATURES_EXPORT, PROOF_GOAL_FEATURES_EXPORT};
use lean_semantic_search_contract::{
    CAPABILITY_SCHEMA_VERSION, CommandResponse, DECLARATION_FEATURE_COMMAND_VERSION, DeclarationFeatureRequest,
    DeclarationFeatureRow, DiagnosticSeverity, ModuleSpec, PROOF_GOAL_FEATURE_COMMAND_VERSION, ProofGoalFeatureRequest,
    ProofGoalFeatureRow, SEMANTIC_FEATURE_VERSION, SourcePosition,
};
use lean_semantic_search_retrieval::{Anchor, SemanticIndex, retrieve_across};
use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    root: PathBuf,
    worker: PathBuf,
    sysroot: PathBuf,
    manifest: PathBuf,
    stale_source: PathBuf,
    host_artifacts: LeanWorkerPreparedHostArtifacts,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

fn canonical(path: &Path) -> Result<()> {
    require(
        path.is_absolute() && path.canonicalize()? == path,
        "explicit canonical input path required",
    )
}

impl Inputs {
    fn validate(&self, input_file: &Path) -> Result<()> {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/native-smoke")
            .canonicalize()?;
        for path in [
            &self.root,
            &self.worker,
            &self.sysroot,
            &self.manifest,
            &self.stale_source,
            input_file,
        ] {
            canonical(path)?;
        }
        require(
            self.root != base && self.root.starts_with(&base),
            "smoke requires a dedicated normal-target input directory",
        )?;
        for path in [&self.manifest, &self.stale_source, input_file] {
            require(
                path.starts_with(&self.root) && path.is_file(),
                "smoke inputs must be owned regular files",
            )?;
        }
        for path in std::iter::once(&self.host_artifacts.shim_manifest_path)
            .chain(&self.host_artifacts.artifact_roots)
            .chain(&self.host_artifacts.shim_search_paths)
        {
            let path = Path::new(path);
            canonical(path)?;
            require(
                path.starts_with(&self.root),
                "host artifacts escaped the owned smoke inputs",
            )?;
        }
        for (key, name) in [
            (lean_toolchain::native_guardian::OWNERS_DIRECTORY_ENV, "owners"),
            ("TMPDIR", "tmp"),
        ] {
            let actual = PathBuf::from(std::env::var_os(key).ok_or_else(|| format!("explicit {key} required"))?);
            require(
                actual == self.root.join(name) && actual.is_dir(),
                "temporary/owner directory differs from owned inputs",
            )?;
        }
        Ok(())
    }

    fn builder(&self, tracker: &LeanWorkerProcessTracker) -> Result<LeanWorkerCapabilityBuilder> {
        Ok(LeanWorkerCapabilityBuilder::from_built_capability(
            &lean_toolchain::LeanBuiltCapability::manifest_path(&self.manifest),
            ["Init.Data.Nat.Basic"],
        )?
        .worker_child(
            LeanWorkerChild::for_toolchain(&self.worker, &self.sysroot)
                .prepared_host_artifacts(self.host_artifacts.clone())
                .checked_native_loader_environment(),
        )
        .process_tracker(tracker.clone())
        .require_host_runtime(LeanWorkerHostRuntimeExpectation {
            compiled_header_sha256: Some(lean_toolchain::LEAN_HEADER_DIGEST.to_owned()),
            runtime_lean_version: Some(lean_toolchain::LEAN_RESOLVED_VERSION.to_owned()),
            ..Default::default()
        })
        .num_threads(2)
        .startup_timeout(Duration::from_secs(30))
        .request_timeout(Duration::from_secs(30))
        .shutdown_timeout(Duration::from_secs(2))
        .json_command_export(DECLARATION_FEATURES_EXPORT)
        .json_command_export(PROOF_GOAL_FEATURES_EXPORT))
    }
}

struct Failure {
    operation: Option<Box<dyn Error>>,
    cleanup: Option<std::result::Result<LeanWorkerShutdownReport, LeanWorkerError>>,
    active_processes: usize,
    _owner: Option<LeanWorkerCapability>,
    _tracker: LeanWorkerProcessTracker,
}
impl std::fmt::Debug for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeSearchFailure")
            .field("operation", &self.operation)
            .field("cleanup", &self.cleanup)
            .field("active_processes", &self.active_processes)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}; retain owned smoke inputs until closure is confirmed")
    }
}
impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.operation.as_deref().or_else(|| {
            self.cleanup
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .map(|error| -> &(dyn Error + 'static) { error })
        })
    }
}

fn validate<Row>(response: &CommandResponse<Row>, version: &str) -> Result<()> {
    require(
        response.schema_version == CAPABILITY_SCHEMA_VERSION
            && response.command_version == version
            && response.feature_version == SEMANTIC_FEATURE_VERSION,
        "incompatible semantic response",
    )?;
    if response
        .diagnostics
        .iter()
        .any(|d| d.severity == DiagnosticSeverity::Error)
    {
        return Err(format!("semantic diagnostics: {:?}", response.diagnostics).into());
    }
    Ok(())
}

fn search(owner: &mut LeanWorkerCapability, sysroot: &Path) -> Result<()> {
    let mut session = owner.open_session(None, None)?;
    let goal = ProofGoalFeatureRequest {
        module: "NativeReleaseSmoke".to_owned(),
        source_text: "import Init\n\ntheorem native_add_comm (a b : Nat) : a + b = b + a := by\n  skip\n  exact Nat.add_comm a b\n".to_owned(),
        file_label: Some("NativeReleaseSmoke.lean".to_owned()), declaration: Some("native_add_comm".to_owned()),
        position: Some(SourcePosition { line: 4, column: 3 }), namespace: None,
    };
    let goals: CommandResponse<ProofGoalFeatureRow> = session.run_json_command(
        &LeanWorkerJsonCommand::new(PROOF_GOAL_FEATURES_EXPORT),
        &goal,
        None,
        None,
    )?;
    validate(&goals, PROOF_GOAL_FEATURE_COMMAND_VERSION)?;
    require(goals.rows.len() == 1, "expected one source-backed goal")?;
    let request = DeclarationFeatureRequest {
        modules: vec![ModuleSpec {
            module: "Init.Data.Nat.Basic".to_owned(),
            origin: Some("native-smoke".to_owned()),
            source_root: None,
        }],
        declaration_ids: Vec::new(),
    };
    let declarations: CommandResponse<DeclarationFeatureRow> = session.run_json_command(
        &LeanWorkerJsonCommand::new(DECLARATION_FEATURES_EXPORT),
        &request,
        None,
        None,
    )?;
    validate(&declarations, DECLARATION_FEATURE_COMMAND_VERSION)?;
    // Correlate the returned opaque ID with the real installed source span,
    // without interpreting feature keys or manufacturing an extractor ID.
    let source = fs::read_to_string(sysroot.join("src/lean/Init/Data/Nat/Basic.lean"))?;
    let line = source
        .lines()
        .position(|line| line.starts_with("protected theorem add_comm "))
        .and_then(|line| line.checked_add(1))
        .ok_or("Nat.add_comm source declaration missing")?;
    let line = u32::try_from(line)?;
    let target = declarations
        .rows
        .iter()
        .find(|row| row.source.is_some_and(|span| span.start.line == line))
        .ok_or("Nat.add_comm missing from actual extracted rows")?;
    let index = SemanticIndex::from_declarations(&declarations.rows);
    let retrieval = retrieve_across(
        &[&index],
        &Anchor::from_proof_goal(goals.rows.first().ok_or("missing goal")?),
        5,
    );
    require(
        !retrieval
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error),
        "native search returned an error diagnostic",
    )?;
    let first = retrieval
        .candidates
        .first()
        .ok_or("native search returned no candidates")?;
    require(
        first.declaration_id == target.declaration_id
            && first.rank == 1
            && first.explanations.iter().any(|e| e.match_count > 0),
        "native search did not rank Nat.add_comm first",
    )?;
    println!(
        "native search: {} declarations; Nat.add_comm rank {}",
        declarations.rows.len(),
        first.rank
    );
    Ok(())
}

#[test]
#[ignore = "requires a matching application-owned semantic worker and disposable prepared native inputs"]
fn native_search_and_stale_source_refusal() -> Result<()> {
    let path = PathBuf::from(
        std::env::var_os("LEAN_SEMANTIC_SEARCH_NATIVE_SMOKE_INPUTS")
            .ok_or("LEAN_SEMANTIC_SEARCH_NATIVE_SMOKE_INPUTS is not set")?,
    );
    let mut bytes = Vec::new();
    File::open(&path)?.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    require(bytes.len() <= 64 * 1024, "smoke input descriptor exceeds 64 KiB")?;
    let inputs: Inputs = serde_json::from_slice(&bytes)?;
    inputs.validate(&path)?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(inputs.root.join("native-search.once"))?;
    let tracker = LeanWorkerProcessTracker::new();
    let mut owner = None;
    let operation = (|| {
        owner = Some(inputs.builder(&tracker)?.open()?);
        search(owner.as_mut().ok_or("missing native owner")?, &inputs.sysroot)
    })();
    let cleanup = owner.as_mut().map(LeanWorkerCapability::try_shutdown);
    let active_processes = tracker.active_processes();
    if operation.is_err()
        || !cleanup
            .as_ref()
            .is_some_and(|r| r.as_ref().is_ok_and(|report| report.exit.success))
        || active_processes != 0
    {
        return Err(Box::new(Failure {
            operation: operation.err(),
            cleanup,
            active_processes,
            _owner: owner,
            _tracker: tracker,
        }));
    }
    println!("native closure: {cleanup:?}");
    drop(owner);
    OpenOptions::new()
        .append(true)
        .open(&inputs.stale_source)?
        .write_all(b"\n-- stale native release smoke input\n")?;
    let (mut unexpected, refusal): (_, Option<Box<dyn Error>>) = match inputs.builder(&tracker)?.open() {
        Ok(owner) => (Some(owner), None),
        Err(error) => (None, Some(Box::new(error))),
    };
    let cleanup = unexpected.as_mut().map(LeanWorkerCapability::try_shutdown);
    let active_processes = tracker.active_processes();
    let refused_source = refusal
        .as_ref()
        .is_some_and(|error| format!("{error:?}").contains(inputs.stale_source.to_string_lossy().as_ref()));
    if !refused_source || active_processes != 0 {
        return Err(Box::new(Failure {
            operation: refusal.or_else(|| Some("changed source was accepted or refusal omitted its identity".into())),
            cleanup,
            active_processes,
            _owner: unexpected,
            _tracker: tracker,
        }));
    }
    println!("changed-source refusal: {refusal:?}; active processes: {active_processes}");
    fs::remove_dir_all(&inputs.root)?;
    Ok(())
}
