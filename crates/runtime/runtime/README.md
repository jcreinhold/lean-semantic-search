# LeanSemanticSearch

Lean package that owns semantic feature extraction for the shared search boundary.

Hosts call its entry points from their own Lean process; see
[`docs/architecture/01-capability-contract.md`](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/01-capability-contract.md).
`Capability.declarationFeatures` imports requested modules and emits canonical fingerprints, role features, low-signal
markers, and source spans where Lean can recover them. `GoalElaboration.runWithSetup` elaborates caller-supplied source
with the `Lean.ModuleSetup` that Lake selected for its module, then extracts features from the selected tactic goal's
Lean expressions and local context.

Neither entry point calls `initSearchPath` or rebuilds the search path from `LEAN_PATH`. A host installs the search path
for declaration imports, and passes the setup that honors Lake's options, import artifacts, dynlibs, and plugins for
proof goals. It keeps those inputs in place, and the working directory fixed for relative setup paths, until its process
exits. The standalone test driver is the local caller that initializes a search path for `lake -d lean test`.

Build and test it from the repository root with:

```sh
lake -d lean build
lake -d lean test
```

The Lean package should keep expression traversal and key encoding private. Rust callers receive only the versioned DTOs
defined by `lean-semantic-search-contract`. Start with
[`docs/architecture/02-lean-features.md`](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/02-lean-features.md)
before changing feature semantics.
