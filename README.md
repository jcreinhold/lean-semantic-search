# lean-semantic-search

Shared semantic-search package for Lean tooling.

This repository defines the boundary where reusable Lean semantic facts live. It is shared by `lean-dup` duplicate
search and `lean-host-mcp` proof-agent search, while keeping downstream workflow and presentation policies out of the
shared package.

The package provides Lean-side semantic feature extraction for declarations and source-backed proof goals, the Rust DTOs
that carry those facts across repository boundaries, ranking over feature rows, a SQLite corpus store, and the packaged
Lean runtime that hosts build. Downstream shaping belongs to the callers.

## Repository Map

| Path | Purpose |
| --- | --- |
| `crates/contract` | Stable serde DTOs, opaque keys, diagnostics, version constants, and response envelopes. |
| `crates/retrieval` | Storage-neutral semantic candidate generation: ranking, fanout limits, and bounded top-k over feature rows. |
| `crates/runtime` | Package-owned Lean runtime payload, provenance, cache materialization, and explicit-sysroot Lake build. |
| `crates/store` | Persistent SQLite-backed `Corpus`: unified postings, streaming order-agnostic build, atomic publish, query-bounded resident set. |
| `lean/` | Lean package under `LeanSemanticSearch`, with the declaration and proof-goal feature entry points hosts call. |
| `docs/architecture` | Boundary notes, the capability contract, the retrieval boundary, the persistence seam, and the SQLite store. |

Start with [docs/architecture/00-boundary.md](docs/architecture/00-boundary.md) when deciding where a new concern
belongs. Use [docs/architecture/01-capability-contract.md](docs/architecture/01-capability-contract.md) when changing
entry points, request shapes, or response envelopes. Use
[docs/architecture/02-lean-features.md](docs/architecture/02-lean-features.md) when changing Lean-side feature
semantics, [docs/architecture/03-retrieval.md](docs/architecture/03-retrieval.md) when changing ranking, fanout, or
top-k policy, and [docs/architecture/05-sqlite-store.md](docs/architecture/05-sqlite-store.md) when changing the store.

## Boundary Summary

| Repository | Owns |
| --- | --- |
| `lean-rs` | Lean toolchain facts, source-package materialization, and process groups for the Lake runs. |
| `lean-semantic-search` | Lean semantic feature extraction, opaque feature DTOs, ranking, the SQLite corpus store, and the packaged Lean runtime. |
| `lean-dup` | Duplicate-search workflow and presentation policy. |
| `lean-host-mcp` | Proof-agent response shaping and project runtime policy. |

The intended downstream callers are `lean-dup` duplicate search and `lean-host-mcp` proof-agent search. The shared
crates do not expose raw Lean expressions, feature-key encodings, storage layout, downstream presentation policy,
transport-specific response types, or project runtime internals.

## Requirements

- Rust stable, with the pinned minimum in `Cargo.toml` (`rust-version = "1.94"`).
- `clippy` and `rustfmt`; `rust-toolchain.toml` asks rustup for both.
- `cargo-nextest` for the test suite.
- A Lean 4 toolchain visible to `lake`.
- Optional local tools for the full policy pass: `mdwright`, `taplo`, and `cargo-deny`.

## Developer Checks

```sh
cargo fmt --all --check
cargo nextest run --workspace
lake -d lean build
lake -d lean test
scripts/check-runtime-vendoring.sh
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
mdwright fmt --check README.md AGENTS.md docs/architecture/*.md crates/*/README.md lean/README.md lean/VENDORING.md
taplo fmt --check
cargo deny check
scripts/prerelease.sh  # all of the above, plus packaging and the CHANGELOG check
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
