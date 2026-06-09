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
and opens the GitHub Release. NEVER run `cargo publish` locally to release, NEVER use `--allow-dirty`. Rehearse the
whole pipeline without uploading by running `release.yml` via `workflow_dispatch` with `dry_run: true`.

Every publishable workspace member publishes together and shares one workspace version. The publishable set is every
member under `[workspace]` whose package does not set `publish = false`; `cargo publish --workspace` publishes exactly
that set, in the dependency order cargo computes. Do not assume a count or a fixed list — derive it:

```sh
# members that will publish (everything without `publish = false`)
cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; m=json.load(sys.stdin); print(*(p["name"] for p in m["packages"] if p.get("publish") != []), sep="\n")'
```

If a member must never reach crates.io, give it `publish = false` so the gate's dry-run excludes it; otherwise it ships.

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

### 2. Version bump (the workspace version is the one source of truth)

Pick the new `X.Y.Z` (patch unless the change is breaking or adds a feature — pre-1.0, so both bump the minor). The
workspace version is the single source of truth; every place that restates it must move in lockstep:

- root `Cargo.toml` `[workspace.package].version` — every member inherits it via `version.workspace = true`.
- root `Cargo.toml` `[workspace.dependencies]` — each **internal path dep that also carries a `version`** must move to
  the new version, or `cargo publish` resolves a dependent against the old published crate. Path-only internal deps have
  nothing to bump. List the pinned ones rather than assuming which they are:

  ```sh
  grep -nE 'path = "crates/[^"]+", version =' Cargo.toml
  ```

- `lean/lakefile.lean`: `version := v!"X.Y.Z"`. CI does not check this, but the project keeps the Lean package and the
  Rust workspace on one unified version — bump it in the same commit.

Run `cargo build` so `Cargo.lock` updates. The gate reads the workspace version from one member (`prerelease.sh` and
`release.yml` anchor on `lean-semantic-search-contract`, but since all members share `[workspace.package].version` any
would do) and the publish job asserts `"v${version}" == "${tag}"` before any upload, so a half-updated bump fails the
run.

### 3. CHANGELOG

Move the `## [Unreleased]` entries in `CHANGELOG.md` into a new `## [X.Y.Z]` section (compose fresh if empty), and leave
a fresh empty `## [Unreleased]` heading on top. The heading text must match the tag **exactly** and carries no date —
tag `v0.2.0` → heading `## [0.2.0]` (match the existing `## [0.1.0]`). The workflow extracts that section verbatim (awk
on `^## \[X.Y.Z\]`) as the GitHub Release body and fails if it is missing or empty; `prerelease.sh` runs the identical
check locally.

### 4. Contract/version lockstep check (only if a schema or algorithm version changed)

The Lean exports and Rust constants must stay in lockstep (see `CLAUDE.md`). If this release changed any schema or
algorithm version, confirm both sides moved together, in the commits being released:

- Each version constant in `crates/contract/src/lib.rs`, plus `RETRIEVAL_POLICY_VERSION` in
  `crates/retrieval/src/policy.rs`, must equal its mirror in `lean/LeanSemanticSearch/Json.lean`. `CLAUDE.md` lists the
  constants and their current values; for any this release touched, compare the Rust and Lean sides directly rather than
  trusting that list to be current.
- Every `@[export lean_semantic_search_*]` in `lean/LeanSemanticSearch/Capability.lean` still matches a
  `*_EXPORT`/`*_COMMAND` constant in `crates/capability/src/lib.rs`.

A version is opaque to callers and comparable only under matching version fields — a drift here is a contract break, not
a cosmetic one. If they are out of sync, fix before tagging.

### 5. PR, merge, then tag — irreversible

Open a PR with the version + CHANGELOG (+ any version-constant) changes; merge after `ci.yml` is green. Before tagging,
re-verify on the merge commit:

- `git rev-parse --abbrev-ref HEAD` is `main` and up to date with `origin/main`.
- `[workspace.package].version`, every pinned `[workspace.dependencies]` version, and `lean/lakefile.lean` all equal the
  intended `X.Y.Z`.
- `CHANGELOG.md` has a `## [X.Y.Z]` heading.

**Confirm with the human, then push the signed tag** (this is the irreversible step):

```sh
git tag -s vX.Y.Z -m "lean-semantic-search vX.Y.Z"   # -s signed (preferred), or -a unsigned annotated
git push origin vX.Y.Z
gh run watch --workflow=release.yml
```

Tags containing `-` (e.g. `vX.Y.Z-rc1`) are auto-marked prerelease and are not made `latest`.

### 6. Post-publish

- `cargo search` shows the new version for each crate in the publish set.
- Confirm the GitHub Release body matches the `## [X.Y.Z]` CHANGELOG section.
- Within ~10 min, confirm `https://docs.rs/<crate>/X.Y.Z` built for each published crate; a docs.rs failure is
  recoverable only by a patch publish with the fix.
- Confirm the fresh `## [Unreleased]` heading is in place at the top of `CHANGELOG.md`.

## When publish fails mid-run

crates.io versions are immutable, so the fix depends on *why* it failed. This repo has no dedicated recovery workflow.

**Partial publish** — some crates uploaded, the rest did not (often the tail crate losing cargo's index-propagation
race). The crate *contents* are fine; only the upload is incomplete. Do **not** bump the version and do **not** re-tag —
a re-run of `cargo publish --workspace` rejects the already-published crates with `already exists`. Publish only the
missing crates by hand, in dependency order, once the index has settled:

```sh
cargo publish -p <crate> --locked   # one per missing crate, in dependency order
```

`cargo publish --workspace --dry-run` prints the order cargo uses — a crate publishes only after the crates it depends
on. Follow that order for the missing ones.

**Contents must change** — a genuine build break, not a propagation race. Bump the patch version, repeat steps 2–5, and
re-tag at the new merge commit; the already-published crates keep their old version.
