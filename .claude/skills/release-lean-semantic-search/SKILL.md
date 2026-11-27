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
the tag matches the workspace version, extracts the CHANGELOG section, then verifies
`cargo package --workspace --locked`, runs `cargo publish --workspace --locked`, and opens the GitHub Release. NEVER run
`cargo publish` locally to release, NEVER use `--allow-dirty`. Rehearse the whole pipeline without uploading by running
`release.yml` via `workflow_dispatch` with `dry_run: true`.

Every current workspace member is publishable and shares one workspace version. Cargo computes dependency order for
workspace packaging and publishing. If adding a `publish = false` member, explicitly exclude it from package/publish
selection; `cargo package --workspace` also verifies private members. Derive the intended publishable set:

```sh
# members that will publish (everything without `publish = false`)
cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; m=json.load(sys.stdin); print(*(p["name"] for p in m["packages"] if p.get("publish") != []), sep="\n")'
```

A member that must never reach crates.io needs `publish = false` and matching explicit package selection in the gates.

## Steps

Do the reversible prep (1–4) freely. Step 5 (tag push) is irreversible — **stop and get explicit human confirmation
before running it.**

### 1. Pre-flight gate

```sh
mkdir -p target/tmp
TMPDIR="$PWD/target/tmp" ELAN_TOOLCHAIN="$(cat lean/lean-toolchain)" scripts/prerelease.sh
```

Select the installed Lean pin without changing Elan defaults, and keep temporary check outputs inside the checkout. Stop
on any failure. This runs the same gates as CI — `lake build`/`lake test`, the runtime-vendoring check,
`cargo fmt`/`clippy -D warnings`/`nextest`/`deny`, the CHANGELOG consistency check, and (without `--quick`) a
`cargo package --workspace --locked`. This builds all tarballs against Cargo's temporary sibling registry without
uploads or publish credentials. External SDK dependencies must already resolve from crates.io; no local override or
`--no-verify` substitutes for that check. Use `--quick` only for iteration, never as the release gate.

For runtime or SDK changes, also run the ignored runtime build test (see [`crates/runtime/README.md`](../../../crates/runtime/README.md#checks))
against the SDK graph being released. It runs the real `lake build` through the SDK's shared writer, cold and warm.

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

### 4. Contract/version lockstep check (only if a schema, algorithm version, or command name changed)

The Lean definitions and Rust constants must stay in lockstep (see `AGENTS.md`). If this release changed any of them,
confirm both sides moved together, in the commits being released:

- Each version and command-name constant in `crates/contract/src/lib.rs` must equal its mirror in
  `lean/LeanSemanticSearch/Json.lean`. Compare the two files directly. `RETRIEVAL_POLICY_VERSION` in
  `crates/retrieval/src/policy.rs` has no Lean mirror.
- If `Capability.declarationFeatures` or `GoalElaboration.runWithSetup` changed name or signature, the `CHANGELOG.md`
  entry says so: hosts call them by name from their own Lean driver.

A version is opaque to callers and comparable only under matching version fields — a drift here is a contract break, not
a cosmetic one. If they are out of sync, fix before tagging.

### 5. Tag — irreversible

Before tagging, re-verify on the commit that will be tagged:

- `ci.yml` is green on that commit.
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

**Partial publish** — confirm the actual crates.io versions first: an index timeout can follow a successful upload. If
contents are unchanged, do not bump or re-tag. Workspace publishing is not an idempotent retry of existing versions.
Dispatch this workflow at the **same existing release tag**, selecting only the missing workspace crates:

```sh
gh workflow run release.yml --ref vX.Y.Z -f dry_run=false -f 'packages=missing-crate-a missing-crate-b'
```

The workflow re-runs verification and full tarball packaging, validates the tag/version and selection before uploading,
and gives Cargo the missing `--package` arguments; Cargo computes dependency order. Unknown, private and duplicate names
are refused. Leave `packages` empty for a fresh full-workspace release. Rehearsal always packages the full workspace,
regardless of this recovery selection. Publish credentials are supplied only to the live upload step. Manual recovery
does not create a GitHub Release; after confirming all uploads, finish the release through the normal GitHub process.
Publishing remains CI-only; do not run a local upload as a recovery shortcut.

**Contents must change** — a genuine build break, not a propagation race. Bump the patch version, repeat steps 2–5, and
re-tag at the new merge commit; the already-published crates keep their old version.
