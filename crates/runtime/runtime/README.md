# LeanSemanticSearch

Lean package that owns semantic feature extraction for the shared search boundary.

The package exports the command names described in
[`docs/architecture/01-capability-contract.md`](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/01-capability-contract.md).
Declaration feature commands import requested modules and emit canonical fingerprints, role features, low-signal
markers, and source spans where Lean can recover them. Proof-goal feature commands elaborate caller-supplied source and
extract features from the selected tactic goal's Lean expressions and local context.

Hosted callers must install Lean's module search path before invoking capability exports. The exports import and
elaborate against the current search path; they do not call `initSearchPath` or rebuild from `LEAN_PATH`. The standalone
test driver is the local caller that initializes a search path for `lake -d lean test`.

A separately supervised Lean process can instead call `GoalElaboration.runWithSetup` with the exact
`Lean.ModuleSetup` obtained from Lake for the requested module. It returns the same feature rows or structured error
as `GoalElaboration.run`, using the same request parser and goal selectors, while honoring Lake's options, import
artifacts, dynlibs and plugins. The caller holds the selected inputs and native images through process closure and
preserves the working directory for relative setup paths. This Lean-only API adds no FFI export or JSON setup field.

Build and test it from the repository root with:

```sh
lake -d lean build
lake -d lean test
```

The opt-in setup regression builds a tiny witness plugin once, then checks its saved Lake setup in fresh Lean
processes without rebuilding. It verifies real goal features and omitted/missing-plugin refusals, and removes its
`target/setup-semantic-check` fixture on success:

```sh
python3 tests/goal_setup.py
```

The Lean package should keep expression traversal and key encoding private. Rust callers receive only the versioned DTOs
defined by `lean-semantic-search-contract`. Start with
[`docs/architecture/02-lean-features.md`](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/02-lean-features.md)
before changing feature semantics.
