# lean-semantic-search-store

Persistent SQLite-backed semantic index implementing the retrieval `Corpus` seam.

This crate is the large-corpus counterpart to the in-memory inverted index in `lean-semantic-search-retrieval`. It owns
the semantic index only: opaque-key postings, per-key fanout, the document total, and the contract feature rows needed
to rebuild a corpus member into an anchor. A `Store` opens read-only and implements `Corpus`, so retrieval ranks over a
persisted index without loading it into memory—the resident set of a query stays proportional to the anchors planned and
postings touched, never to the corpus size. A `StoreBuilder` ingests declaration and feature items in any order, pairs
them with bounded memory, and publishes the result atomically by renaming a temp build into place.

It carries no declaration display text, module or kind fields, provenance, labels, probe caches, or any duplicate-audit
or proof-agent vocabulary—those stay with consumers. The `corpus_token`, `schema_version`, and `policy_version` are
recorded and exposed as opaque read-only facts; the store never interprets them. `Store::open_fresh` reuses a corpus
only on a matching opaque token and matching versions, turning every mismatch or corruption into a structured cache miss
rather than an error; `set_latest`/`cleanup` are neutral, latest-pointer-protecting, dry-run-by-default primitives over
content-addressed corpus directories. No other shared crate depends on this one, and `retrieval` takes no dependency on
it.

## Use it

```toml
[dependencies]
lean-semantic-search-store = "0.3"
```

```rust
use lean_semantic_search_store::{Ingest, Store, StoreBuilder};
use lean_semantic_search_retrieval::{Anchor, retrieve_across};
```

## See also

- [Project README](https://github.com/jcreinhold/lean-semantic-search/blob/main/README.md)
- [Boundary note](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/00-boundary.md)
- [Persistence seam](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/04-persistence.md)
- [SQLite store design](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/05-sqlite-store.md)
- [Cache lifecycle](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/06-cache-lifecycle.md)
- [Retrieval crate](https://github.com/jcreinhold/lean-semantic-search/blob/main/crates/retrieval/README.md)
- [DTO contract crate](https://github.com/jcreinhold/lean-semantic-search/blob/main/crates/contract/README.md)

## License

Licensed under MIT or Apache-2.0, at your option.
