# Lean Runtime Payload

This note records the runtime Lean package shipped by `lean-semantic-search-runtime`. Downstream hosts should depend on
that crate instead of copying this source into their own repositories.

## Provenance

- `source_revision`: `f504ad7616de785fe5cbf6f9d41684f9bd552e23`
- Lake package name: `lean-semantic-search`
- Lean library: `LeanSemanticSearch`
- Manifest fact: `lean/lake-manifest.json` has `"packages": []`.

Downstream hosts must generate their own `lean-toolchain` for the consumer/worker toolchain. The upstream
`lean/lean-toolchain` is useful for developing this repository, but it is not runtime authority for the packaged runtime
crate or any downstream host.

## Runtime File Set

Include these upstream paths:

- `lean/lakefile.lean`
- `lean/lake-manifest.json`
- `lean/LeanSemanticSearch.lean`
- `lean/LeanSemanticSearch/**`
- `lean/README.md`
- `lean/VENDORING.md`
- `LICENSE-APACHE`
- `LICENSE-MIT`

Exclude these paths:

- `.lake`
- built artifacts such as `.olean`, `.ilean`, `.c`, `.so`, and `.dylib`
- `lean/lean-toolchain`
- `lean/Main.lean`
- `lean/LeanSemanticSearchTest.lean`
- `lean/LeanSemanticSearchTest/**`

`lean-semantic-search-runtime` stores this payload under `crates/runtime/runtime/`. When materializing the package, it
strips the upstream `lean/` prefix so `lakefile.lean` sits at the downstream package root, keeps the license files with
the materialized package, rewrites only Lake package metadata to the loader-safe identifier `lean_semantic_search`, and
writes a generated `lean-toolchain`.

## Runtime Source Digest

The runtime source digest covers the runtime source payload and license files, excluding README.md and this vendoring
note so documentation-only changes do not change the cache key and the recorded value is not self-referential. Compute
it from the repository root:

```sh
{
  git ls-files -z --cached --others --exclude-standard -- \
    lean/lakefile.lean \
    lean/lake-manifest.json \
    lean/LeanSemanticSearch.lean \
    'lean/LeanSemanticSearch/**' \
    LICENSE-APACHE \
    LICENSE-MIT
} | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256
```

`runtime_source_digest`: `83761ed97e78cea41ee275d783cd682f6d3112ee5bc2c34fe27b964180a3a3eb`

## Runtime Build Measurement

Recorded 2026-06-09 on macOS arm64 (`Darwin Mac 25.4.0`), Apple M4 Pro, Lean toolchain `leanprover/lean4:v4.31.0-rc1`.
Runtime payload: 14 Lean source files (`LeanSemanticSearch.lean` plus 13 files under `LeanSemanticSearch/`), zero Lake
packages.

Command:

```sh
LEAN_SEMANTIC_SEARCH_RUNTIME_SYSROOT="$(lean --print-prefix)" \
LEAN_SEMANTIC_SEARCH_RUNTIME_TOOLCHAIN="$(cat lean/lean-toolchain)" \
  cargo test -p lean-semantic-search-runtime --test build_cached -- --ignored --nocapture
```

Result: cold materialize + shared capability build `6378 ms`; immediate warm cache reuse + build check `6 ms`.
