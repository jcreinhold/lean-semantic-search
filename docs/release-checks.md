# Release package checks

Use `cargo package --workspace --locked` for the full verified archive gate. Cargo stages sibling tarballs in a
temporary registry and builds from them before any upload. Do not use `--no-verify`, local SDK overrides or manufactured
registry locks. This workspace currently has five publishable members; private members require explicit
exclusion/selection. Live publishing remains `cargo publish --workspace --locked`, with the token confined to that step.
For unchanged partial uploads, dispatch the same release tag with only missing `packages`; Cargo orders the selected
dependencies. The workflow validates exact publishable member names and shares one release/recovery concurrency group.

## Checked on 2026-11-20

Semantic base: `a5203e6db2c02395533c1917dded042c77e6d1bf`. Installed Cargo 1.100.0, tiny disposable two-crate workspace:

- Verified `cargo package --workspace --locked --offline`: passed for unpublished leaf and dependent, in that order.
- Workspace `cargo publish --dry-run --locked`: passed too. Semantic's existing workspace dry run already handled
  unpublished siblings; the reported SDK failure was a per-crate dry run, which the fixture also rejected.
- `publish = false` member: workspace packaging tried to build it and failed; explicit publishable selection passed.
- Missing external fixture SDK: packaging refused with `no matching package named ... found`.

No upload or broad/native Lean build ran. Raw logs were not retained. Fixture/cache cleanup measured 1,073,152 allocated
bytes before and zero after; its path no longer exists. An initial offline publish comparison refused an HTTP query;
workspace dry publishing was then checked online. Mechanics do not qualify semantic's pending R1 source or SDK graph.
Real unpatched locked packaging, full release gates and publication remain separate checks after R0/R1 are ready.

Workflow checks: actionlint, shellcheck, shfmt, Markdown and 11 mocked recovery/tag cases passed. No Git-blob
enforcement was added or tested.
