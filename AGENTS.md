# lean-semantic-search

`lean-semantic-search` is the **shared semantic-search package** for Lean tooling. It is the boundary where reusable
Lean semantic facts live, consumed by two downstream callers it does not contain: `lean-dup` (duplicate search) and
`lean-host-mcp` (proof-agent search). It depends on `lean-rs`/`lean-rs-worker` only as a generic transport substrate.

The repo is a dual Lean + Rust workspace: the **Lean package** does semantic feature extraction; the **Rust crates**
define the stable JSON contract and command identity that cross repository boundaries. There is no retrieval, ranking,
or storage yet—those arrive later and must not be added speculatively.

## Commands

```sh
# Rust
cargo test                                   # uses nextest config in .config/nextest.toml (2 threads, no fail-fast)
cargo test -p lean-semantic-search-contract  # single crate
cargo test <name>                            # single test by substring
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings

# Lean (always pass -d lean from the repo root; the package lives in lean/)
lake -d lean build
lake -d lean test                            # runs the `tests` exe (Main root)

# Full policy pass (optional local tools)
mdwright fmt --check README.md AGENTS.md docs/architecture/*.md crates/*/README.md lean/README.md
taplo fmt --check
cargo deny check
```

Toolchain is pinned: Rust stable with `rust-version = "1.91"`, edition 2024; a Lean 4 toolchain visible to `lake`.
`.cargo/config.toml` sets `LEAN_RS_NUM_THREADS=1` and `RUST_LOG=warn` for every cargo invocation—that is the single
source of truth for test env vars (do not duplicate them in nextest config). Lean import paths can take seconds on a
cold process; the nextest `slow-timeout` warns rather than killing until ~4 minutes.

If `lake`, `mdwright`, `taplo`, or `cargo-deny` is unavailable, record the exact command failure rather than working
around it.

## Architecture: the boundary is the point

The whole design is about **what each layer is forbidden to know**. Read `docs/architecture/00-boundary.md` before
deciding where a new concern belongs, and `docs/architecture/01-capability-contract.md` before touching export names,
request shapes, response envelopes, or versions. Each crate/module is organized around a *hidden decision*, not around
importance—create a new Rust crate only when it hides something (e.g. retrieval becomes a crate when candidate-search
internals exist, never as an empty placeholder).

**Lean package (`lean/LeanSemanticSearch/`)** owns extraction; the split follows information ownership, not algorithm
steps:
- `Canonical`: expression traversal, universe normalization, binder scheduling, fingerprint keys (`canonical.expr.v3`).
- `RoleFeatures`: role assignment, broad-head/low-signal marking, private role-key encoding.
- `ModuleExtraction`: search-path setup, import, declaration/private/generated filtering, source-range lookup.
- `DeclarationFeatures`: combines accepted declarations with fingerprints + role features.
- `GoalFeatures` / `GoalElaboration`: same semantic facts from an open proof goal; elaboration + goal selection live
  together because they share source maps, info trees, and metavariable contexts.
- `Json`: command envelopes, version strings, request parsing, structured diagnostics.
- `Capability`: the five `@[export]` entry points (the worker ABI).

**Rust crates (`crates/`)**:
- `contract`: stable serde DTOs, opaque keys, diagnostics, version constants, response envelopes. The cross-repository
  JSON contract.
- `capability`: worker-facing command names, export names, advertised facts, empty-diagnostic helpers. Intentionally
  small: command identity over generic transport.
- `retrieval`: storage-neutral semantic candidate generation over feature rows. Hides ranking weights, rarity weighting,
  fanout/posting limits, broad-head pruning, and the multi-lane bounded top-k. Callers see ranked candidates +
  feature-family explanations + diagnostics, never postings, heaps, composite scores, or raw keys. Ranks over a `Corpus`
  trait (the seam a later persistent store fills) with the in-memory inverted index as the reference impl;
  `retrieve_across` fans one anchor across a slice of corpora. No storage dependency, no on-disk layout, no downstream
  ranking policy in this crate. Carries its own `RETRIEVAL_POLICY_VERSION` (`lean-semantic-search.retrieval.v2`); adds
  no DTOs to `contract`. See `docs/architecture/04-persistence.md`.

**The Lean exports and Rust constants must stay in lockstep.** The five `@[export lean_semantic_search_*]` functions in
`Capability.lean` correspond one-to-one to the `*_EXPORT`/`*_COMMAND` constants in `crates/capability/src/lib.rs`, and
the JSON shapes they emit correspond to the DTOs in `crates/contract/src/lib.rs`. Changing a command name, payload
shape, or version in one side requires updating the other and the contract doc.

## Boundary rules

These constrain changes more than the compiler does:

- Do not add search semantics to `lean-rs`; use its generic worker capability transport.
- Do not put downstream workflow policy in shared crates: no review-state fields, report presentation policy, experiment
  knobs, or production gates.
- Do not put transport-specific or project-runtime types in shared search crates.
- Do not expose raw Lean expressions, feature-key encodings, worker framing records, storage layout, or cache paths in
  public APIs or docs.
- Keep feature keys opaque. Fingerprint strings currently carry a `canonical.expr.v3` prefix, but callers may store and
  compare keys only under matching version fields.
- Prefer concrete private modules over traits until two real implementations exist. The `Corpus` trait in `retrieval` is
  the deliberate exception: it is the seam a persistent store implements, introduced ahead of that second implementor;
  see `docs/architecture/04-persistence.md`.
- Public DTOs crossing repository boundaries must carry a schema or algorithm version field. Version strings are
  centralized as constants in `contract` and mirrored in the Lean `Json` module. The current versions: capability schema
  `lean-semantic-search.capability.v1`, canonical `canonical.expr.v3`, role-key `features.role_key.v1`, feature rows
  `features.roles.v3`, declaration command `declaration_features.v1`, proof-goal command `proof_goal_features.v1`.
- Command failures stay inside the envelope as structured diagnostics (rows `[]` + an error diagnostic) rather than
  failing the transport—malformed JSON, bad selectors, import failures, and unavailable proof states all return this
  way.
- Proof-goal features are source-backed, computed from elaborated Lean expressions—never from pretty-printed goal text.
- Create Rust crates around a hidden decision, not around importance. Keep ranking, fanout, and top-k policy private to
  `retrieval`; callers see only ranks, feature-family explanations, and diagnostics. Retrieval takes no storage
  dependency: persistence enters only as the `Corpus` seam, never as a database, file, or on-disk layout in the shared
  crates. The `Corpus` seam carries no display, provenance, or audit field.

## Writing

- Write interface comments before new public APIs. Comments should state stable meaning and hidden ownership, not repeat
  field names.
- Keep README files oriented around navigation: what the crate owns, what it deliberately does not own, how to run it,
  and where to read next.

## Rust lint posture

The workspace `[workspace.lints]` in `Cargo.toml` is deliberately strict: `unsafe-code = "forbid"`, and a large clippy
restriction set warns on `unwrap_used`/`expect_used`/`panic`/`indexing_slicing`/`arithmetic_side_effects`/`todo`, with
`disallowed_methods`/`disallowed_types`/`mem_forget` denied. Write code that avoids these rather than allowing them
locally. Numeric casts and float arithmetic are explicitly allowed for this domain.

## Dependency policy

Workspace dependencies are centralized in the root `Cargo.toml` `[workspace.dependencies]`. Prefer major-only version
requirements for crates with a stable major (`serde = "1"`). Keep path dependencies in the workspace table so member
crates inherit one version and path.
