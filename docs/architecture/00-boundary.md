# Boundary

This repository is the standalone semantic-search layer shared by `lean-dup` and `lean-host-mcp`.

## Design Note

`lean-rs` is the generic Lean SDK. This package uses it to materialize its Lean source package and to run `lake build`
under the shared writer. It must not learn proof search, duplicate search, fingerprints, role features, ranking weights,
fanout policy, or candidate evidence.

Search semantics live here because they change for different reasons than worker transport, downstream workflow, or
proof-agent response shaping. A standalone package lets `lean-dup` and `lean-host-mcp` share semantic facts without
forcing either downstream caller to inherit the other caller's policy vocabulary.

## Alternatives considered

A standalone package hides semantic feature decisions behind shared DTOs and lets each downstream caller keep its own
presentation and workflow policy. Two alternatives were rejected:

- **Put semantic search in `lean-rs`.** Makes the SDK depend on feature algorithms and search policy, enlarging the
  public surface every SDK user inherits.
- **Let `lean-host-mcp` depend directly on `lean-dup-search`.** Leaks duplicate-search workflow policy into proof-agent
  search.

## Hidden Knowledge

| Layer | Hidden knowledge |
| --- | --- |
| Lean package | Expression traversal, binder scheduling, proof-goal extraction, role-feature assignment, broad-head marking, semantic algorithm versions. |
| Rust contract crate | Stable cross-repository JSON shapes, schema versions, opaque equality-key wrappers, diagnostic vocabulary. |
| Rust retrieval crate | Ranking and rarity weights, fanout and posting limits, broad-head pruning, the multi-lane bounded top-k, and the `Corpus` seam. |
| Rust store crate | SQLite schema, streaming build, atomic publish, and the read-only query path. |
| Rust runtime crate | Runtime Lean file set, source digest, generated downstream toolchain rule, cache materialization, provenance sidecar, and explicit-sysroot Lake build. |

A crate exists only to hide something; none starts as an empty placeholder.

The runtime crate hides package distribution. Downstream hosts ask for a built `LeanSemanticSearch` package for a
specific toolchain and sysroot; they do not copy `lean/`, reconstruct source digests, or decide which Lean files ship at
runtime.

## What Must Not Leak

Public APIs and crate docs must not expose raw expressions, feature-key internals, the store's schema, downstream
workflow policy, transport-specific response types, or project runtime internals.

Opaque feature keys are allowed as equality tokens only. Their encoding is private to the Lean package and meaningful
only with the matching algorithm version.

## Downstream Callers

The concrete downstream callers are:

- `lean-dup` duplicate search, which owns its workflow and reports;
- `lean-host-mcp` proof-agent search, which owns tool responses and fallback behavior.

This package does not migrate either caller; each adopts a new release itself.
