# Cache lifecycle

The SQLite store in `05-sqlite-store.md` writes a corpus and records four opaque facts about it—a caller-supplied
`corpus_token`, the store's own `schema_version`, the retrieval `policy_version`, and `total_documents`. This note
decides what those recorded values *mean* for reuse, and hardens the store against partial writes, version drift, and
multiple readers. It is the difference between a file that happens to hold an index and a cache a long-lived tool can
trust across runs, toolchains, and processes.

Nothing here teaches the store what the token is made of. It compares an opaque string and the two versions it owns; the
freshness decision stays neutral.

## The freshness contract

A persisted corpus is reusable for a request **iff** its stored `corpus_token` equals the caller's supplied token *and*
its stored `schema_version` and `policy_version` equal the running ones. Any mismatch is a cache miss, not an error: the
caller rebuilds. `Store::open_fresh` returns the opened corpus on a full match and a structured [`CacheMiss`] otherwise
— `TokenMismatch`, `SchemaDrift`, `PolicyDrift`, `Missing`, or `Corrupt`. It never errors and never panics, so the
caller's whole vocabulary for "cannot reuse" is one rebuild path.

This is "define errors out of existence" (Ousterhout, _A Philosophy of Software Design_, ch. 10) applied to a cache. A
version mismatch is not exceptional—it is the expected signal after a toolchain bump or a policy change—so it is a value
the normal path returns, not an exception the caller must catch. Corruption joins it: a file that fails to open or whose
required metadata cannot be read is `Corrupt`, the same cache miss, never a hard failure that strands the caller
mid-request.

## Who owns the cache key

The cache key has two halves, owned by two layers.

| Question | Owner | Folded into |
| --- | --- | --- |
| "What makes *my* corpus stale?" | the caller | the opaque `corpus_token` |
| "Is this artifact compatible with the running code?" | the store | `schema_version` + `policy_version` |

The caller's ingredients—Lake files, source digests, toolchain identity, include policy, selected roots—are real, but
they are *the caller's*. It hashes them into one opaque token and hands the store the digest, never the ingredients. The
store compares tokens for equality and never inspects their contents. The store's own halves are different in kind:
`schema_version` tracks its on-disk layout, `policy_version` tracks the retrieval ranking the corpus was built for.
Those move when the running code changes, independent of any corpus content.

Mixing the two halves would be the leak. If the store learned that a token is "a Lake digest plus a toolchain string,"
it would carry consumer vocabulary the boundary forbids—and every consumer with a different notion of staleness
(`lean-dup`'s audited universe, a proof agent's pinned mathlib) would have to push its ingredients down into a layer
that exists precisely to not know them. Keeping the token opaque lets one store serve every consumer's freshness rule
without learning any of them.

## Build, publish, and the latest pointer

A corpus lives in a content-addressed directory under a cache root; its index file sits inside at a fixed, private name.
The caller names the directory (the content address is its concern, derived from its token); the store names the file.
The neutral primitives in `lifecycle.rs` know only directories and a pointer:

- `index_path(root, name)`—where a build writes and a reader opens.
- `set_latest(root, name)`—publish `name` as the active corpus.
- `latest_index_path(root)`—resolve the active corpus, ready to open.
- `cleanup(root, keep, mode)`—remove what the caller no longer wants.

Two publishes are atomic, and both for the same reason—a rename on one filesystem is all-or-nothing. The index file
itself is published by `StoreBuilder` renaming its temp build into place (see `05-sqlite-store.md`). The latest pointer
is published by `set_latest` writing the bare directory name to a temp file beside the pointer and renaming it over the
old one. A reader resolving the pointer therefore reads either the complete old name or the complete new one, never a
torn or empty file.

The ordering is the safety property. A rebuild **publishes before it cleans**: it builds the new corpus into a fresh
directory, flips the pointer to it, and only then asks `cleanup` to remove the old one. Because `cleanup` always
protects the pointer's current target, the old directory is removable only *after* the pointer no longer names it. A
concurrent reader resolving the pointer at any instant lands on a directory that still exists—the old one before the
flip, the new one after. It never observes a missing index.

The pointer is a separate file under the root, not a row in the index's `metadata` table. Recording it in the database
would both reopen the index for a write and grow its metadata past the four facts the boundary test pins. A sibling file
keeps the index immutable once published and its metadata exactly its four opaque facts.

## Corruption recovery

`open_fresh` detects corruption cheaply: the file opens and its four required metadata facts read, or it is `Corrupt`. A
truncated or garbage file fails one of those steps and becomes a cache miss; the caller rebuilds and the fresh corpus
opens. There is deliberately no `PRAGMA integrity_check` on the open path. A full integrity scan walks the whole
database—work proportional to the corpus—which would reintroduce exactly the cost the query-bounded resident set exists
to avoid (below). The structural check is constant in corpus size and catches the corruption that actually strands a
reader: a file that cannot yield its metadata.

## Concurrency

Multiple readers over one published corpus are safe by construction. Each `Store` opens its own read-only,
single-threaded connection (`SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`, `PRAGMA query_only`), and a published corpus
is never written again—a rebuild writes a *different* directory and a *different* temp file. So readers and a concurrent
build touch disjoint paths: nothing contends, nothing locks, and no reader can observe a half-written index because
half-written indices live only at a temp path no pointer names.

## Cleanup as a neutral primitive

`cleanup` is the caller's tool, not the store's policy. Given the directories the caller still wants, it keeps those,
keeps the active pointer's target, and reports the rest as removable—with each directory's bytes—so the caller inspects
before deleting. It defaults to reporting: `CleanupMode::DryRun` computes the plan and touches nothing;
`CleanupMode::Execute` removes the removable directories. There is no automatic, time- or size-driven eviction; the
store provides the "keep this set, remove the rest, protect the latest" mechanism and the caller drives it.

## The persistence gate

The query-bounded resident set from `05-sqlite-store.md` is a standing gate. A query over a persisted corpus must keep
its Rust-side peak heap under a fixed bound and not scale with corpus size; the lifecycle additions add nothing to the
query path, so they must not move it. `tests/benchmark.rs` is the evidence, asserting peak query heap `< 4 MB` and a
200k/100k heap ratio `< 1.5`.

Reproduce with `cargo test -p lean-semantic-search-store --test benchmark --release -- --ignored --nocapture`:

| Metric | 100k declarations | 200k declarations |
| --- | --- | --- |
| Peak query heap | 21,532 bytes | 21,532 bytes |
| Query latency | ≈3.0 ms | ≈3.0 ms |
| Build time | ≈0.4 s | ≈0.9 s |

The peak query heap is identical to the byte across a doubled corpus (ratio 1.00) and four orders of magnitude under the
4 MB bound; query latency is flat and only build time scales, as it must. Absolute latency and build time are
machine-dependent—the invariants the gate fixes are the flat, bounded heap and the flat query latency. The lifecycle
additions touch no query-path code, and the gate holds.

## Design it twice

Three decisions were weighed; each chose the option that keeps the store ignorant of token contents and turns drift into
a cache miss.

1. **Store compares an opaque token + its own versions, not freshness end-to-end.** Owning freshness end-to-end would
   force the store to learn the token's ingredients—the leak above. Comparing an opaque string plus the two versions it
   already owns keeps the boundary intact and still answers every reuse question.

2. **Cleanup is a neutral caller-driven primitive, not store-owned eviction.** A store that evicted on its own age or
   size policy would encode a retention decision no shared layer should make. The neutral "keep this set, remove the
   rest, protect the latest" primitive lets each consumer keep what its workflow needs.

3. **Version mismatch and corruption are cache misses, not hard errors.** Failing hard would push a recovery decision
   onto every caller for a condition with one sensible response—rebuild. Folding both into `CacheMiss` gives the caller
   a single path and removes a whole class of error handling.

See `04-persistence.md` for the `Corpus` seam and `05-sqlite-store.md` for the schema and the read/write design.
