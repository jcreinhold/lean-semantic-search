# Capability Contract

The Lean package exports commands through the generic `lean-rs-worker` capability ABI. The worker substrate owns process
framing and lifecycle; this package owns command names, versions, and JSON payload meanings.

## Exports

| Export | ABI | Meaning |
| --- | --- | --- |
| `lean_semantic_search_metadata` | `String -> IO String` | Report command names, semantic algorithm versions, and capability facts. |
| `lean_semantic_search_doctor` | `String -> IO String` | Return structured pass, warning, and error diagnostics. |
| `lean_semantic_search_declaration_features` | `String -> IO String` | Return declaration feature rows for requested modules or declarations. |
| `lean_semantic_search_proof_goal_features` | `String -> IO String` | Return proof-goal feature rows for a selected proof state. |
| `lean_semantic_search_stream_declaration_features` | `String -> USize -> USize -> IO UInt8` | Optional large-batch declaration feature export. |

The declaration and proof-goal commands return versioned semantic feature rows. The streaming export is reserved for
large declaration batches; the current implementation returns success and emits no rows.

## Metadata

Metadata uses the generic worker shape:

```json
{
  "commands": [{ "name": "declaration_features", "version": "declaration_features.v1" }],
  "capabilities": [{ "name": "semantic_features.declarations", "version": "features.roles.v3" }],
  "lean_version": "Lean 4.x",
  "extra": {
    "schema_version": "lean-semantic-search.capability.v1",
    "canonical_version": "canonical.expr.v3",
    "feature_version": "features.roles.v3"
  }
}
```

The command list is an advertisement, not a cache key. Downstream callers must use explicit schema and feature-version
fields in command responses when deciding compatibility.

## Doctor

Doctor reports use structured diagnostics:

```json
{
  "diagnostics": [
    { "severity": "pass", "code": "lean_semantic_search.boundary.ready", "message": "...", "details": {} }
  ],
  "metadata": { "schema_version": "lean-semantic-search.capability.v1" }
}
```

The doctor reports a passing boundary check and a passing feature-availability check.

## Feature Commands

Declaration requests identify imported modules and may restrict extraction to known declaration ids:

```json
{
  "modules": [{ "module": "My.Project.Module", "origin": "workspace", "source_root": "/repo" }],
  "declaration_ids": [],
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

## Streaming

The streaming export exists for large declaration batches. Its payload schema is the same semantic row schema as the
non-streaming declaration feature command, but delivery mechanics remain owned by `lean-rs-worker`. The current
implementation emits no payload entries and returns success.

## Versioning

| Version | Covers |
| --- | --- |
| `lean-semantic-search.capability.v1` | Capability metadata, doctor, and command envelope. |
| `canonical.expr.v3` | Canonical expression fingerprint algorithm. |
| `features.role_key.v1` | Private role-feature key algorithm. |
| `features.roles.v3` | Semantic feature-row algorithm. |
| `declaration_features.v1` | Declaration feature command schema. |
| `proof_goal_features.v1` | Proof-goal feature command schema. |

Retrieval-specific versions can be added later without changing these export names.
