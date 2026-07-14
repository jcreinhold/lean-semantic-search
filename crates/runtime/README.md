# lean-semantic-search-runtime

Package-owned runtime for the `LeanSemanticSearch` Lean capability.

This crate ships the runtime Lean payload, materializes it under a caller-owned cache root for a requested Lean
toolchain, writes the downstream `lean-toolchain`, builds the `LeanSemanticSearch` shared capability with an explicit
Lean sysroot, and returns a `lean_toolchain::LeanBuiltCapability`.

It deliberately does not open worker sessions, choose imports, rank proof candidates, expose raw feature keys, or expose
the runtime file manifest. Hosts such as `lean-host-mcp` use this crate to obtain a built capability, then use
`lean-rs-worker-parent` to load it against their own consumer workspace.

## Use It

```rust
use lean_semantic_search_runtime::{SemanticSearchRuntimeBuild, build_cached};

let runtime = build_cached(SemanticSearchRuntimeBuild {
    cache_root: std::path::PathBuf::from("/tmp/semantic-runtime-cache"),
    toolchain_label: "leanprover/lean4:v4.32.0".to_owned(),
    lean_sysroot: std::path::PathBuf::from("/path/to/elan/toolchain"),
})?;
# Ok::<(), lean_semantic_search_runtime::Error>(())
```

The cache entry is keyed by the runtime source digest and sanitized toolchain label. Callers own the cache root; this
crate owns everything below it.

## Runtime Payload

The packaged payload lives under `runtime/` and mirrors the runtime subset of the repository `lean/` package:

- `lakefile.lean`
- `lake-manifest.json`
- `LeanSemanticSearch.lean`
- `LeanSemanticSearch/**`
- `README.md`
- `VENDORING.md`
- `LICENSE-APACHE`
- `LICENSE-MIT`

It excludes test modules, executable drivers, build outputs, and the upstream `lean/lean-toolchain`. Downstream
materialization always generates a fresh `lean-toolchain` from the caller's requested toolchain label.

The materialized cache entry rewrites only Lake package metadata from the source package name `lean-semantic-search` to
the loader-safe identifier `lean_semantic_search` before building. The source payload and runtime provenance remain tied
to the recorded source revision and digest.

## Checks

From the repository root:

```sh
cargo test -p lean-semantic-search-runtime
cargo package --list -p lean-semantic-search-runtime
scripts/check-runtime-vendoring.sh
```
