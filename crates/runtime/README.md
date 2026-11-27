# lean-semantic-search-runtime

Package-owned runtime for the `LeanSemanticSearch` Lean package.

This crate ships the runtime Lean payload, materializes it under a caller-owned cache root for a requested Lean
toolchain, writes the downstream `lean-toolchain`, and runs `lake build LeanSemanticSearch` with an explicit Lean
sysroot. It returns the package root, the directory of built `.olean` files, and the package provenance. Lake decides
what is stale.

It deliberately does not start Lean processes, choose imports, rank proof candidates, expose raw feature keys, or expose
the runtime file manifest. Hosts such as `lean-host-mcp` put the returned module directory on `LEAN_PATH` for their own
supervised Lean processes.

## Use It

```rust
use std::path::PathBuf;

use lean_semantic_search_runtime::{SemanticSearchRuntimeBuild, build, open};

let input = SemanticSearchRuntimeBuild {
    cache_root: PathBuf::from("/path/to/host-owned/semantic-runtime-cache"),
    toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
    lean_sysroot: PathBuf::from("/path/to/elan/toolchain"),
};
// Once, before serving:
build(input.clone(), || false)?;
// While serving:
let runtime = open(input)?;
let lean_path = runtime.module_dir();
# Ok::<(), lean_semantic_search_runtime::Error>(())
```

The cache entry is keyed by the runtime source digest and sanitized toolchain label. Callers own the cache root; this
crate owns everything below it.

`build` and `open` run on Unix. `build` runs `lake build LeanSemanticSearch` in its own process group with a two-minute
limit; a timeout or the cancellation predicate stops the whole group. A file lock on the cache entry makes calls on one
cache take turns. Nothing may load the modules while a build runs, so a host builds before it serves. `open` runs
`lake build --no-build LeanSemanticSearch`, which builds nothing: Lake decides whether the modules are current, and
`Error::needs_build` reports when they are not. Lake's output goes to `lake-output.log` in the package root, and an
error message ends with its last 2,000 characters.

`materialize_source_package` serves downstream Lake projects, such as `lean-dup`, that `require` `LeanSemanticSearch`
and build it themselves. It starts no compiler.

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

Materialization renames the Lake package from `lean-semantic-search` to the plain identifier `lean_semantic_search`. The
provenance still records the digest of the payload before the rename.

## Checks

From the repository root:

```sh
cargo nextest run -p lean-semantic-search-runtime
cargo package --list -p lean-semantic-search-runtime
scripts/check-runtime-vendoring.sh
```

The ignored `build_and_open_against_explicit_sysroot` integration test needs an absolute
`LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT`. Against one temporary cache it checks that `open` reports a needed build, then
builds cold and warm, opens, and checks that all three return the same modules:

```sh
TMPDIR="$PWD/target/tmp" \
LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT="$(lean --print-prefix)" \
  cargo nextest run -p lean-semantic-search-runtime --run-ignored ignored-only
```

The normal Rust tests do not compile or load Lean; they run a fake `lake` script. A failed build keeps its cache for
inspection.
