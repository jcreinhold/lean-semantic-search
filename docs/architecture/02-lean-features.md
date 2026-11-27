# Lean Features

The Lean package owns semantic extraction. Rust callers receive versioned JSON facts; they do not learn how expressions
are traversed, how binders are scheduled, or how feature keys are encoded.

## Functional Boundaries

| Module | Hidden decision |
| --- | --- |
| `Canonical` | Expression traversal, universe normalization, binder scheduling, connective normalization, and fingerprint keys. |
| `RoleFeatures` | Role assignment, broad-head marking, low-signal markers, and role-key encoding. |
| `LeanCompat` | The only code that names volatile Lean internals: module import, declaration enumeration, generated/private filtering, source-range lookup, source elaboration, info-tree walks, and tactic proof-state selection. It returns owned records, never a `Lean.Expr`. |
| `ModuleExtraction` | Parsing the declaration request and handing it to `LeanCompat.collectDeclSources`. |
| `DeclarationFeatures` | Combining accepted declarations with canonical fingerprints and role features. |
| `GoalFeatures` | Computing the same semantic facts from an open proof goal's local context and target expression. |
| `GoalElaboration` | Parsing the proof-goal request, asking `LeanCompat.selectGoalSnapshotWithSetup` for the selected goal, and passing it to `GoalFeatures`. |
| `Json` | Command envelopes, version strings, request parsing helpers, and structured diagnostics. |
| `Capability` | The declaration command entry point, `declarationFeatures`. |

The split follows information ownership, not algorithm steps. Source elaboration and proof-goal selection are together
because they share source maps, info trees, and metavariable contexts. Feature assignment is separate because
declaration and proof-goal extraction both use it.

Search-path construction belongs to the host. `Capability.declarationFeatures` imports against Lean's current module
search path, and `GoalElaboration.runWithSetup` uses the Lake module setup the host passes. Neither initializes the
search path or rebuilds it from the process environment.

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
failures, and unavailable proof states return rows `[]` with an error diagnostic. This keeps the host's process and
transport handling separate from semantic command policy.
