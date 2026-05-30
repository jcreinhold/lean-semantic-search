# lean-semantic-search-retrieval

Storage-neutral semantic candidate generation over Lean feature rows.

This crate owns how an anchor's features become a bounded, ranked set of candidate declarations: role and rarity
weighting, broad-head pruning, posting fanout limits, and bounded top-k selection. It consumes the opaque declaration
and proof-goal feature rows from `lean-semantic-search-contract` and returns ranked candidates explained in stable
feature-family terms, with structured diagnostics for pruning and saturation.

It is in-memory and storage-neutral: `SemanticIndex` is a view over rows the caller already holds, not a database. A
proof-goal anchor starts from a source-backed feature row, never from rendered goal text.

This crate deliberately does not store or persist an index, rank for any particular downstream workflow, expose raw
feature keys or composite scores, or abstract over candidate sources with a trait. Those are caller concerns or
premature generality. The only keys it touches are the opaque equality tokens it ingests.

## Use it

```toml
[dependencies]
lean-semantic-search-retrieval = "0.1"
```

```rust
use lean_semantic_search_retrieval::{Anchor, SemanticIndex};
```

## See also

- Project README: ../../README.md
- Boundary note: ../../docs/architecture/00-boundary.md
- Retrieval boundary: ../../docs/architecture/03-retrieval.md
- DTO contract crate: ../contract/README.md

## License

Licensed under MIT or Apache-2.0, at your option.
