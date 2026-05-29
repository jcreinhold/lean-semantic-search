# lean-semantic-search

Shared semantic-search foundation for Lean tooling.

This repository defines the boundary where reusable Lean semantic facts live. It is shared by `lean-dup` duplicate
search and `lean-host-mcp` proof-agent search, while keeping their workflow and presentation policies out of the shared
package.

The current state is foundation-only: Rust DTOs, capability command identity, Lean export skeletons, architecture notes,
and checks. Real feature extraction, retrieval, ranking, storage, and downstream shaping arrive later.

## Repository Map

| Path | Purpose |
| --- | --- |
| `crates/contract` | Stable serde DTOs, opaque keys, diagnostics, version constants, and response envelopes. |
| `crates/capability` | Worker-facing command names, export names, advertised facts, and foundation responses. |
| `lean/` | Lean package under `LeanSemanticSearch`, exporting the generic capability commands. |
| `docs/architecture` | Boundary notes and the capability contract. |

Start with [docs/architecture/00-boundary.md](docs/architecture/00-boundary.md) when deciding where a new concern
belongs. Use [docs/architecture/01-capability-contract.md](docs/architecture/01-capability-contract.md) when changing
export names, request shapes, response envelopes, or streaming behavior.

## Boundary Summary

| Repository | Owns |
| --- | --- |
| `lean-rs` | Lean FFI, worker lifecycle, generic JSON and streaming capability transport, runtime facts. |
| `lean-semantic-search` | Lean semantic feature extraction, opaque feature DTOs, shared candidate evidence, retrieval logic. |
| `lean-dup` | Duplicate-review workflow, labels, baselines, reports, replacement guidance, production audit policy. |
| `lean-host-mcp` | MCP tools, proof-agent response shaping, project runtime policy, fallback behavior. |

The intended downstream callers are `lean-dup` duplicate search and `lean-host-mcp` proof-agent search. The shared
crates do not expose raw Lean expressions, feature-key encodings, storage layout, duplicate-review policy, MCP response
types, or project actor internals.

## Requirements

- Rust stable, with the pinned minimum in `Cargo.toml` (`rust-version = "1.91"`).
- `clippy` and `rustfmt`; `rust-toolchain.toml` asks rustup for both.
- A Lean 4 toolchain visible to `lake`.
- Optional local tools for the full policy pass: `mdwright`, `taplo`, and `cargo-deny`.

## Developer Checks

```sh
cargo fmt --all --check
cargo test
cargo clippy --all-targets -- -D warnings
lake -d lean build
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
