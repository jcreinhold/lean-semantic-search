# lean-semantic-search-retrieval

Storage-neutral semantic candidate generation over Lean feature rows.

This crate owns how an anchor's features become a bounded, ranked set of candidate declarations: role and rarity
weighting, broad-head pruning, posting fanout limits, and a multi-lane bounded selection that keeps selective role
matches from being crowded out behind fingerprint cohorts. It consumes the opaque declaration and proof-goal feature
rows from `lean-semantic-search-contract` and returns ranked candidates explained in stable feature-family terms, with
structured diagnostics for pruning and per-lane saturation.

It ranks over a `Corpus` trait—the seam a persistent store later fills—with the in-memory inverted `SemanticIndex` as
the reference implementation: a view over rows the caller already holds, not a database. `retrieve_across` fans one
anchor across a slice of corpora and merges the result into one bounded, ranked list. A proof-goal anchor starts from a
source-backed feature row, never from rendered goal text.

This crate deliberately does not store or persist an index, take a storage dependency, rank for any particular
downstream workflow, expose raw feature keys or composite scores, or carry corpus identity, provenance, or audit policy.
Those are caller concerns. The only keys it touches are the opaque equality tokens it ingests, and its ranking
calibration carries its own `RETRIEVAL_POLICY_VERSION`.

## Use it

```toml
[dependencies]
lean-semantic-search-retrieval = "0.3"
```

```rust
use lean_semantic_search_retrieval::{Anchor, SemanticIndex, retrieve_across};
```

## Native release smoke

The ignored `native_search_and_stale_source_refusal` integration test runs a source-backed goal against all eligible
`Init.Data.Nat.Basic` declarations and requires `Nat.add_comm` to rank first. It then closes the worker, changes only an
owned source copy, and requires the next open to refuse that source with no remaining worker processes. It defines no
native authority, builds no worker or Lean artifacts, and changes no installed service or toolchain.

Supply a matching application-owned semantic worker and real SDK-prepared artifacts in a fresh disposable directory
below this checkout's `target/native-smoke/`. The worker must own the semantic operation table, verify these inputs, and
identify the changed source in its refusal. A default authority-less worker is not suitable. Keep every capability and
shim artifact below that owned directory. Set `LEAN_SEMANTIC_SEARCH_NATIVE_SMOKE_INPUTS` to a canonical JSON file inside
it, containing:

| Field | Input |
| --- | --- |
| `root` | Canonical disposable run directory. |
| `worker` | Explicit canonical path to the matched application-owned worker. |
| `sysroot` | Explicit canonical installed Elan Lean prefix. |
| `manifest` | Actual built semantic capability manifest below `root`. |
| `stale_source` | Disposable materialized Lean source below `root`. |
| `host_artifacts` | Actual SDK `LeanWorkerPreparedHostArtifacts` descriptor for owned shim artifacts. |

Set process-local `LEAN_SYSROOT` to the same prefix, `TMPDIR` to `<root>/tmp`, and `LEAN_RS_WORKER_OWNERS_DIRECTORY` to
`<root>/owners`; create those private directories before running. Do not set native loader overrides. Use the normal
build cache and an allocated two-job native lane:

```sh
CARGO_BUILD_JOBS=2 cargo test -p lean-semantic-search-retrieval --test native_search --locked -- --ignored --nocapture
```

Success removes the disposable input directory only after confirmed closure. An operation or cleanup failure retains it
and reports the original errors separately; inspect ownership before cleanup or retry. No raw logs or separate receipts
are required. Cold/warm materialization belongs to the existing runtime build gate. This smoke qualifies the actual
supplied graph only: publication requires the released SDK patch, matching worker inputs, and real registry locks
without source overrides.

## See also

- [Project README](https://github.com/jcreinhold/lean-semantic-search/blob/main/README.md)
- [Boundary note](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/00-boundary.md)
- [Retrieval boundary](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/03-retrieval.md)
- [Persistence seam](https://github.com/jcreinhold/lean-semantic-search/blob/main/docs/architecture/04-persistence.md)
- [DTO contract crate](https://github.com/jcreinhold/lean-semantic-search/blob/main/crates/contract/README.md)

## License

Licensed under MIT or Apache-2.0, at your option.
