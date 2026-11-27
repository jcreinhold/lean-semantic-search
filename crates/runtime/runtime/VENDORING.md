# Lean Runtime Payload

This note records the runtime Lean package shipped by `lean-semantic-search-runtime`. Downstream hosts should depend on
that crate instead of copying this source into their own repositories.

## Provenance

- `source_revision`: `c6cfa0dc75934d3487f8e018ad154cb104e23642`
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

`runtime_source_digest`: `b562d416e379da1c53e19b985deee1bcab1ecb0cd33120d2d586431b1cad0ac4`
