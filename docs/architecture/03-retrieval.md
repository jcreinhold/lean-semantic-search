# Retrieval

The `retrieval` crate owns storage-neutral semantic candidate generation. It consumes the opaque feature rows the Lean
package emits and returns ranked candidates. It does not store anything, rank for any particular workflow, or know how a
caller obtained its rows.

## Why a crate, and why this one

`retrieval` exists because there is finally a decision to hide: how an anchor's features become a bounded, ranked set of
candidates. That decision has real internal machinery—role and rarity weighting, broad-head pruning, posting fanout
limits, a bounded top-k heap—and exactly one narrow job at its surface: build an index, build an anchor, retrieve. That
is a deep module.

A `core` crate was the obvious alternative and the wrong one. `core` names importance, not a hidden decision; it becomes
a bucket that accretes whatever has no better home, and its interface grows to match its contents. `retrieval` names
what it hides, so its surface can stay small as its internals grow. Candidate sources are now abstracted behind a
`Corpus` trait—not for generality, but because a second real implementor, a persistent store, is imminent and ranking
must stay written once across both; see `04-persistence.md`.

## What stays in `contract`

Retrieval adds no DTOs to `contract`. The feature rows it reads—`FeatureRow`/`DeclarationFeatureRow`,
`ProofGoalFeatureRow`, `Fingerprints`, `RoleFeature`, `OpaqueFeatureKey`, `SourceSpan`, the request and response
envelopes, `Diagnostic`, and the version constants—remain the stable Lean-to-Rust JSON contract. Retrieval's result
types (`Retrieval`, `Candidate`, `MatchExplanation`, `FeatureFamily`) are a Rust library interface owned by this crate,
not part of that JSON contract. The retrieval policy carries its own identity, `RETRIEVAL_POLICY_VERSION`, because ranks
are comparable only within one calibration; that constant lives here, not in `contract`, since it versions a retrieval
decision rather than a Lean fact.

## In-memory and storage-neutral

`SemanticIndex` is a view over rows the caller already holds—an inverted map from opaque key to the declarations that
carry it, plus the document counts rarity weighting needs. It is the reference implementation of the `Corpus` trait;
there is no database, cache, or on-disk layout here, so any caller can build an index from rows obtained however it
likes: a duplicate-search index, a proof-agent corpus, a test fixture. Persistence enters only as the `Corpus` seam a
later store fills, never as a storage dependency in this crate; see `04-persistence.md`.

## Source-backed proof-goal retrieval

A proof-goal anchor is built from a `ProofGoalFeatureRow`—the same source-backed row the Lean package computes from an
elaborated goal. Retrieval never sees, and never wants, rendered goal text. An anchor is a weighted set of opaque keys;
where those keys came from is the extractor's concern, and a declaration anchor and a proof-goal anchor are the same
thing once planned.

## Explanations without keys

A candidate match is explained in feature families—`statement_fingerprint`, `role_conclusion_const`, `role_head`, and
the rest—never in raw keys. Families are stable labels a caller can reason about and re-rank against; raw keys are
opaque equality tokens whose encoding is private to the Lean package. Head roles collapse to one family on purpose: a
caller should reason about "a head matched", not about which head encoding produced it. The per-role weighting that
still separates heads stays inside the policy module.

## Diagnostics without internals

Callers learn what happened through structured `Diagnostic`s, not through the structures that produced them. A posting
that fans out beyond its limit yields a `retrieval.posting_pruned` warning naming the feature family and the fanout; a
bounded top-k that overflows yields a `retrieval.top_k_saturated` warning with the limit and the dropped count. Neither
exposes the postings, the document-frequency table, the per-candidate scores, or the heap. The composite score is
internal to selection and never crosses the surface—callers order by `rank` and explain by family.
