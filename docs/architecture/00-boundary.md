# Boundary

This repository is the standalone semantic-search layer shared by `lean-dup` and `lean-host-mcp`.

## Design Note

`lean-rs` remains only the communication and runtime substrate. It owns Lean FFI, process startup, session lifecycle,
generic JSON commands, generic streaming commands, timeouts, restart policy, and runtime facts. It must not learn proof
search, duplicate search, fingerprints, role features, ranking weights, fanout policy, or candidate evidence.

Search semantics live here because they change for different reasons than worker transport, duplicate-review workflow,
or MCP response shaping. A standalone package lets `lean-dup` and `lean-host-mcp` share semantic facts without forcing
either downstream caller to inherit the other caller's policy vocabulary.

## Design It Twice

| Design | Result |
| --- | --- |
| Put semantic search in `lean-rs`. | Rejected. It would make the worker substrate depend on feature algorithms and search policy, increasing the public surface of every worker user. |
| Let `lean-host-mcp` depend directly on `lean-dup-search`. | Rejected. It would leak duplicate-review workflow and audit policy into proof-agent search. |
| Create standalone `lean-semantic-search`. | Chosen. It hides semantic feature decisions behind shared DTOs and lets each downstream caller keep its own presentation and workflow policy. |

## Hidden Knowledge

| Layer | Hidden knowledge |
| --- | --- |
| Lean package | Expression traversal, binder scheduling, proof-goal extraction, role-feature assignment, broad-head marking, semantic algorithm versions. |
| Rust contract crate | Stable cross-repository JSON shapes, schema versions, opaque equality-key wrappers, diagnostic vocabulary. |
| Rust retrieval crate | Storage-neutral candidate generation, ranking weights, fanout and top-k limits, saturation diagnostics. Created when retrieval exists, not as an empty foundation crate. |
| Rust capability crate | Export names, typed command identity, request/response serde boundaries over generic worker transport. |

## What Must Not Leak

Public docs and APIs must not expose raw expressions, feature-key internals, worker rows, SQLite/cache layout,
duplicate-audit policy, MCP/HTTP response types, or project actor internals.

Opaque feature keys are allowed as equality tokens only. Their encoding is private to the Lean package and meaningful
only with the matching algorithm version.

## Downstream Callers

The concrete downstream callers are:

- `lean-dup` duplicate search, which owns review policy and reports;
- `lean-host-mcp` proof-agent search, which owns MCP tool responses and fallback behavior.

This foundation does not migrate either caller. It creates the boundary that later prompts can build on.
