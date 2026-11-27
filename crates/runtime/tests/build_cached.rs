//! Opt-in end-to-end build check for the packaged semantic runtime.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use lean_semantic_search_runtime::{Error, SemanticSearchRuntimeBuild, build_cached_with_writer};

// Until the complete operation succeeds, even unwinding must retain the cache:
// an SDK failure may mean its writer still owns work below this directory.
struct BuildCache {
    path: PathBuf,
    directory: Option<tempfile::TempDir>,
}

impl BuildCache {
    fn new(directory: tempfile::TempDir) -> Self {
        Self {
            path: directory.path().to_path_buf(),
            directory: Some(directory),
        }
    }

    fn retain(mut self) -> PathBuf {
        if let Some(directory) = self.directory.take() {
            return directory.keep();
        }
        self.path.clone()
    }

    fn close(mut self) -> std::io::Result<()> {
        if let Some(directory) = self.directory.take() {
            directory.close()?;
        }
        Ok(())
    }
}

impl Drop for BuildCache {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            eprintln!(
                "retained incomplete semantic runtime cache: {}",
                directory.keep().display()
            );
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum BuildCheckError {
    #[error("semantic runtime build failed; cache retained at {}: {source}", cache.display())]
    Build {
        cache: PathBuf,
        #[source]
        source: Box<Error>,
    },
    #[error("semantic runtime build succeeded; cache cleanup failed at {}: {source}", cache.display())]
    Cleanup {
        cache: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

fn with_build_cache<T>(
    directory: tempfile::TempDir,
    operation: impl FnOnce(&Path) -> Result<T, Error>,
) -> Result<T, BuildCheckError> {
    let cache = BuildCache::new(directory);
    match operation(&cache.path) {
        Ok(value) => {
            let path = cache.path.clone();
            cache
                .close()
                .map_err(|source| BuildCheckError::Cleanup { cache: path, source })?;
            Ok(value)
        }
        Err(source) => Err(BuildCheckError::Build {
            cache: cache.retain(),
            source: Box::new(source),
        }),
    }
}

#[test]
fn incomplete_build_cache_is_retained() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_path_buf();
    drop(BuildCache::new(directory));
    assert!(path.is_dir());
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[test]
fn successful_build_check_removes_cache() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_path_buf();
    with_build_cache(directory, |_| Ok(()))?;
    assert!(!path.exists());
    Ok(())
}

#[test]
fn unknown_writer_failure_retains_cache_and_original_error() -> Result<(), Box<dyn std::error::Error>> {
    use lean_toolchain::shared_writer::{WriterDisposition, WriterFailure};
    let directory = tempfile::tempdir()?;
    let path = directory.path().to_path_buf();
    let expected = WriterFailure {
        disposition: WriterDisposition::Unknown,
        detail: "writer completion is unresolved".to_owned(),
        stop_receipt: None,
    };
    let result: Result<(), BuildCheckError> = with_build_cache(directory, |_| {
        Err(Error::Build {
            toolchain_label: "test".to_owned(),
            source: lean_toolchain::LinkDiagnostics::LakeWriterFailure {
                project_root: path.clone(),
                target_name: "LeanSemanticSearch".to_owned(),
                failure: Box::new(expected.clone()),
            },
        })
    });
    let Err(BuildCheckError::Build { cache, source }) = result else {
        return Err("original typed writer failure was lost".into());
    };
    assert_eq!(cache, path);
    let Error::Build {
        source: lean_toolchain::LinkDiagnostics::LakeWriterFailure { failure, .. },
        ..
    } = *source
    else {
        return Err("original typed writer failure was lost".into());
    };
    assert_eq!(*failure, expected);
    assert!(path.is_dir());
    // The injected failure started no process; only this pure fixture can be removed.
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[test]
#[ignore = "requires an explicit Lean sysroot and SDK-compatible shared-writer helper"]
fn build_cached_against_explicit_sysroot() -> Result<(), Box<dyn std::error::Error>> {
    let sysroot = std::env::var_os("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT")
        .map(PathBuf::from)
        .ok_or("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT is not set")?;
    let writer = std::env::var_os("LEAN_RS_WRITER_EXECUTABLE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("LEAN_RS_WRITER_EXECUTABLE must select an absolute SDK-compatible helper")?;
    if !sysroot.is_absolute() {
        return Err("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT must be absolute".into());
    }
    let toolchain = std::env::var("LEAN_SEMANTIC_SEARCH_RUNTIME_TOOLCHAIN")
        .unwrap_or_else(|_| "leanprover/lean4:v4.36.0".to_owned());
    with_build_cache(tempfile::tempdir()?, |cache_root| {
        let cold_start = std::time::Instant::now();
        let cold = build_cached_with_writer(
            SemanticSearchRuntimeBuild {
                cache_root: cache_root.to_path_buf(),
                toolchain_label: toolchain.clone(),
                lean_sysroot: sysroot.clone(),
            },
            &writer,
            Duration::from_secs(120),
            || false,
        )?;
        let cold_elapsed = cold_start.elapsed();

        let warm_start = std::time::Instant::now();
        let warm = build_cached_with_writer(
            SemanticSearchRuntimeBuild {
                cache_root: cache_root.to_path_buf(),
                toolchain_label: toolchain.clone(),
                lean_sysroot: sysroot,
            },
            &writer,
            Duration::from_secs(120),
            || false,
        )?;
        let warm_elapsed = warm_start.elapsed();

        assert_eq!(cold.provenance, warm.provenance);
        assert_eq!(cold.built.package_name(), Some("lean_semantic_search"));
        assert_eq!(warm.built.module_name(), Some("LeanSemanticSearch"));
        println!(
            "semantic_runtime_build toolchain={toolchain} cold_ms={} warm_ms={}",
            cold_elapsed.as_millis(),
            warm_elapsed.as_millis()
        );
        Ok(())
    })?;
    Ok(())
}
