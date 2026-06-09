# Persistence seam

The `retrieval` crate ranks an anchor against a corpus. Until now the only corpus was an in-memory inverted index built
from rows the caller already held, and `03-retrieval.md` declared persistence "a caller concern, deliberately left
outside." That is right for a small workspace corpus and wrong for a mathlib-scale comparison corpus: materializing
every declaration row and posting list in memory is exactly the cost a persistent index exists to avoid.

This note introduces the seam a persistent store slots into, with **no database in this layer**. The only `Corpus`
implementation here is the existing in-memory inverted index, now expressed behind the trait. The SQLite store arrives
later and implements the same seam; the ranking algorithm, anchor planning, and output shape never learn where rows
live.

## What the persistent layer owns, and what it must not

The persistent layer—this seam plus the later store—owns only the **semantic index** needed to generate candidates. It
never carries the vocabulary a consumer reasons in.

| Concern | Owner |
| --- | --- |
| Opaque-key postings, per-key fanout counts, the document total | Persistent layer (`Corpus`) |
| The feature rows needed to rebuild a corpus member into an anchor | Persistent layer (`Corpus`) |
| Declaration text, module / kind / origin display fields | Consumer |
| Provenance meaning, corpus identity, labels, freshness | Consumer |
| Evidence modes, probe / verification, audit and review policy | Consumer |

The store holds opaque equality tokens and the rows behind them, nothing a caller would render. A `Corpus` answers "who
carries this key, and how many", and "give me this member's feature row"; it is never asked what a key means, where a
corpus came from, or how a candidate should be displayed.

## Why ranking needs no candidate hydration

Ranking walks the anchor's planned keys. For each key it needs three things and no more: the key's **posting list** (the
candidates that carry it), the key's **fanout count** (to judge whether the posting is too broad to expand), and the
corpus **document total** (to weight a match by rarity). Everything else a planned key carries—its feature family, its
base weight, its posting limit, whether it admits a candidate alone—comes from the *anchor's own key*, decided when the
anchor was planned from its low-signal markers. The candidate side contributes only membership.

So a candidate never has to be hydrated to be ranked. A backend can answer fanout from a `COUNT`, postings from an
indexed scan, and rarity from a stored row total, without ever loading a declaration's features. Hydration—turning a
ranked id into something a human reads—is a consumer step, downstream of and outside retrieval.

## The `Corpus` trait

`Corpus` exposes exactly what the ranking loop consumes:

- `document_total()`—the corpus size, for the rarity curve. One number per corpus per query.
- `fanout(keys)`—the match count for each key, returned aligned to the input. **Batched** so a SQL backend answers in
  one query rather than one per key, and so posting and broad-head pruning can judge a key's selectivity *before*
  touching postings.
- `postings(key, limit)`—the declaration ids carrying a key, bounded by the caller's posting limit so a SQL backend can
  `LIMIT` the scan. Called only for keys that survived fanout pruning.
- `declaration_row(declaration_id)`—the feature row of a corpus member, so a member can serve as an anchor (self-audit
  and corpus-versus-corpus). Proof-goal anchors are built from a live row and never call this.

Every method is expressible by both an in-memory map and a per-key SQL query, and the methods are batched wherever a SQL
backend would otherwise issue one query per key.

### Where the trait lives—design it twice

The trait lives in `retrieval`, and the later store depends on `retrieval` to implement it. Four placements were
weighed:

1. **Trait in `retrieval`, store implements it (chosen).** Retrieval is the high-level module; it owns the abstraction
   it needs, and the low-level store depends inward on that abstraction. The ranking loop is written once over
   `&dyn Corpus`. `retrieval` takes **no** storage dependency, and there is **no `retrieval -> store` edge**.
2. **Trait in `contract`.** Avoids an edge either way, but `contract` is the pure cross-repository JSON contract—serde
   DTOs, opaque keys, version constants. A behavioral trait that returns postings is not a DTO and does not belong in it
   (see `00-boundary.md`).
3. **Keep `SemanticIndex` concrete and add a second concrete `PersistentIndex` later.** Copies the ranking loop per
   backend—the one outcome the seam exists to prevent.
4. **Generic `Corpus<Key>`.** There is one key encoding (`OpaqueFeatureKey`). Parameterizing over a key type before a
   second encoding exists is generality without a payer.

Option 1 keeps the algorithm written exactly once and keeps `retrieval` free of any storage dependency. The
store-implements-retrieval edge it introduces is dependency inversion working as intended, not a leak.

## Multi-corpus fan-out

`retrieve_across(corpora, anchor, limit)` ranks one anchor against a slice of corpora and merges the result into one
bounded, ranked candidate list. Single-corpus retrieval is the one-element case. Each corpus weights matches by its own
document total and fanout—rarity is relative to the corpus a key was found in—and accumulation is keyed by
`declaration_id`, so a candidate that appears in more than one corpus merges once, summing its contributions.

Corpus identity stays with the caller. A merged candidate carries only its id, rank, and feature-family explanations;
nothing names which corpus it came from. A caller that fans an anchor across a workspace and a mathlib corpus already
knows which is which and attaches that meaning itself. The seam deliberately has no notion of "mathlib",
"source-backed", or freshness.

Because accumulation is summation and the final order breaks ties on the declaration id, the merged list is
deterministic and independent of the order the corpora are passed in.

## The multi-lane recall guarantee

Ranking accumulates two sub-scores per candidate: a fingerprint/statement score and a role/binder score. A single
combined top-k ranks by their total—and fingerprint families weigh far more than role families (statement 100 down to
conclusion 45, against role constants 18 down to heads 3). So a cohort of candidates sharing a strong fingerprint can
fill the bound and evict a candidate whose only match is a rare, selective role key, even though that role match is
exactly the signal a caller wanted surfaced.

The guarantee bounds the two lanes separately and unions them. A **fingerprint/statement lane** keeps the best `limit`
candidates by fingerprint score; a **role/binder lane** keeps the best `limit` by role score; the result is their union,
ranked by total. A selective role match that the combined order would evict ranks high *within the role lane*, so it
survives. Each lane saturation is reported as a `retrieval.top_k_saturated` diagnostic naming the lane in stable,
key-free terms (`fingerprint_statement`, `role_binder`).

This is a neutral retrieval-quality decision, not duplicate-audit policy: it preserves recall of a signal the crate
already computes and names no downstream workflow. Folding it into shared `policy` is what lets a downstream consumer
stop reimplementing candidate selection on top of raw retrieval output. Because the bounded result a caller observes now
changes, the change moves `RETRIEVAL_POLICY_VERSION` from `lean-semantic-search.retrieval.v1` to
`lean-semantic-search.retrieval.v2`. That constant versions a retrieval decision rather than a Lean fact, so it lives in
`retrieval` and is not mirrored in the contract crate or the Lean package.

## What this note does not add

No SQLite, file, mmap, or storage dependency enters the shared crates here; no on-disk layout is fixed. The `Corpus`
trait carries no display, provenance, or audit field. The candidate output shape—`Candidate`, `MatchExplanation`,
`FeatureFamily`—is unchanged, and no score, weight, posting, or heap crosses the surface. The SQLite implementation of
this seam is the subject of the next note.
