# lean-semantic-search-retrieval

Storage-neutral semantic candidate generation over Lean feature rows.

This crate owns how an anchor's features become a bounded, ranked set of candidate declarations: role and rarity
weighting, broad-head pruning, posting fanout limits, and a multi-lane bounded selection that keeps selective role
matches from being crowded out behind fingerprint cohorts. It consumes the opaque declaration and proof-goal feature
rows from `lean-semantic-search-contract` and returns ranked candidates explained in stable feature-family terms, with
structured diagnostics for pruning and per-lane saturation.

It ranks over a `Corpus` trait—the seam a persistent store later fills—with the in-memory inverted `SemanticIndex` as
the reference implementation: a view over rows the caller already holds, not a database. `retrieve_across` fans one
anchor across a slice of corpora and merges the result into one bounded, ranked list. A proof-goal anchor starts from a
source-backed feature row, never from rendered goal text.

This crate deliberately does not store or persist an index, take a storage dependency, rank for any particular
downstream workflow, expose raw feature keys or composite scores, or carry corpus identity, provenance, or audit policy.
Those are caller concerns. The only keys it touches are the opaque equality tokens it ingests, and its ranking
calibration carries its own `RETRIEVAL_POLICY_VERSION`.

## Use it

```toml
[dependencies]
lean-semantic-search-retrieval = "0.3"
```

```rust
use lean_semantic_search_retrieval::{Anchor, SemanticIndex, retrieve_across};
```

## See also

- [Project README](https://github.com/jcreinhold/lean-semantic-search/blob/main/README.md)
- [Boundary note](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/00-boundary.md)
- [Retrieval boundary](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/03-retrieval.md)
- [Persistence seam](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/04-persistence.md)
- [DTO contract crate](https://github.com/jcreinhold/lean-semantic-search/blob/main/crates/contract/README.md)

## License

Licensed under MIT or Apache-2.0, at your option.
