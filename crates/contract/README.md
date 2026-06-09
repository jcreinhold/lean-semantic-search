# lean-semantic-search-contract

Stable JSON DTOs and version markers for the shared Lean semantic-search boundary.

This crate owns the cross-repository contract between the Lean feature package and its Rust callers. It defines
metadata, doctor diagnostics, module and proof-goal requests, feature rows, opaque fingerprints, role features,
streaming summaries, and version constants. Callers may serialize, store, and compare these values according to their
version fields, but they must not interpret opaque feature keys.

Declaration requests identify modules and optional declaration ids. Proof-goal requests carry source text plus a
module/declaration/position selector, so Lean elaborates and extracts from expressions rather than from rendered goal
text.

Expression traversal, key encoding, ranking policy, storage layout, duplicate-review workflow, and proof-agent response
shaping all belong outside this crate.

## Use it

```toml
[dependencies]
lean-semantic-search-contract = "0.3"
```

```rust
use lean_semantic_search_contract::CapabilityMetadata;
```

## See also

- [Project README](https://github.com/jcreinhold/lean-semantic-search/blob/main/README.md)
- [Boundary note](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/00-boundary.md)
- [Capability contract](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/01-capability-contract.md)

## License

Licensed under MIT or Apache-2.0, at your option.
