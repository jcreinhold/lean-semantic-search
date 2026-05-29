# lean-semantic-search-capability

Worker-facing command identity and foundation responses for Lean semantic search.

This crate owns the names and advertised versions of the generic worker commands implemented by the Lean package:
metadata, doctor diagnostics, declaration features, proof-goal features, and the optional streaming declaration-feature
export. It also builds the empty foundation responses returned before real Lean feature extraction exists.

The capability crate deliberately does not rank candidates, choose retrieval policy, shape downstream tool responses, or
know anything about storage. Its job is command identity over the generic `lean-rs-worker` transport.

## Status

Foundation-only. The command names and export names are stable for later prompts, while feature rows remain empty until
the Lean extractor is implemented.

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
- DTO contract crate: ../contract/README.md

## License

Licensed under MIT or Apache-2.0, at your option.
