//! Opt-in end-to-end build check for the packaged semantic runtime.

use std::path::PathBuf;

use lean_semantic_search_runtime::{SemanticSearchRuntimeBuild, build_cached};

#[test]
#[ignore = "requires a Lean sysroot; set LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT to run"]
fn build_cached_against_explicit_sysroot() -> Result<(), String> {
    let sysroot = std::env::var_os("LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT")
        .map(PathBuf::from)
        .ok_or_else(|| "LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT is not set".to_owned())?;
    let toolchain = std::env::var("LEAN_SEMANTIC_SEARCH_RUNTIME_TOOLCHAIN")
        .unwrap_or_else(|_| "leanprover/lean4:v4.33.0-rc2".to_owned());
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let cold_start = std::time::Instant::now();
    let cold = build_cached(SemanticSearchRuntimeBuild {
        cache_root: temp.path().to_path_buf(),
        toolchain_label: toolchain.clone(),
        lean_sysroot: sysroot.clone(),
    })
    .map_err(|error| error.to_string())?;
    let cold_elapsed = cold_start.elapsed();

    let warm_start = std::time::Instant::now();
    let warm = build_cached(SemanticSearchRuntimeBuild {
        cache_root: temp.path().to_path_buf(),
        toolchain_label: toolchain.clone(),
        lean_sysroot: sysroot,
    })
    .map_err(|error| error.to_string())?;
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
}
