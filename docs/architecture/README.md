# Architecture

Start here when deciding where a new semantic-search concern belongs.

- [Boundary](00-boundary.md): the repository split, hidden knowledge ownership, and forbidden leaks.
- [Capability contract](01-capability-contract.md): exported Lean command names, JSON envelopes, versions, and streaming
- [Lean features](02-lean-features.md): Lean-side module boundaries for canonicalization, role features, module
  extraction, and proof-goal extraction.

Retrieval, ranking, and storage-neutral candidate search should each land only when there is enough behavior to hide
behind a clear owner.
