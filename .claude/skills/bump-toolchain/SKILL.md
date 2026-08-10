---
name: bump-toolchain
description: Bump lean-semantic-search's pinned Lean toolchain (and, when it rides along, the lean-rs crate line it builds against). Use whenever the user wants to move lean-semantic-search to a newer Lean release, adopt a new lean-rs version, update the `lean/lean-toolchain` pin, re-sync the vendored runtime payload after a toolchain change, or extend toolchain support — even if they only say "bump the toolchain" without naming the coupled dependency work.
---

# Bump lean-semantic-search's pinned Lean toolchain

lean-semantic-search is the **upstream** shared package, not a downstream consumer. It does not follow anyone else's
toolchain — it *chooses* one, and downstream hosts (`lean-dup`, `lean-host-mcp`) follow lean-semantic-search. So the
target toolchain here is a relatively free choice: any real `leanprover/lean4` release the Lean package compiles under
and the test suites pass on. The constraint is downstream-facing, not upstream-facing: pick a toolchain a published
`lean-rs` release actually supports, so consumers can build the worker against it.

This is a different job from a downstream pin. Three things make it different, and getting them wrong is where the time
goes:

- **There is one authoritative pin: `lean/lean-toolchain`.** No root pin, no per-fixture pins. CI and the release
  workflow read this single file (`--default-toolchain "$(cat lean/lean-toolchain)"`).
- **The Lean package has no git `require`s.** `lean/lakefile.lean` depends on nothing upstream, so a toolchain bump
  never moves a Lake dependency tag. The only versioned coupling is on the Rust side: the `lean-rs` crate family.
- **lean-rs is a *separate, co-timed* dependency, not a forced one.** This repo's Rust crates use `lean-rs-worker-protocol`
  and the `lean-toolchain` crate (protocol/metadata, not the ABI host); no crate embeds `libleanshared` or has a
  `build.rs`. So a Lean-toolchain bump does **not** force a lean-rs bump. They often move together (as v0.3.1 did), but
  treat the lean-rs bump as an independent step you do only when adopting a new lean-rs release.

## Before you start: identify the target

You need at most two facts, and only the first is required:

1. The target **Lean toolchain** (e.g. `leanprover/lean4:v4.32.0`). Confirm a published `lean-rs` release supports it,
   so downstream can consume the toolchain you pin.
2. *(Optional)* The **`lean-rs` line** to adopt (e.g. `0.3` → `0.4`) — only if this bump also moves lean-rs. The repos
   are `github.com/jcreinhold/lean-rs` and `github.com/jcreinhold/lean-semantic-search`.

A pure point-release toolchain bump is common and clean when the package still compiles unchanged (e.g. the documented
header-identical `v4.31.0-rc1` → `-rc2` move). When the new toolchain forces *Lean source* edits to compile, you have
extra work: the vendored runtime payload and its digest move too (see step 5).

## The ritual

### 1. Install the toolchain

```sh
elan toolchain install leanprover/lean4:vX.Y.Z
```

~500 MB; skip if already installed.

### 2. Move the single toolchain pin

```sh
echo 'leanprover/lean4:vX.Y.Z' > lean/lean-toolchain
```

That is the whole pin. Confirm there is exactly one and nothing else crept in:

```sh
find . -name lean-toolchain -not -path '*/.lake/*' -not -path '*/target/*'
```

Expect only `lean/lean-toolchain`. The vendored runtime copy under `crates/runtime/runtime/` intentionally has **no**
`lean-toolchain` (downstream hosts generate their own; `check-runtime-vendoring.sh` fails if one appears there) — do not
add one.

### 3. Bump lean-rs only if this bump adopts a new lean-rs line (skip for a toolchain-only bump)

Two coordinated places:

- **Workspace deps** — in the root `Cargo.toml` `[workspace.dependencies]`, bump `lean-rs-worker-protocol` and
  `lean-toolchain` to the new requirement (prefer major-only for a stable major, per the dependency policy).
- **`deny.toml` floor** — the `[bans] deny` list version-floors the whole `lean-rs` family (`lean-rs`, `lean-rs-sys`,
  `lean-rs-abi`, `lean-rs-host`, `lean-rs-interop-shims`, `lean-rs-worker-{protocol,parent,child}`, `lean-toolchain`).
  Raise the floor to the new major and update the explanatory comment. The floor exists because lean-rs is `0.x`, so
  `0.N.x` and `0.(N+1).x` cannot coexist in one graph — a stale floor lets a pre-bump copy sneak back in as a deep
  `E0308` instead of a named cargo-deny diagnostic.

Then refresh the lock: `cargo update -p lean-rs-worker-protocol -p lean-toolchain` (or rely on `cargo build` to resolve),
and verify with `cargo deny check`.

### 4. Update the Rust floor only if lean-rs raised it

The `lean-rs` crates carry a `rust-version`. Adopting a new line can raise this repo's floor. If so, bump `rust-version`
in `Cargo.toml` `[workspace.package]` and update the matching prose: `AGENTS.md` ("Rust stable with `rust-version =
…`") and `README.md` ("Requirements"). If lean-rs didn't move it, leave it alone — there is no Lean-version string to
update in that prose (AGENTS.md/README pin only Rust and say "a Lean 4 toolchain visible to `lake`", deliberately
version-agnostic).

### 5. Rebuild, test, and re-sync the vendored runtime

Build the Lean package and run its driver first (the Rust runtime tests shell out to `lake`), then the Rust suite:

```sh
lake -d lean build                  # canonical Lean package under the new toolchain
lake -d lean test                   # the `tests` exe (Main root)
cargo test                          # workspace suite (nextest: 2 threads, no fail-fast)
```

If the new toolchain forced **any edit to the canonical Lean source** (`lean/LeanSemanticSearch/**`,
`lean/lakefile.lean`, `lean/lake-manifest.json`, `lean/LeanSemanticSearch.lean`), the vendored copy and its digest move
in lockstep:

- Re-sync the payload into `crates/runtime/runtime/` (mirror exactly the file set in `lean/VENDORING.md`; the runtime
  copy excludes `lean-toolchain`, `Main.lean`, `LeanSemanticSearchTest*`, and build artifacts).
- Recompute the digest with the command in `lean/VENDORING.md`, then update **both** the `RUNTIME_SOURCE_DIGEST` const
  in `crates/runtime/src/lib.rs` and the `runtime_source_digest` value in `lean/VENDORING.md` (and its synced copy).
- `crates/runtime/src/lib.rs`'s `runtime_source_digest_matches_recorded_value` test is the canary — a digest you forgot
  to update fails there.

Prove the vendored payload matches the canonical source and still builds (this is what CI runs):

```sh
scripts/check-runtime-vendoring.sh
```

Then the lint and policy gates, since CI treats warnings as errors:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo deny check
```

A toolchain-only bump (no Lean source change) does **not** move `runtime_source_digest` — the digest excludes
`lean-toolchain`, `README.md`, and `VENDORING.md` by design. Don't "refresh" it to paper over an unrelated diff.

### 6. Refresh the cosmetic toolchain literals (consistency only — these are not pins)

The runtime crate is toolchain-agnostic: the caller supplies `toolchain_label`. The in-repo occurrences of the old
toolchain string are test fixtures and a doc example, not authority. Update them so they reflect the current toolchain
and don't mislead the next reader:

- `crates/runtime/src/lib.rs` — the `toolchain_label` literals in `#[cfg(test)] mod tests`.
- `crates/runtime/tests/build_cached.rs` — the `unwrap_or_else` default for `LEAN_SEMANTIC_SEARCH_RUNTIME_TOOLCHAIN`.
- `crates/runtime/README.md` — the example `toolchain_label`.

Grep the old version to catch any stragglers, and **leave historical records alone** — dated measurements
(`lean/VENDORING.md` "Recorded …"), the `LeanSemanticSearchTest.lean` golden-capture comment, and past `CHANGELOG.md`
entries record real past runs and must not be rewritten:

```sh
grep -rIn 'v4\.OLD\.VERSION' --exclude-dir=.lake --exclude-dir=target --exclude=Cargo.lock --exclude=CHANGELOG.md .
```

### 7. Record it in the CHANGELOG

Add bullets under `## [Unreleased]` in `CHANGELOG.md`, modeled on the v0.3.1 entry:

- A `### Changed` bullet naming the new toolchain pin and, if applicable, the lean-rs line adopted. If a Lean source
  edit moved the digest, note `(`runtime_source_digest` updated for the source edit.)` as v0.3.1 does.
- If you raised the `deny.toml` floor, an `### Internal` bullet explaining the new floor (mirror the v0.4.0 entry).

Leave the actual version bump and tag to the release: the `release-lean-semantic-search` skill cuts the release from
`[Unreleased]`. Don't bump the workspace `version` or the lakefile `version` here.

### 8. Commit

Use the repo's commit style, e.g. `Bump Lean toolchain to vX.Y.Z` (add `and lean-rs
deps` if step 3 applied). Summarize in the body: the new toolchain, any lean-rs line and Rust-floor change, whether the
runtime digest moved, and the test result.

## When it fails

| Symptom | Cause | Action |
| --- | --- | --- |
| `lake -d lean build` fails to compile under the new toolchain | A genuine Lean behavior/API change in the new release | Fix the Lean source minimally; remember the edit triggers the step-5 vendoring re-sync + digest recompute. |
| `runtime_source_digest_matches_recorded_value` test fails | Lean source changed but `RUNTIME_SOURCE_DIGEST` / `VENDORING.md` weren't recomputed | Recompute the digest (step 5) and update both the const and the vendoring note. |
| `check-runtime-vendoring.sh` reports the payload differs | The vendored copy under `crates/runtime/runtime/` drifted from `lean/` | Re-sync the file set (step 5); never edit only one side. |
| `cargo deny check` fails with a pre-floor lean-rs version | A transitive pin dragged an old lean-rs copy in, or the floor wasn't raised | Reconcile the workspace dep with the new line and raise the `deny.toml` floor (step 3). |
| A Rust test fails only on the new toolchain | Likely an upstream behavior change in Lean or lean-rs | Reproduce minimally; if it's an upstream regression, raise it with the lean4 / lean-rs maintainers rather than pinning around it here. |
