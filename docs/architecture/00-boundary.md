# Boundary

This repository is the standalone semantic-search layer shared by `lean-dup` and `lean-host-mcp`.

## Design Note

`lean-rs` remains only the communication and runtime substrate. It owns Lean FFI, process startup, session lifecycle,
generic JSON commands, generic streaming commands, timeouts, restart policy, and runtime facts. It must not learn proof
search, duplicate search, fingerprints, role features, ranking weights, fanout policy, or candidate evidence.

Search semantics live here because they change for different reasons than worker transport, downstream workflow, or
proof-agent response shaping. A standalone package lets `lean-dup` and `lean-host-mcp` share semantic facts without
forcing either downstream caller to inherit the other caller's policy vocabulary.

## Alternatives considered

A standalone package hides semantic feature decisions behind shared DTOs and lets each downstream caller keep its own
presentation and workflow policy. Two alternatives were rejected:

- **Put semantic search in `lean-rs`.** Makes the worker substrate depend on feature algorithms and search policy,
  enlarging the public surface every worker user inherits.
- **Let `lean-host-mcp` depend directly on `lean-dup-search`.** Leaks duplicate-search workflow policy into proof-agent
  search.

## Hidden Knowledge

| Layer | Hidden knowledge |
| --- | --- |
| Lean package | Expression traversal, binder scheduling, proof-goal extraction, role-feature assignment, broad-head marking, semantic algorithm versions. |
| Rust contract crate | Stable cross-repository JSON shapes, schema versions, opaque equality-key wrappers, diagnostic vocabulary. |
| Rust capability crate | Export names, typed command identity, request/response serde boundaries over generic worker transport. |

A crate exists only to hide something. Retrieval—storage-neutral candidate generation, ranking weights, fanout and top-k
limits, saturation diagnostics—becomes its own crate once that behavior exists, not before.

## What Must Not Leak

Public docs and APIs must not expose raw expressions, feature-key internals, worker rows, storage layout, downstream
workflow policy, transport-specific response types, or project runtime internals.

Opaque feature keys are allowed as equality tokens only. Their encoding is private to the Lean package and meaningful
only with the matching algorithm version.

## Downstream Callers

The concrete downstream callers are:

- `lean-dup` duplicate search, which owns its workflow and reports;
- `lean-host-mcp` proof-agent search, which owns tool responses and fallback behavior.

This package does not migrate either caller. It defines the boundary; retrieval and the downstream callers build on it.
