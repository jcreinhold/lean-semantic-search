# Lean Runtime Vendoring

This note records the runtime Lean package that downstream hosts may copy from this repository. It is provenance for
vendoring, not a new public API surface.

## Provenance

- `source_revision`: `9851b1c3d960088da5f2224c9ee7d8c828ac2de7`
- Lake package name: `lean-semantic-search`
- Lean library: `LeanSemanticSearch`
- Manifest fact: `lean/lake-manifest.json` has `"packages": []`.

Downstream hosts must generate their own `lean-toolchain` for the consumer/worker toolchain. The upstream
`lean/lean-toolchain` is useful for developing this repository, but it is not runtime authority for a vendored host
package.

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

When materializing the package, strip the upstream `lean/` prefix so `lakefile.lean` sits at the downstream package
root. Keep the license files with the vendored package.

## Runtime Source Digest

The runtime source digest covers the runtime source payload and license files, excluding this vendoring note so the
recorded value is not self-referential. Compute it from the repository root:

```sh
cd ~/Code/lean-semantic-search
{
  git ls-files -z --cached --others --exclude-standard -- \
    lean/lakefile.lean \
    lean/lake-manifest.json \
    lean/LeanSemanticSearch.lean \
    'lean/LeanSemanticSearch/**' \
    lean/README.md \
    LICENSE-APACHE \
    LICENSE-MIT
} | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256
```

`runtime_source_digest`: `929eabcdca88138cc50c57bcc3abb45cbe144acd5dc132a567e146671ec3dcf4`
