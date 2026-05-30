# SQLite store

The persistence seam in `04-persistence.md` left a `Corpus` trait in `retrieval` and one implementation behind it: the
in-memory inverted index, which loads every row and posting list into RAM. That is right for a small workspace corpus
and wrong for a mathlib-scale one, where materializing the whole corpus is the cost a persistent index exists to avoid.

This note is the SQLite implementation of that seam. `lean-semantic-search-store` adds a second `Corpus` — a persisted,
on-disk index — and changes nothing about the ranking algorithm, anchor planning, policy, or output shape. A `Store` is
just another `Corpus`: `retrieve_across` fans one anchor across several of them exactly as before. The store depends on
`retrieval` to implement the trait; nothing depends on the store, and `retrieval` gains no storage dependency.

## What the store owns

The store owns the semantic index only: opaque-key postings, per-key fanout, the document total, and the contract
`DeclarationFeatureRow`s needed to rebuild a corpus member into an anchor. It holds no declaration display text, no
module or kind fields beyond a feature row, no provenance, no labels, no probe cache, and none of the duplicate-audit or
proof-agent vocabulary. Those stay with consumers. A boundary test asserts the on-disk table names, column names, and
metadata keys carry none of that vocabulary.

## Schema

Three tables, each hiding one decision.

```sql
metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL)
-- schema_version, policy_version, corpus_token, total_documents

feature_rows(declaration_id  TEXT PRIMARY KEY,
             feature_version TEXT NOT NULL,
             row_json        TEXT NOT NULL)
-- the whole DeclarationFeatureRow as JSON; serves anchor reconstruction and full rebuild

postings(key TEXT NOT NULL, declaration_id TEXT NOT NULL,
         PRIMARY KEY (key, declaration_id)) WITHOUT ROWID
-- the unified inverted index over opaque keys
```

`postings` is the inverted index; its composite primary key doubles as the lookup index, so a `COUNT` over a key prefix
answers fanout and a range scan answers postings — no secondary index is needed. `feature_rows` stores each row as JSON
so a member can be reconstructed exactly. `metadata` records the four opaque facts the store exposes but never
interprets.

## Design it twice

Four decisions were weighed against `lean-dup`'s index, which is the reference to learn from, not to copy.

1. **One unified `postings` table, not separate fingerprint and role tables.** Fingerprint keys and role keys each carry
   their own version prefix owned by the Lean feature package, so the two key spaces cannot collide; one table with one
   primary-key index answers both fanout and postings. `lean-dup` needs `kind` and `role` columns only because it also
   stores a display string and re-derives an internal handle — neither of which exists here. One table means one index,
   one batched `GROUP BY` for fanout, and a simpler write path.

2. **Key directly by `declaration_id`, not an internal opaque handle.** The declaration id is already opaque and stable
   by contract. `lean-dup` hashes it into a `decl-<digest>` handle to join a wide multi-column declarations table; the
   store has no such table, so keying `postings` and `feature_rows` on `declaration_id` removes a hash, a mapping table,
   and an indirection, and makes anchor reconstruction a single primary-key lookup.

3. **Batched fanout via a chunked `IN (...)`, per-key prepared statement for postings.** `fanout(keys)` is the batched
   call — one `SELECT key, COUNT(*) FROM postings WHERE key IN (?, …) GROUP BY key`, chunked under SQLite's
   bind-parameter ceiling (an anchor plans only dozens of keys, so one chunk in practice), with results mapped back
   aligned to the input and zero for misses. `postings(key, limit)` is per-key by the trait's shape, so it reuses one
   prepared, cached statement. A `carray`/temp-table join was rejected: it needs the array extension for a benefit that
   appears only at key counts a single anchor never reaches.

4. **Store the whole row as JSON, not normalized feature columns.** `declaration_row` must round-trip a
   `DeclarationFeatureRow` exactly, opaque keys preserved. serde JSON guarantees that and keeps the store ignorant of
   the row's internal shape — it never interprets a feature field. Normalizing into columns would couple the store to
   the contract DTO's evolution and re-encode opaque keys for no read-path gain, since postings are already derived into
   their own table at write time.

## The write path

A build streams two kinds of item — a declaration identity announcement and a feature row — that may arrive in any order
or interleaved. A declaration is written only once both halves have arrived, paired by `declaration_id`. The store keeps
only the feature row; the announcement carries no data of its own, but pairing means a feature row for a declaration the
extractor later filtered is never indexed alone, and `total_documents` counts announced-and-featured declarations. The
unpaired halves wait in two small maps and are evicted the moment they pair, so the resident set during a build is
bounded by the number of concurrently unpaired declarations, never by the corpus; SQLite holds the rows already written.

The whole build runs in one transaction against a temp file, reusing two prepared statements (one for `feature_rows`,
one for `postings`). For each paired row the four fingerprints and every role key are inserted once each — deduplicated
into a set exactly as the in-memory index does, so fanout counts match. The build connection sets `journal_mode = OFF`
and `synchronous = OFF`: the store is a build-once cache artifact whose crash safety comes from the publish step, not
the journal, so the build trades durability for throughput. This is deliberately the opposite of the read connection's
flags.

Publish writes the metadata, commits, closes the connection, and renames the temp file into place — atomic on one
filesystem. A builder dropped before publishing removes its temp file, so an interrupted build leaves any prior corpus
at the destination untouched and the new path absent.

## The read path

A `Store` opens read-only — `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`, plus `PRAGMA query_only = true` — and
validates the schema version and reads the four metadata facts once, caching them. So the `Corpus` methods are total:
`document_total` is a field read, and a read error degrades to an empty result rather than surfacing, because the
connection was validated at open. `fanout` is the batched `COUNT … GROUP BY`; `postings` is a per-key indexed scan
bounded by the caller's limit (the never-pruned `usize::MAX` fingerprint limit clamps to a valid SQL bound, which is
safe because the caller only reaches `postings` once fanout already fits the limit); `declaration_row` deserializes one
stored row. None of these hydrate display data — the store holds none.

## The memory property

Retrieving over a persisted corpus keeps the Rust-side resident set proportional to the work of the query — the anchors
planned and the postings touched — never proportional to the corpus. An open `Store` retains only a connection and the
four cached metadata values, which is constant in corpus size; a query allocates the planned keys, the fanout counts,
and the surviving postings, which is proportional to the query. SQLite's own page cache and scratch buffers are
C-allocated and bounded by its fixed cache, so they are corpus-independent too and do not appear in a Rust heap
measurement.

The benchmark builds two synthetic corpora — 100k and 200k declarations — where a fixed-size cohort shares the anchor's
keys and everyone else is unique, so the postings a query touches stay constant as the corpus grows. It asserts the peak
query heap stays under a fixed bound and does not scale with corpus size. Measured: the peak query heap was identical at
both sizes (≈21 KB), query latency was flat (≈4.5 ms), and only build time scaled with the corpus (≈2.6 s at 100k, ≈5.1
s at 200k). The in-memory `Corpus` loads everything, which is right for a small or workspace corpus; the SQLite `Corpus`
is for a large or external one such as mathlib.

## The corpus token

`corpus_token` is an opaque, caller-supplied content identity stored in `metadata` and exposed as a read-only fact,
alongside the store's own `schema_version` and the retrieval `policy_version`. The store records and compares these but
never interprets the token: it folds no Lake, workspace, or toolchain knowledge into it. The freshness and lifecycle
contract — deciding when a stored token means a corpus is stale — is the subject of the next note; this one only writes
and exposes the value.
