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
    toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
    lean_sysroot: std::path::PathBuf::from("/path/to/elan/toolchain"),
})?;
# Ok::<(), lean_semantic_search_runtime::Error>(())
```

The cache entry is keyed by the runtime source digest and sanitized toolchain label. On macOS, linked builds also
separate canonical sysroot paths and generate a library runpath to the selected sysroot's `lib/lean` directory. Callers
own the cache root; this crate owns everything below it.

Hosts that already own the SDK shared-writer executable use `build_cached_with_writer` on Unix. Pass an explicitly
selected absolute helper path, an admission timeout, and a cancellation predicate. The same helper acknowledges entry
and project lock custody before materialization, runs Lake, and retains custody through artifact publication. It must
implement the matching SDK shared-writer protocol. These build APIs provide no load or query authority and do not own
worker sessions.

`build_cached` remains available and uses the SDK's writer selection. Its source-entry transaction covers the whole
build and publication; its lexical entry lock alone does not survive controller death. Source-only
`materialize_source_package` remains available for downstream Lake dependencies and starts no writer or compiler.

Admission timeout is not a whole-request deadline. Cancellation asks the SDK to stop owned work. Errors preserve typed
SDK failures, including execution disposition and any validated stop receipt. A missing result is not permission to
retry, and unknown cleanup must not be treated as confirmed retirement. Consumers recover writer evidence from the error
source chain, including `WriterFailure::from_io` for a `SourcePackageError::Io` source. The returned capability keeps
its SDK manifest, package/module identity, and initializer descriptor; loading and retirement belong to the host.

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

Materialization rewrites Lake package metadata from `lean-semantic-search` to the loader-safe identifier
`lean_semantic_search`. A macOS linked build also generates its selected sysroot runpath; source-only materialization
keeps the original link configuration. The source payload and provenance retain their recorded revision and digest.

## Checks

From the repository root:

```sh
cargo test -p lean-semantic-search-runtime
cargo package --list -p lean-semantic-search-runtime
scripts/check-runtime-vendoring.sh
```

The ignored `build_cached_against_explicit_sysroot` integration test uses an absolute
`LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT` and absolute SDK-compatible `LEAN_RS_WRITER_EXECUTABLE`. It makes cold and warm
calls against one temporary cache and compares provenance. Run it only with an allocated native build lane and a
matching reviewed helper; the normal Rust tests do not compile or load Lean. A failed or incomplete native build retains
its cache and typed SDK error. Confirm writer retirement before removing that cache. This build check alone does not
validate capability loading or search.
