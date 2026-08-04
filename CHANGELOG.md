# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow extracts the section matching a `vX.Y.Z` tag into the GitHub Release body, so every tagged version
must have a corresponding `## [X.Y.Z]` section here.

## [Unreleased]

## [0.7.0]

### Changed

- Bumped the Lean toolchain pin to `leanprover/lean4:v4.33.0-rc2` and advanced onto the `lean-rs` 0.7 line:
  `lean-rs-worker-protocol` and `lean-toolchain` pins move from `"0.6"` to `"0.7"`. lean-rs 0.7.0 adds Lean 4.32.2 and
  4.33.0-rc2 to its supported toolchain window (both share the byte-identical `lean.h` ABI of their preceding
  compatible releases); the wire protocol is unchanged, so no contract or runtime behavior this package consumes
  changes, but the lean-rs types re-exported in this package's public API must stay on one lean-rs line downstream
  (`lean-host-mcp`). The Lean package compiles unchanged under the new toolchain; only the cosmetic
  `toolchain_label` test/doc literals in `lean-semantic-search-runtime` were refreshed. (`runtime_source_digest` moved
  only because this release's unified version bump touches `lean/lakefile.lean`, which is part of the vendored runtime
  payload.)

### Internal

- Raised the `deny.toml` `lean-rs`-family version floor from `>= 0.4` to `>= 0.7`, in lockstep with the workspace's
  adoption of the `lean-rs` `0.7` line, so a stale pre-`0.7` copy dragged in by a not-yet-upgraded consumer fails
  `cargo deny check` loudly instead of surfacing as a deep `E0308`.

## [0.6.0]

### Changed

- Advanced onto the `lean-rs` 0.6 line: `lean-rs-worker-protocol` and `lean-toolchain` pins move
  from `"0.5"` to `"0.6"`. lean-rs 0.6.0 adds pool-staleness eviction, byte-bounded import residue,
  and independent session-pool capacity in the worker stack; the wire protocol is unchanged
  (`PROTOCOL_VERSION` stays 10), so no contract or runtime behavior this package consumes changes,
  but the lean-rs types re-exported in this package's public API must stay on one lean-rs line
  downstream (`lean-host-mcp`). The minor bump carries that coupling.

## [0.5.0]

### Changed

- Advanced onto the `lean-rs` 0.5 line: `lean-rs-worker-protocol` and `lean-toolchain` pins move
  from `"0.4"` to `"0.5"`. lean-rs 0.5.0 adds `entry_goals`/`locals` to
  `LeanWorkerProofAttemptEnvelope` (additive, serde-defaulted) and repairs the host shim's
  declaration-candidate scan; neither changes the contract or runtime behavior this package
  consumes, but the lean-rs types re-exported in this package's public API must stay on one
  lean-rs line downstream (`lean-host-mcp`). The minor bump carries that coupling.


## [0.4.3]

### Changed

- Bumped the Lean toolchain pin to `leanprover/lean4:v4.33.0-rc1`, the toolchain the already-adopted `lean-rs` 0.4
  release is built against, so downstream hosts can build the worker against the pinned toolchain. The toolchain bump
  itself required no Lean source change (the package compiles unchanged); the only refreshed literals are the cosmetic
  `toolchain_label` test/doc examples in `lean-semantic-search-runtime`. (`runtime_source_digest` moved only because
  this release's unified version bump touches `lean/lakefile.lean`, which is part of the vendored runtime payload.)

### Internal

- Raised the `deny.toml` `lean-rs`-family version floor from `>= 0.3` to `>= 0.4`, in lockstep with the workspace's
  adoption of the `lean-rs` `0.4` line, so a stale pre-`0.4` copy dragged in by a not-yet-upgraded consumer fails
  `cargo deny check` loudly instead of surfacing as a deep `E0308`.

## [0.4.2]

### Changed

- Bumped the Lean toolchain pin to `leanprover/lean4:v4.32.0`, promoting the `-rc1` pin to the stable release. The Lean
  package compiles unchanged under it; `lean-rs` picked up the coordinated `0.3.1` patch release (still the `0.3` line,
  so no workspace-dependency, `deny.toml` floor, or `rust-version` change was needed). Only the cosmetic
  `toolchain_label` test/doc literals in `lean-semantic-search-runtime` were refreshed.

## [0.4.1]

### Changed

- Bumped the Lean toolchain pin to `leanprover/lean4:v4.32.0-rc1`, the toolchain the published `lean-rs` 0.3.0 release
  is built against, so downstream hosts can build the worker against the pinned toolchain. The Lean package compiles
  unchanged under it; the only refreshed literals are the cosmetic `toolchain_label` test/doc examples in
  `lean-semantic-search-runtime`. The `lean-rs` crates stay on the `0.3` line (already the latest), so no
  workspace-dependency, `deny.toml` floor, or `rust-version` change was needed.
- Realigned the Lean package version (`lean/lakefile.lean`) to the unified workspace version `0.4.1`; it had been left
  at `0.3.1` through the `0.4.0` release. Because the lakefile is part of the vendored runtime payload, this re-synced
  the copy under `crates/runtime/runtime/` and moved `RUNTIME_SOURCE_DIGEST` to match the new source.

### Internal

- Reformatted `deny.toml` with `taplo` (cosmetic key and array ordering only); the `lean-rs` `>= 0.3` floor and every
  ban entry are unchanged.

## [0.4.0]

### Changed

- **Breaking:** upgraded the `lean-rs` workspace crates (`lean-rs-worker-protocol`, `lean-toolchain`) from `0.2` to
  `0.3`. These crates surface `lean-toolchain` types (e.g. `LeanBuiltCapability`) in this crate's public API, so a
  consumer must move to the `lean-rs` `0.3` line in lockstep — `0.2.x` and `0.3.x` cannot coexist in one dependency
  graph.

### Internal

- `deny.toml` now version-floors the entire `lean-rs` crate family at `>= 0.3`. A future partial upgrade that drags a
  pre-0.3 copy back into the graph now fails `cargo deny check` with a clear, named diagnostic instead of surfacing as a
  deep `E0308` type mismatch in a downstream consumer.

## [0.3.1]

### Changed

- Upgraded the `lean-rs` workspace crates (`lean-rs-worker-protocol`, `lean-toolchain`) to 0.2.2 and bumped the Lean
  toolchain pin to `leanprover/lean4:v4.31.0-rc2` (header-identical to `-rc1`).
- `RoleFeatures.factsFromStatement` now deduplicates role features in O(n) via a hash set on the (injective) feature
  sort key, replacing the previous per-insert linear scan (O(n²) in a statement's feature count). The emitted feature
  rows are byte-identical — `featuresJson`/`sortedFeatures` re-sort by the same key, so only the distinct set, not
  insertion order, was ever observable — so the change is a pure performance fix with no cache-key or version impact.
  (`runtime_source_digest` updated for the source edit.)

## [0.3.0]

### Added

- `lean-semantic-search-runtime`: a package-owned runtime crate that ships the `LeanSemanticSearch` Lean capability
  payload, materializes it in a caller-owned per-toolchain cache with a generated downstream `lean-toolchain`, records
  provenance, builds via `CargoLeanCapability::lean_sysroot`, and returns a `LeanBuiltCapability` for downstream hosts.

### Changed

- `lean-semantic-search-runtime` now delegates source-package materialization to the shared `lean-toolchain` helper
  while preserving its public runtime API. Cache keys remain digest/toolchain based, provenance sidecars and generated
  downstream `lean-toolchain` files are still recorded, and the runtime payload remains a zero-dependency Lake package.

## [0.2.0]

### Added

- `lean-semantic-search-retrieval`: a `Corpus` trait—the storage seam a later persistent store implements—with the
  in-memory inverted index as the reference implementation, and `retrieve_across` for fanning one anchor across a slice
  of corpora into one bounded, ranked list.
- `lean-semantic-search-store`: a persisted, on-disk `Corpus` over SQLite—a streaming, order-agnostic build with a
  query-bounded resident set and an atomic single-file publish. `Store::open_fresh` reuses a corpus only on a matching
  opaque `corpus_token` and matching `schema_version`/`policy_version`, reporting every mismatch or corruption as a
  structured `CacheMiss` rather than an error; `set_latest`/`cleanup` are neutral, latest-pointer-protecting,
  dry-run-by-default primitives over content-addressed corpus directories. The store records the versions and the opaque
  token but never interprets the token's contents. See `docs/architecture/05-sqlite-store.md` and
  `docs/architecture/06-cache-lifecycle.md`.

### Changed

- `lean-semantic-search-retrieval`: bounded selection now bounds a fingerprint/statement lane and a role/binder lane
  separately and unions them, so a selective role match is not crowded out behind a fingerprint cohort.
  `RETRIEVAL_POLICY_VERSION` moves to `lean-semantic-search.retrieval.v2`. Ranking accumulates by `declaration_id`
  rather than a dense row index, so a non-contiguous backend can implement `Corpus`. See
  `docs/architecture/04-persistence.md`.

## [0.1.0]

Initial release of the shared semantic-search package for Lean tooling.

### Added

- `lean-semantic-search-contract`: stable serde DTOs, opaque keys, diagnostics, version constants, and response
  envelopes—the cross-repository JSON contract.
- `lean-semantic-search-capability`: worker-facing command names, export names, advertised facts, and empty-diagnostic
  helpers.
- `lean-semantic-search-retrieval`: storage-neutral semantic candidate generation over feature rows, carrying its own
  `RETRIEVAL_POLICY_VERSION`.
- Lean feature-extraction package (`lean/LeanSemanticSearch`): canonical traversal, role features, module and
  declaration extraction, proof-goal features, JSON envelopes, and the five `@[export]` capability entry points.
