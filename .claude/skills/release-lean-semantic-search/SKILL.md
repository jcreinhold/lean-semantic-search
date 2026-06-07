---
name: release-lean-semantic-search
description: Cut a lean-semantic-search release (run the pre-release gate, bump the workspace version, update the CHANGELOG, push a signed tag) that publishes the workspace to crates.io via CI. Use when releasing lean-semantic-search, publishing the crates, bumping the workspace version for a release, or cutting a vX.Y.Z tag.
---

# Release lean-semantic-search

[`scripts/prerelease.sh`](../../../scripts/prerelease.sh) and
[`.github/workflows/release.yml`](../../../.github/workflows/release.yml) are the source of truth — follow them. This
skill is the checklist plus the cross-file invariants CI only catches *after* you tag, when it is too late: crates.io
versions are **immutable**, so a botched publish burns a version permanently.

**Publishing happens only in CI.** Pushing a `vX.Y.Z` git tag fires `release.yml`, which re-runs the full gate, asserts
the tag matches the workspace version, extracts the CHANGELOG section, then runs `cargo publish --workspace --locked`
and opens the GitHub Release. NEVER run `cargo publish` locally to release, NEVER use `--allow-dirty`. Rehearse the whole
pipeline without uploading by running `release.yml` via `workflow_dispatch` with `dry_run: true`.

All four crates publish together — `lean-semantic-search-contract`, `-capability`, `-retrieval`, `-store` — and share one
workspace version.

## Steps

Do the reversible prep (1–4) freely. Step 5 (tag push) is irreversible — **stop and get explicit human confirmation
before running it.**

### 1. Pre-flight gate

```sh
scripts/prerelease.sh            # mirrors release.yml's verify job + the publish preflight; --quick skips the publish dry-run
```

Stop on any failure. This runs the same gates as CI — `lake build`/`lake test`, the runtime-vendoring check,
`cargo fmt`/`clippy -D warnings`/`nextest`/`deny`, the CHANGELOG consistency check, and (without `--quick`) a
`cargo publish --workspace --dry-run`. Use `--quick` only for fast iteration, never as the actual release gate.

### 2. Version bump (one source of truth, three places)

Pick the new `X.Y.Z` (patch unless the change is breaking/feature — it is pre-1.0, so breaking changes bump the minor).
The workspace version is the single source of truth; three places must agree:

- root `Cargo.toml`: `[workspace.package].version = "X.Y.Z"` (all four crates inherit it via `version.workspace = true`).
- root `Cargo.toml` `[workspace.dependencies]`: the two **version-pinned internal** path deps —
  `lean-semantic-search-contract` and `lean-semantic-search-retrieval` — each carry a `version = "X.Y.Z"` that must move
  in lockstep, or `cargo publish` resolves the wrong version. (`-capability` and `-store` are path-only, nothing to edit.)
- `lean/lakefile.lean`: `version := v!"X.Y.Z"`. CI does not check this, but the project keeps the Lean package and the
  Rust workspace on one unified version — bump it in the same commit.

Run `cargo build` so `Cargo.lock` updates. The publish job asserts `"v${workspace version}" == "${tag}"` (read from the
`lean-semantic-search-contract` crate) before any upload, so a half-updated version fails the run.

### 3. CHANGELOG

Move the `## [Unreleased]` entries in `CHANGELOG.md` into a new `## [X.Y.Z]` section (compose fresh if empty), and leave a
fresh empty `## [Unreleased]` heading on top. The heading text must match the tag **exactly** and carries no date — tag
`v0.2.0` → heading `## [0.2.0]` (match the existing `## [0.1.0]`). The workflow extracts that section verbatim (awk on
`^## \[X.Y.Z\]`) as the GitHub Release body and fails if it is missing or empty; `prerelease.sh` runs the identical
check locally.

### 4. Contract/version lockstep check (only if a schema or algorithm version changed)

The Lean exports and Rust constants must stay in lockstep (see `CLAUDE.md`). If this release changed any schema or
algorithm version, confirm both sides moved together, in the commits being released:

- Rust version constants in `crates/contract/src/lib.rs` (`CAPABILITY_SCHEMA_VERSION` = `lean-semantic-search.capability.v1`,
  `CANONICAL_FEATURE_VERSION` = `canonical.expr.v3`, `SEMANTIC_FEATURE_VERSION` = `features.roles.v3`,
  `DECLARATION_FEATURE_COMMAND_VERSION` = `declaration_features.v1`, `PROOF_GOAL_FEATURE_COMMAND_VERSION` =
  `proof_goal_features.v1`) and `RETRIEVAL_POLICY_VERSION` in `crates/retrieval/src/policy.rs`
  (`lean-semantic-search.retrieval.v2`) ↔ their mirrors in `lean/LeanSemanticSearch/Json.lean` (those plus `roleKeyVersion`
  = `features.role_key.v1`).
- the five `@[export lean_semantic_search_*]` names in `lean/LeanSemanticSearch/Capability.lean` still match the
  `*_EXPORT`/`*_COMMAND` constants in `crates/capability/src/lib.rs`.

A version is opaque to callers and comparable only under matching version fields — a drift here is a contract break, not
a cosmetic one. If they are out of sync, fix before tagging.

### 5. PR, merge, then tag — irreversible

Open a PR with the version + CHANGELOG (+ any version-constant) changes; merge after `ci.yml` is green. Before tagging,
re-verify on the merge commit:

- `git rev-parse --abbrev-ref HEAD` is `main` and up to date with `origin/main`.
- `[workspace.package].version`, the two pinned `[workspace.dependencies]` versions, and `lean/lakefile.lean` all equal
  the intended `X.Y.Z`.
- `CHANGELOG.md` has a `## [X.Y.Z]` heading.

**Confirm with the human, then push the signed tag** (this is the irreversible step):

```sh
git tag -s vX.Y.Z -m "lean-semantic-search vX.Y.Z"   # -s signed (preferred), or -a unsigned annotated
git push origin vX.Y.Z
gh run watch --workflow=release.yml
```

Tags containing `-` (e.g. `vX.Y.Z-rc1`) are auto-marked prerelease and are not made `latest`.

### 6. Post-publish

- `cargo search lean-semantic-search-contract` (and `-capability`, `-retrieval`, `-store`) show the new version.
- Confirm the GitHub Release body matches the `## [X.Y.Z]` CHANGELOG section.
- Within ~10 min, confirm `https://docs.rs/lean-semantic-search-contract/X.Y.Z` (and the other three) built; a docs.rs
  failure is recoverable only by a patch publish with the fix.
- Confirm the fresh `## [Unreleased]` heading is in place at the top of `CHANGELOG.md`.

## When publish fails mid-run

crates.io versions are immutable, so the fix depends on *why* it failed. This repo has no dedicated recovery workflow.

**Partial publish** — some crates uploaded, the rest did not (often the tail crate losing cargo's index-propagation
race). The crate *contents* are fine; only the upload is incomplete. Do **not** bump the version and do **not** re-tag —
a re-run of `cargo publish --workspace` rejects the already-published crates with `already exists`. Publish only the
missing crates by hand, in dependency order, once the index has settled:

```sh
cargo publish -p lean-semantic-search-<crate> --locked   # order: contract → capability, retrieval → store
```

**Contents must change** — a genuine build break, not a propagation race. Bump the patch version, repeat steps 2–5, and
re-tag at the new merge commit; the already-published crates keep their old version.
