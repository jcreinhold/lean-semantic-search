# lean-semantic-search-capability

Worker-facing command identity for Lean semantic search.

This crate owns the names and advertised versions of the generic worker commands implemented by the Lean package:
metadata, doctor diagnostics, declaration features, proof-goal features, and the optional streaming declaration-feature
export. It also builds empty diagnostic response helpers for hosts that need to surface command failures in the shared
envelope shape.

This crate deliberately does not rank candidates, choose retrieval policy, shape downstream tool responses, or know
anything about storage. Its job is command identity over the generic `lean-rs-worker` transport: it advertises the
command names, export names, versions, and structured metadata that hosts need to load the capability the `lean/`
package implements.

## Use it

```toml
[dependencies]
lean-semantic-search-capability = "0.3"
```

```rust
use lean_semantic_search_capability::EXPORTS;
```

## See also

- [Project README](https://github.com/jcreinhold/lean-semantic-search/blob/main/README.md)
- [Boundary note](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/00-boundary.md)
- [Capability contract](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/01-capability-contract.md)
- [Lean feature boundary](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/02-lean-features.md)
- [DTO contract crate](https://github.com/jcreinhold/lean-semantic-search/blob/main/crates/contract/README.md)

## License

Licensed under MIT or Apache-2.0, at your option.
