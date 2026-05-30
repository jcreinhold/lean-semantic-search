# Agent Rules

This repository is the shared semantic-search package for `lean-dup` and `lean-host-mcp`.

## Boundary

- Do not add search semantics to `lean-rs`; use its generic worker capability transport.
- Do not put downstream workflow policy in shared crates: no review-state fields, report presentation policy, experiment
  knobs, or production gates.
- Do not put transport-specific or project-runtime types in shared search crates.
- Do not expose raw Lean expressions, feature-key encodings, worker framing records, storage layout, or cache paths in
  public APIs or docs.
- Keep feature keys opaque. Callers may store and compare keys only under matching version fields.
- Prefer concrete private modules over traits until two real implementations exist.
- Public DTOs must carry schema or algorithm version fields when they cross repository boundaries.
- Create Rust crates around a hidden decision, not around importance. `contract` owns schema compatibility; `capability`
  owns worker-facing command identity; `retrieval` owns candidate-search internals (ranking weights, rarity weighting,
  fanout limits, broad-head pruning, bounded top-k). Keep ranking, fanout, and top-k policy private to `retrieval`;
  callers see only ranks, feature-family explanations, and diagnostics. Do not add storage, persistence, or downstream
  ranking policy to it, and do not expose raw feature keys or composite scores.

## Writing

- Write interface comments before new public APIs. Comments should state stable meaning and hidden ownership, not repeat
  field names.
- Keep README files oriented around navigation: what the crate owns, what it deliberately does not own, how to run it,
  and where to read next.

## Checks

Run the checks from `README.md` before handing off, or report exact unavailable-tool failures. At minimum, keep these
green when the tools are installed:

```sh
cargo fmt --all --check
cargo test
cargo clippy --all-targets -- -D warnings
lake -d lean build
mdwright fmt --check README.md AGENTS.md docs/architecture/*.md crates/*/README.md lean/README.md
taplo fmt --check
cargo deny check
```
