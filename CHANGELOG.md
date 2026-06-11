# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow extracts the section matching a `vX.Y.Z` tag into the GitHub Release body, so every tagged version
must have a corresponding `## [X.Y.Z]` section here.

## [Unreleased]

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
