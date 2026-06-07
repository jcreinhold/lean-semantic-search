# Lean Features

The Lean package owns semantic extraction. Rust callers receive versioned JSON facts; they do not learn how expressions
are traversed, how binders are scheduled, or how feature keys are encoded.

## Functional Boundaries

| Module | Hidden decision |
| --- | --- |
| `Canonical` | Expression traversal, universe normalization, binder scheduling, connective normalization, and fingerprint keys. |
| `RoleFeatures` | Role assignment, broad-head marking, low-signal markers, and role-key encoding. |
| `ModuleExtraction` | Request parsing, timing, module import through `LeanCompat.collectDeclSources`, declaration filtering, generated/private filtering, and source-range lookup. |
| `DeclarationFeatures` | Combining accepted declarations with canonical fingerprints and role features. |
| `GoalFeatures` | Computing the same semantic facts from an open proof goal's local context and target expression. |
| `GoalElaboration` | Elaborating source text, walking the info tree, selecting a tactic proof state, and passing expressions to `GoalFeatures`. |
| `Json` | Command envelopes, version strings, request parsing helpers, and structured diagnostics. |

The split follows information ownership, not algorithm steps. Source elaboration and proof-goal selection are together
because they share source maps, info trees, and metavariable contexts. Feature assignment is separate because
declaration and proof-goal extraction both use it.

Search-path construction belongs to the caller that embeds the capability. Hosted exports import and elaborate against
Lean's current module search path; they do not initialize that path or rebuild it from process environment.

## Declaration Features

Declaration extraction imports requested modules, filters declarations, opens each declaration type with
`forallTelescope`, and emits:

- opaque canonical fingerprints;
- role-aware semantic features;
- a top-level binder count;
- low-signal markers such as broad heads;
- a source span when Lean can recover declaration ranges.

Feature rows use `features.roles.v3`. Opaque fingerprint strings currently carry the private canonical prefix
`canonical.expr.v3`, but callers must treat the whole string as an equality token.

## Proof-Goal Features

Proof-goal extraction is source-backed. The request supplies source text plus a module/declaration/position selector.
Lean parses and elaborates that source, selects a tactic state from the info tree, opens the first pre-tactic goal, and
extracts from the goal target plus non-implementation-detail locals.

The command does not compute features from pretty-printed goal text. Rendered goals may be useful diagnostics in other
tools, but they are not semantic input here.

## Diagnostics

Command failures stay inside the command envelope as structured diagnostics. Malformed JSON, invalid selectors, import
failures, and unavailable proof states return rows `[]` with an error diagnostic. This keeps the generic worker
transport separate from semantic command policy.
