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

Foundation commands return valid empty responses with a structured warning until the Lean feature package is
implemented.

## Metadata

Metadata uses the generic worker shape:

```json
{
  "commands": [{ "name": "declaration_features", "version": "declaration_features.foundation.v1" }],
  "capabilities": [{ "name": "semantic_features.declarations", "version": "features.foundation.v1" }],
  "lean_version": "Lean 4.x",
  "extra": { "schema_version": "lean-semantic-search.capability.v1", "foundation_only": true }
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

The foundation doctor reports a passing boundary check and a warning that real feature extraction arrives later.

## Feature Commands

Declaration and proof-goal feature responses use the same envelope:

```json
{
  "schema_version": "lean-semantic-search.capability.v1",
  "command": "declaration_features",
  "command_version": "declaration_features.foundation.v1",
  "feature_version": "features.foundation.v1",
  "rows": [],
  "diagnostics": []
}
```

Future rows may include declaration or goal identifiers, opaque fingerprints, role features, low-signal markers, and
bounded source spans. They must not include raw expressions, feature-key encodings, storage records, downstream report
fields, or transport response types.

## Streaming

The streaming export exists for large declaration batches. Its payload schema is the same semantic row schema as the
non-streaming declaration feature command, but delivery mechanics remain owned by `lean-rs-worker`. The foundation
implementation emits no payload entries and returns success.

## Versioning

- `lean-semantic-search.capability.v1`: capability metadata, doctor, and command envelope version.
- `features.foundation.v1`: placeholder semantic algorithm version.
- `declaration_features.foundation.v1`: declaration feature command schema version.
- `proof_goal_features.foundation.v1`: proof-goal feature command schema version.

Later prompts may add real feature versions without changing the export names.
