# lean-semantic-search

Shared semantic-search package for Lean tooling.

This repository defines the boundary where reusable Lean semantic facts live. It is shared by `lean-dup` duplicate
search and `lean-host-mcp` proof-agent search, while keeping downstream workflow and presentation policies out of the
shared package.

The package provides Lean-side semantic feature extraction for declarations and source-backed proof goals, the Rust DTOs
that carry those facts across repository boundaries, and the command identity for the worker that serves them.
Retrieval, ranking, storage, and downstream shaping belong to the callers, not here.

## Repository Map

| Path | Purpose |
| --- | --- |
| `crates/contract` | Stable serde DTOs, opaque keys, diagnostics, version constants, and response envelopes. |
| `crates/capability` | Worker-facing command names, export names, advertised facts, and empty diagnostic helpers. |
| `lean/` | Lean package under `LeanSemanticSearch`, exporting declaration and proof-goal feature commands. |
| `docs/architecture` | Boundary notes and the capability contract. |

Start with [docs/architecture/00-boundary.md](docs/architecture/00-boundary.md) when deciding where a new concern
belongs. Use [docs/architecture/01-capability-contract.md](docs/architecture/01-capability-contract.md) when changing
export names, request shapes, response envelopes, or streaming behavior. Use
[docs/architecture/02-lean-features.md](docs/architecture/02-lean-features.md) when changing Lean-side feature
semantics.

## Boundary Summary

| Repository | Owns |
| --- | --- |
| `lean-rs` | Lean FFI, worker lifecycle, generic JSON and streaming capability transport, runtime facts. |
| `lean-semantic-search` | Lean semantic feature extraction, opaque feature DTOs, shared candidate evidence, future retrieval logic. |
| `lean-dup` | Duplicate-search workflow and presentation policy. |
| `lean-host-mcp` | Proof-agent response shaping and project runtime policy. |

The intended downstream callers are `lean-dup` duplicate search and `lean-host-mcp` proof-agent search. The shared
crates do not expose raw Lean expressions, feature-key encodings, storage layout, downstream presentation policy,
transport-specific response types, or project runtime internals.

## Requirements

- Rust stable, with the pinned minimum in `Cargo.toml` (`rust-version = "1.91"`).
- `clippy` and `rustfmt`; `rust-toolchain.toml` asks rustup for both.
- A Lean 4 toolchain visible to `lake`.
- Optional local tools for the full policy pass: `mdwright`, `taplo`, and `cargo-deny`.

## Developer Checks

```sh
cargo fmt --all --check
cargo test
lake -d lean build
lake -d lean test
cargo clippy --all-targets -- -D warnings
mdwright fmt --check README.md AGENTS.md docs/architecture/*.md crates/*/README.md lean/README.md
taplo fmt --check
cargo deny check
```

If `lake`, `mdwright`, `taplo`, or `cargo-deny` is unavailable, record the exact command failure and keep the repository
contents otherwise complete.

## Dependency Policy

Workspace dependencies are centralized in the root `Cargo.toml`. Use current compatible releases, and prefer major-only
requirements when the crate has a stable major version (`serde = "1"`, `serde_json = "1"`). Keep path dependencies in
`[workspace.dependencies]` so member crates inherit a single version and path.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option. See `LICENSE-APACHE` and
`LICENSE-MIT`.
