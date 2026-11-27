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
use std::path::{Path, PathBuf};
use std::time::Duration;

use lean_semantic_search_runtime::{SemanticSearchRuntimeBuild, build_cached_with_writer};

let runtime = build_cached_with_writer(
    SemanticSearchRuntimeBuild {
        cache_root: PathBuf::from("/path/to/host-owned/semantic-runtime-cache"),
        toolchain_label: "leanprover/lean4:v4.36.0".to_owned(),
        lean_sysroot: PathBuf::from("/path/to/elan/toolchain"),
    },
    Path::new("/path/to/lean-rs-writer"),
    Duration::from_secs(120),
    || false,
)?;
let lean_path = runtime.module_dir();
# Ok::<(), lean_semantic_search_runtime::Error>(())
```

The cache entry is keyed by the runtime source digest and sanitized toolchain label. Callers own the cache root; this
crate owns everything below it.

`build_cached_with_writer` runs on Unix. Pass an explicitly selected absolute path to an SDK shared-writer helper, an
admission timeout, and a cancellation predicate. The helper takes custody of the cache entry and project locks before
materialization and keeps it while Lake runs, so concurrent callers on one cache wait their turn. The helper must speak
the matching SDK shared-writer protocol. The result grants no permission to load or query the modules.

The admission timeout is not a deadline for the whole request; the Lake run has its own two-minute limit. Cancellation
asks the SDK to stop owned work. A missing result is not permission to retry, and unknown cleanup must not be treated as
confirmed retirement. `Error` is opaque: its message names the failed step, and its source chain keeps the SDK's
`WriterFailure`, directly or inside an `std::io::Error` (read it with `WriterFailure::from_io`).

`materialize_source_package` serves downstream Lake projects, such as `lean-dup`, that `require` `LeanSemanticSearch`
and build it themselves. It starts no writer or compiler.

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
cargo test -p lean-semantic-search-runtime
cargo package --list -p lean-semantic-search-runtime
scripts/check-runtime-vendoring.sh
```

The ignored `build_cached_against_explicit_sysroot` integration test needs an absolute
`LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT` and an absolute SDK-compatible `LEAN_RS_WRITER_EXECUTABLE`. It builds cold and
warm against one temporary cache and checks that both calls return the same built modules:

```sh
TMPDIR="$PWD/target/tmp" \
LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT="$(lean --print-prefix)" \
LEAN_RS_WRITER_EXECUTABLE=/absolute/path/to/lean-rs-writer \
  cargo nextest run -p lean-semantic-search-runtime --run-ignored ignored-only
```

The normal Rust tests do not compile or load Lean. A failed build keeps its cache and the SDK error; confirm the writer
stopped before removing that cache.
