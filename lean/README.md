# LeanSemanticSearch

Lean package that owns semantic feature extraction for the shared search boundary.

The foundation package exports the command names described in
[`docs/architecture/01-capability-contract.md`](../docs/architecture/01-capability-contract.md) and returns valid empty
JSON responses with structured warnings. Later prompts will replace those placeholders with expression traversal, binder
scheduling, role-feature extraction, proof-goal extraction, and semantic algorithm versions.

Build it from the repository root with:

```sh
lake -d lean build
```

The Lean package should keep expression traversal and key encoding private. Rust callers receive only the versioned DTOs
defined by `lean-semantic-search-contract`.
