# Architecture

Start here when deciding where a new semantic-search concern belongs.

- [Boundary](00-boundary.md): the repository split, hidden knowledge ownership, and forbidden leaks.
- [Capability contract](01-capability-contract.md): exported Lean command names, JSON envelopes, versions, and streaming
  skeleton.

The current repository is a foundation. Retrieval, ranking, storage-neutral candidate search, and real Lean feature
extraction should each land only when there is enough behavior to hide behind a clear owner.
