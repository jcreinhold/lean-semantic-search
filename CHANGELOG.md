# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow extracts the section matching a `vX.Y.Z` tag into the GitHub Release body, so every tagged version
must have a corresponding `## [X.Y.Z]` section here.

## [Unreleased]

## [0.1.0]

Initial release of the shared semantic-search package for Lean tooling.

### Added

- `lean-semantic-search-contract`: stable serde DTOs, opaque keys, diagnostics, version constants, and response
  envelopes — the cross-repository JSON contract.
- `lean-semantic-search-capability`: worker-facing command names, export names, advertised facts, and empty-diagnostic
  helpers.
- `lean-semantic-search-retrieval`: storage-neutral semantic candidate generation over feature rows, carrying its own
  `RETRIEVAL_POLICY_VERSION`.
- Lean feature-extraction package (`lean/LeanSemanticSearch`): canonical traversal, role features, module and
  declaration extraction, proof-goal features, JSON envelopes, and the five `@[export]` capability entry points.
