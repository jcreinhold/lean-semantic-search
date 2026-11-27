# Capability Contract

A host runs this package's Lean code in its own supervised Lean process. It builds the package with
`lean-semantic-search-runtime`, puts the built modules on `LEAN_PATH`, and calls the entry points below from its own
Lean driver. This package owns the entry points, command names, versions, and JSON payload meanings. The host owns the
process, import selection, and transport.

## Entry Points

| Lean declaration | Type | Meaning |
| --- | --- | --- |
| `LeanSemanticSearch.Capability.declarationFeatures` | `String → IO String` | Answer one declaration feature request, given as JSON text, with a complete response envelope. |
| `LeanSemanticSearch.GoalElaboration.runWithSetup` | `ModuleSetup → Json → IO (Except JsonSupport.Error (Array Json))` | Elaborate one proof-goal request's source with the module setup Lake selected for it, and return the proof-goal rows or a structured error. |

Hosts import `LeanSemanticSearch.Capability`, which imports both. `declarationFeatures` imports the requested modules
from the current search path; the caller installs that path first.

`runWithSetup` takes the setup that `lake setup-file` prints for the source file. It loads the setup's dynlibs and
plugins and uses its imports, artifact map, options, package, and module mode. When one of these fails it returns an
error; it never falls back to the source header or the search path, and it never calls `initSearchPath`. A setup for
another module is a request error.

`runWithSetup` returns rows, not an envelope. Wrap them with the `LeanSemanticSearch.JsonSupport` helpers:

```lean
let command := JsonSupport.proofGoalCommand
let version := JsonSupport.proofGoalFeatureCommandVersion
let features := JsonSupport.semanticFeatureVersion
let response :=
  match ← GoalElaboration.runWithSetup setup request with
  | .ok rows => JsonSupport.responseJson command version features rows #[]
  | .error err => JsonSupport.errorResponseJson command version features err
```

`JsonSupport` also holds `schemaVersion`, `declarationCommand`, and `declarationFeatureCommandVersion`.
`crates/contract` mirrors these strings as Rust constants; a host compares them with each response before using its
rows.

## Feature Commands

Declaration requests identify imported modules and may restrict extraction to known declaration ids. Omit
`declaration_ids` (or send `null`) to discover all eligible declarations. A nonempty array selects those known ids; an
explicit empty array selects none. The Rust `DeclarationFeatureRequest` omits its empty vector for discovery and accepts
an omitted selector when deserializing, preserving the existing downstream discovery behavior.

```json
{
  "modules": [{ "module": "My.Project.Module", "origin": "workspace" }],
  "include_private": false,
  "include_generated": false
}
```

Proof-goal requests are source-backed. Lean elaborates the supplied source, selects a tactic proof state, and extracts
features from the selected goal's expressions and local declarations:

```json
{
  "module": "My.Project.Module",
  "source_text": "import My.Project.Module\n\ntheorem t : True := by\n  trivial",
  "file_label": "My/Project/Module.lean",
  "declaration": "t",
  "position": { "line": 4, "column": 3 },
  "namespace": null
}
```

Declaration request JSON objects must have unique keys, including nested objects and keys written with Unicode escapes.
Duplicate keys return an empty `rows` array and a `lean_semantic_search.request.invalid` error diagnostic in the
response. Proof-goal requests reach `runWithSetup` as parsed `Json`, so the host's parser decides; Lean's `Json.parse`
also rejects duplicate keys.

Declaration and proof-goal feature responses use the same envelope:

```json
{
  "schema_version": "lean-semantic-search.capability.v1",
  "command": "declaration_features",
  "command_version": "declaration_features.v1",
  "feature_version": "features.roles.v3",
  "rows": [],
  "diagnostics": []
}
```

Declaration rows include declaration ids, opaque fingerprints, role features, binder counts, low-signal markers, and
bounded source spans when Lean can recover ranges. Proof-goal rows include goal ids, opaque fingerprints, role features,
and low-signal markers. Rows must not include raw expressions, feature-key encodings, storage records, downstream report
fields, rendered goals, or transport response types.

## Versioning

| Version | Covers |
| --- | --- |
| `lean-semantic-search.capability.v1` | Command response envelope. |
| `canonical.expr.v3` | Canonical expression fingerprint algorithm. |
| `features.role_key.v1` | Private role-feature key algorithm; Lean only, since role keys are opaque to Rust. |
| `features.roles.v3` | Semantic feature-row algorithm. |
| `declaration_features.v1` | Declaration feature command schema. |
| `proof_goal_features.v1` | Proof-goal feature command schema. |

Ranking carries its own `RETRIEVAL_POLICY_VERSION` in `crates/retrieval`; see [03-retrieval.md](03-retrieval.md).
