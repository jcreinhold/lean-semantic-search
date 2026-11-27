# lean-semantic-search

`lean-semantic-search` is the **shared semantic-search package** for Lean tooling. It is the boundary where reusable
Lean semantic facts live, consumed by two downstream callers it does not contain: `lean-dup` (duplicate search) and
`lean-host-mcp` (proof-agent search). It uses `lean-rs` only to materialize its Lean package and run `lake build` under
the SDK's shared writer.

The repo is a dual Lean + Rust workspace. The **Lean package** extracts semantic features. The **Rust crates** define
the JSON contract that crosses repository boundaries, rank candidates over feature rows, persist large corpora in
SQLite, and ship the Lean package to hosts. Add nothing a caller does not need yet.

## Commands

```sh
# Rust
cargo nextest run --workspace                       # 2 threads, no fail-fast (.config/nextest.toml)
cargo nextest run -p lean-semantic-search-contract  # one crate
cargo nextest run <name>                            # tests whose name contains <name>
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps  # rustdoc link lints; clippy does not see them

# Lean (always pass -d lean from the repo root; the package lives in lean/)
lake -d lean build
lake -d lean test                            # runs the `tests` exe (Main root)
scripts/check-runtime-vendoring.sh           # the runtime crate's Lean payload matches lean/ and ships

# Full policy pass (optional local tools)
mdwright fmt --check README.md AGENTS.md docs/architecture/*.md crates/*/README.md lean/README.md lean/VENDORING.md
taplo fmt --check
cargo deny check

# Every release gate in one run
TMPDIR="$PWD/target/tmp" ELAN_TOOLCHAIN="$(cat lean/lean-toolchain)" scripts/prerelease.sh
```

Prefix test runs with `TMPDIR="$PWD/target/tmp"` as well, so temp files stay in the checkout; create the directory
first. The normal Rust tests do not run Lean. The ignored runtime build test does (see `crates/runtime/README.md`); it
can take seconds, and the nextest `slow-timeout` warns rather than killing it until ~4 minutes.

Toolchain is pinned: Rust stable with `rust-version = "1.94"`, edition 2024; a Lean 4 toolchain visible to `lake`.

If `lake`, `mdwright`, `taplo`, or `cargo-deny` is unavailable, record the exact command failure rather than working
around it.

## Release rule

Before 1.0, a breaking change or a new feature bumps the minor version; anything else bumps the patch. Each user-visible
change adds an entry under `## [Unreleased]` in `CHANGELOG.md`, marked `Breaking:` when callers must change. The
`release-lean-semantic-search` skill cuts the release.

Test a change that spans the SDK and this repo together before pushing or tagging:

- Point each Cargo command at the local SDK with `--config` patches. From the repository root, with the SDK checked out
  beside it:

  ```sh
  sdk=../lean-rs/crates
  cargo nextest run --workspace \
    --config "patch.crates-io.lean-toolchain.path=\"$sdk/lean-toolchain\"" \
    --config "patch.crates-io.lean-rs-abi.path=\"$sdk/lean-rs-abi\""
  ```

  Cargo skips a patch whose version does not meet the requirement in `Cargo.toml`, so confirm the resolved crates with
  `cargo metadata --format-version 1` and the same patches.

- Patching rewrites `Cargo.lock`. Copy it aside first and copy it back afterwards; never commit a local path or a
  lockfile that names one.

- With the same patches, run the affected real Lean operation (for the runtime, the ignored build test in
  `crates/runtime/README.md`) and the Cargo checks that `scripts/prerelease.sh` runs. The script itself takes no
  patches, and its packaging step needs published crates.

- Do not publish an SDK crate just to test it here. When the whole change passes, release the SDK first, then resolve
  the published pins and run `scripts/prerelease.sh`.

## Architecture: the boundary is the point

The whole design is about **what each layer is forbidden to know**. Read `docs/architecture/00-boundary.md` before
deciding where a new concern belongs, and `docs/architecture/01-capability-contract.md` before touching entry points,
request shapes, response envelopes, or versions. Each crate/module is organized around a *hidden decision*, not around
importance: create a new Rust crate only when it hides something, never as an empty placeholder.

**Lean package (`lean/LeanSemanticSearch/`)** owns extraction; the split follows information ownership, not algorithm
steps:
- `Canonical`: expression traversal, universe normalization, binder scheduling, fingerprint keys (`canonical.expr.v3`).
- `RoleFeatures`: role assignment, broad-head/low-signal marking, private role-key encoding.
- `Hashing`: the one stable hash behind every opaque key; changing it changes every emitted key.
- `LeanCompat`: the only code that names volatile Lean internals: module import, declaration enumeration,
  private/generated filtering, source-range lookup, source elaboration, and tactic goal selection.
- `ModuleExtraction`: declaration request parsing.
- `DeclarationFeatures`: combines accepted declarations with fingerprints + role features.
- `GoalFeatures` / `GoalElaboration`: same semantic facts from an open proof goal. `GoalElaboration` parses the request;
  elaboration and goal selection stay together in `LeanCompat.Frontend` because they share source maps, info trees, and
  metavariable contexts.
- `Json`: command envelopes, version strings, request parsing, structured diagnostics.
- `Capability`: `declarationFeatures`, the declaration command entry point hosts call; it imports `GoalElaboration` so
  hosts reach `runWithSetup` through one import.

**Rust crates (`crates/`)**:
- `contract`: stable serde DTOs, opaque keys, diagnostics, version constants, response envelopes. The cross-repository
  JSON contract.
- `retrieval`: storage-neutral semantic candidate generation over feature rows. Hides ranking weights, rarity weighting,
  fanout/posting limits, broad-head pruning, and the multi-lane bounded top-k. Callers see ranked candidates +
  feature-family explanations + diagnostics, never postings, heaps, composite scores, or raw keys. Ranks over a `Corpus`
  trait with two implementations: the in-memory inverted index here and `store`'s SQLite index. `retrieve_across` fans
  one anchor across a slice of corpora. No storage dependency, no on-disk layout, no downstream ranking policy in this
  crate. Carries its own `RETRIEVAL_POLICY_VERSION` (`lean-semantic-search.retrieval.v2`); adds no DTOs to `contract`.
  See `docs/architecture/04-persistence.md`.
- `store`: the SQLite `Corpus` for corpora too large to hold in memory, such as mathlib. Hides the schema, the
  order-agnostic streaming build, publish by atomic rename, and a read path whose memory use tracks the query, not the
  corpus. It records the caller's opaque `corpus_token` but never decides freshness or where corpora live. See
  `docs/architecture/05-sqlite-store.md`.
- `runtime`: ships the Lean package to hosts. Hides the payload under `crates/runtime/runtime/` (a copy of the runtime
  subset of `lean/`), its source digest, cache materialization, and the `lake build` it runs under the SDK's shared
  writer. Hosts get the package root, the built module directory, and provenance. A change to a payload file under
  `lean/` must be copied there and the digest recomputed; see `lean/VENDORING.md`.

**The Lean definitions and Rust constants must stay in lockstep.** The command names and versions in
`crates/contract/src/lib.rs` match their Lean mirrors in `Json.lean` and `Canonical.version`, and the JSON shapes the
entry points emit match its DTOs. Changing a command name, payload shape, or version on one side requires updating the
other and the contract doc. Hosts call `Capability.declarationFeatures` and `GoalElaboration.runWithSetup` by name, so
renaming either is a breaking change.

## Boundary rules

These constrain changes more than the compiler does:

- Do not add search semantics to `lean-rs`.
- Do not put downstream workflow policy in shared crates: no review-state fields, report presentation policy, experiment
  knobs, or production gates.
- Do not put transport-specific or project-runtime types in shared search crates.
- Do not expose raw Lean expressions, feature-key encodings, the store's SQLite schema, or cache layout in public APIs
  or crate docs.
- Keep feature keys opaque. Fingerprint strings currently carry a `canonical.expr.v3` prefix, but callers may store and
  compare keys only under matching version fields.
- Prefer concrete private modules over traits until two real implementations exist, as they do for `Corpus`.
- Public DTOs crossing repository boundaries must carry a schema or algorithm version field. The current versions:
  capability schema `lean-semantic-search.capability.v1`, canonical `canonical.expr.v3`, feature rows
  `features.roles.v3`, declaration command `declaration_features.v1`, proof-goal command `proof_goal_features.v1`. Each
  is a constant in `contract` with a Lean mirror. The role-key version `features.role_key.v1` lives only in Lean: it
  prefixes opaque keys, which Rust never parses.
- Command failures stay inside the envelope as structured diagnostics (rows `[]` + an error diagnostic) rather than
  failing the transport—malformed JSON, bad selectors, import failures, and unavailable proof states all return this
  way.
- Proof-goal features are source-backed, computed from elaborated Lean expressions—never from pretty-printed goal text.
- Keep ranking, fanout, and top-k policy private to `retrieval`; callers see only ranks, feature-family explanations,
  and diagnostics. Retrieval takes no storage dependency: persistence enters only through the `Corpus` seam, and only
  `store` knows SQLite. The `Corpus` seam carries no display, provenance, or audit field.

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

## Storage and evidence limits

- Keep no evidence by default. When a result changes a release or maintainer decision, keep one short Git-tracked
  summary: revision, exact check, outcome, and first error needed to act. Do not retain raw logs or traces.
- Before any task writes outputs, name the canonical paths, total byte limit, owner, next named check, and expiry. Keep
  task outputs within this checkout. Do not use `~/Documents/Codex`, `/private/tmp` (including `/tmp`), or `.git` for
  non-Git data. Do not evade limits with alternate paths, aliases, archives, compression, or split files.
- Use installed Elan toolchains and one normal build cache. Do not copy toolchains, targets, or checkouts as evidence.
  If a check cannot meet these limits, stop and report the conflict rather than create another storage location.
- Delete generated task outputs when no next named check owns them, after confirming no live work needs them. Preserve
  active inputs and unique source. Measure allocated bytes before and after cleanup; report retained paths, bytes,
  owners, and expiry. A command succeeding is not proof that cleanup happened.
- Prove Git blob checks with small passing and oversized failing cases, including renamed and partially staged files,
  before claiming enforcement. If the gate is absent or untested, say so. Git checks do not bound ignored or untracked
  output: enforce and measure those limits at the writer. These rules do not authorize a new storage service.
