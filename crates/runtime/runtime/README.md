# LeanSemanticSearch

Lean package that owns semantic feature extraction for the shared search boundary.

The package exports the command names described in
[`docs/architecture/01-capability-contract.md`](../docs/architecture/01-capability-contract.md). Declaration feature
commands import requested modules and emit canonical fingerprints, role features, low-signal markers, and source spans
where Lean can recover them. Proof-goal feature commands elaborate caller-supplied source and extract features from the
selected tactic goal's Lean expressions and local context.

Hosted callers must install Lean's module search path before invoking capability exports. The exports import and
elaborate against the current search path; they do not call `initSearchPath` or rebuild from `LEAN_PATH`. The standalone
test driver is the local caller that initializes a search path for `lake -d lean test`.

Build and test it from the repository root with:

```sh
lake -d lean build
lake -d lean test
```

The Lean package should keep expression traversal and key encoding private. Rust callers receive only the versioned DTOs
defined by `lean-semantic-search-contract`. Start with
[`docs/architecture/02-lean-features.md`](../docs/architecture/02-lean-features.md) before changing feature semantics.
