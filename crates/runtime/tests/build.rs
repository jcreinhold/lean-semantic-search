//! Opt-in end-to-end build check for the packaged semantic runtime.

#![cfg(unix)]

use lean_semantic_search_runtime::{Error, SemanticSearchRuntimeBuild, build, open};
use std::path::{Path, PathBuf};

// Until the complete operation succeeds, even unwinding keeps the cache, so a
// failed build can be inspected.
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
#[ignore = "requires an explicit Lean sysroot"]
fn build_and_open_against_explicit_sysroot() -> Result<(), Box<dyn std::error::Error>> {
    let sysroot = std::env::var_os("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT")
        .map(PathBuf::from)
        .ok_or("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT is not set")?;
    if !sysroot.is_absolute() {
        return Err("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT must be absolute".into());
    }
    let toolchain = std::env::var("LEAN_SEMANTIC_SEARCH_RUNTIME_TOOLCHAIN")
        .unwrap_or_else(|_| "leanprover/lean4:v4.36.0".to_owned());
    with_build_cache(tempfile::tempdir()?, |cache_root| {
        let input = SemanticSearchRuntimeBuild {
            cache_root: cache_root.to_path_buf(),
            toolchain_label: toolchain.clone(),
            lean_sysroot: sysroot,
        };
        let unbuilt = open(input.clone());
        assert!(
            matches!(&unbuilt, Err(error) if error.needs_build()),
            "nothing is built yet: {unbuilt:?}"
        );

        let cold_start = std::time::Instant::now();
        let cold = build(input.clone(), || false)?;
        let cold_elapsed = cold_start.elapsed();

        let warm_start = std::time::Instant::now();
        let warm = build(input.clone(), || false)?;
        let warm_elapsed = warm_start.elapsed();

        let open_start = std::time::Instant::now();
        let opened = open(input)?;
        let open_elapsed = open_start.elapsed();

        assert_eq!(cold, warm);
        assert_eq!(cold, opened);
        assert!(cold.module_dir().starts_with(cold.package_root()));
        assert!(cold.module_dir().join("LeanSemanticSearch.olean").is_file());
        println!(
            "semantic_runtime_build toolchain={toolchain} cold_ms={} warm_ms={} open_ms={}",
            cold_elapsed.as_millis(),
            warm_elapsed.as_millis(),
            open_elapsed.as_millis()
        );
        Ok(())
    })?;
    Ok(())
}
