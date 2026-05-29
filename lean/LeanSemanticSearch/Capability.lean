import Lean

/-!
Foundation exports for the standalone Lean semantic search capability.

The exports use the generic `lean-rs-worker` capability ABI, but the command
names, versions, and response payloads are owned by this package.
-/

namespace LeanSemanticSearch.Capability

open Lean

def schemaVersion : String := "lean-semantic-search.capability.v1"

def foundationFeatureVersion : String := "features.foundation.v1"

def declarationFeatureCommandVersion : String := "declaration_features.foundation.v1"

def proofGoalFeatureCommandVersion : String := "proof_goal_features.foundation.v1"

private def commandJson (name version : String) : Json :=
  Json.mkObj [("name", Json.str name), ("version", Json.str version)]

private def capabilityJson (name version : String) : Json :=
  Json.mkObj [("name", Json.str name), ("version", Json.str version)]

private def diagnosticJson
    (severity code message : String)
    (details : Json := Json.null) : Json :=
  Json.mkObj
    [ ("severity", Json.str severity)
    , ("code", Json.str code)
    , ("message", Json.str message)
    , ("details", details)
    ]

private def foundationWarning (command : String) : Json :=
  diagnosticJson
    "warning"
    "lean_semantic_search.foundation.not_implemented"
    "semantic feature extraction is not implemented in the foundation package"
    (Json.mkObj [("command", Json.str command), ("foundation_only", Json.bool true)])

private def metadataPayload : Json :=
  Json.mkObj
    [ ( "commands"
      , Json.arr
          #[ commandJson "metadata" schemaVersion
           , commandJson "doctor" schemaVersion
           , commandJson "declaration_features" declarationFeatureCommandVersion
           , commandJson "proof_goal_features" proofGoalFeatureCommandVersion
           , commandJson "stream_declaration_features" declarationFeatureCommandVersion
           ] )
    , ( "capabilities"
      , Json.arr
          #[ capabilityJson "semantic_features.declarations" foundationFeatureVersion
           , capabilityJson "semantic_features.proof_goals" foundationFeatureVersion
           , capabilityJson "rows.json.streaming" schemaVersion
           , capabilityJson "diagnostics.structured" schemaVersion
           ] )
    , ("lean_version", Json.str s!"Lean {Lean.versionString}")
    , ( "extra"
      , Json.mkObj
          [ ("schema_version", Json.str schemaVersion)
          , ("package", Json.str "lean-semantic-search")
          , ("foundation_only", Json.bool true)
          ] )
    ]

private def doctorPayload : Json :=
  Json.mkObj
    [ ( "diagnostics"
      , Json.arr
          #[ diagnosticJson
               "pass"
               "lean_semantic_search.boundary.ready"
               "standalone semantic-search boundary is available"
               (Json.mkObj [("package", Json.str "lean-semantic-search")])
           , diagnosticJson
               "warning"
               "lean_semantic_search.foundation.not_implemented"
               "feature extraction is a foundation placeholder until the Lean feature package is implemented"
               (Json.mkObj [("next_prompt", Json.str "02-lean-feature-package-extraction")])
           ] )
    , ( "metadata"
      , Json.mkObj
          [ ("schema_version", Json.str schemaVersion)
          , ("foundation_only", Json.bool true)
          ] )
    ]

private def commandPayload (command version : String) : Json :=
  Json.mkObj
    [ ("schema_version", Json.str schemaVersion)
    , ("command", Json.str command)
    , ("feature_version", Json.str foundationFeatureVersion)
    , ("rows", Json.arr #[])
    , ("diagnostics", Json.arr #[foundationWarning command])
    , ("command_version", Json.str version)
    ]

@[export lean_semantic_search_metadata]
def metadata (_requestJson : String) : IO String :=
  pure metadataPayload.compress

@[export lean_semantic_search_doctor]
def doctor (_requestJson : String) : IO String :=
  pure doctorPayload.compress

@[export lean_semantic_search_declaration_features]
def declarationFeatures (_requestJson : String) : IO String :=
  pure (commandPayload "declaration_features" declarationFeatureCommandVersion).compress

@[export lean_semantic_search_proof_goal_features]
def proofGoalFeatures (_requestJson : String) : IO String :=
  pure (commandPayload "proof_goal_features" proofGoalFeatureCommandVersion).compress

@[export lean_semantic_search_stream_declaration_features]
def streamDeclarationFeatures (_requestJson : String) (_handle _trampoline : USize) : IO UInt8 :=
  pure 0

end LeanSemanticSearch.Capability

