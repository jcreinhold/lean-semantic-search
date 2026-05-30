# lean-semantic-search-capability

Worker-facing command identity for Lean semantic search.

This crate owns the names and advertised versions of the generic worker commands implemented by the Lean package:
metadata, doctor diagnostics, declaration features, proof-goal features, and the optional streaming declaration-feature
export. It also builds empty diagnostic response helpers for hosts that need to surface command failures in the shared
envelope shape.

The capability crate deliberately does not rank candidates, choose retrieval policy, shape downstream tool responses, or
know anything about storage. Its job is command identity over the generic `lean-rs-worker` transport.

## Status

The Lean extractor is implemented in the `lean/` package. This crate remains intentionally small: it advertises command
names, export names, versions, and structured metadata for hosts that load the capability.

## Use it

```toml
[dependencies]
lean-semantic-search-capability = "0.1"
```

```rust
use lean_semantic_search_capability::EXPORTS;
```

## See also

- Project README: ../../README.md
- Boundary note: ../../docs/architecture/00-boundary.md
- Capability contract: ../../docs/architecture/01-capability-contract.md
- Lean feature boundary: ../../docs/architecture/02-lean-features.md
- DTO contract crate: ../contract/README.md

## License

Licensed under MIT or Apache-2.0, at your option.
