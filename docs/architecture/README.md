# Architecture

Start here when deciding where a new semantic-search concern belongs.

- [Boundary](00-boundary.md): the repository split, hidden knowledge ownership, and forbidden leaks.
- [Capability contract](01-capability-contract.md): exported Lean command names, JSON envelopes, versions, and the
  streaming export.
- [Lean features](02-lean-features.md): Lean-side module boundaries for canonicalization, role features, module
  extraction, and proof-goal extraction.
- [Retrieval](03-retrieval.md): storage-neutral semantic candidate generation — ranking, fanout limits, broad-head
  pruning, and bounded top-k over feature rows.
- [Persistence](04-persistence.md): the `Corpus` seam, multi-corpus fan-out, and the multi-lane recall guarantee — no
  database in this layer.
- [SQLite store](05-sqlite-store.md): the persisted, on-disk `Corpus` — unified postings, streaming order-agnostic
  build, atomic publish, and a query-bounded resident set.

Downstream ranking policy and corpus freshness should each land only when there is enough behavior to hide behind a
clear owner.
