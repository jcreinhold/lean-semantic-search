import Lean
import LeanSemanticSearch.DeclarationFeatures
import LeanSemanticSearch.GoalElaboration

/-!
Exports for the standalone Lean semantic search capability.

The exports use the generic `lean-rs-worker` capability ABI, but the command
names, versions, and response payloads are owned by this package.
-/

namespace LeanSemanticSearch.Capability

open Lean

private def commandJson (name version : String) : Json :=
  Json.mkObj [("name", Json.str name), ("version", Json.str version)]

private def capabilityJson (name version : String) : Json :=
  Json.mkObj [("name", Json.str name), ("version", Json.str version)]

private def metadataPayload : Json :=
  Json.mkObj
    [ ( "commands"
      , Json.arr
          #[ commandJson "metadata" JsonSupport.schemaVersion
           , commandJson "doctor" JsonSupport.schemaVersion
           , commandJson JsonSupport.declarationCommand JsonSupport.declarationFeatureCommandVersion
           , commandJson JsonSupport.proofGoalCommand JsonSupport.proofGoalFeatureCommandVersion
           , commandJson "stream_declaration_features" JsonSupport.declarationFeatureCommandVersion
           ] )
    , ( "capabilities"
      , Json.arr
          #[ capabilityJson "semantic_features.declarations" JsonSupport.semanticFeatureVersion
           , capabilityJson "semantic_features.proof_goals" JsonSupport.semanticFeatureVersion
           , capabilityJson "rows.json.streaming" JsonSupport.schemaVersion
           , capabilityJson "diagnostics.structured" JsonSupport.schemaVersion
           ] )
    , ("lean_version", Json.str s!"Lean {Lean.versionString}")
    , ( "extra"
      , Json.mkObj
          [ ("schema_version", Json.str JsonSupport.schemaVersion)
          , ("package", Json.str "lean-semantic-search")
          , ("canonical_version", Json.str Canonical.version)
          , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
          ] )
    ]

private def doctorPayload : Json :=
  Json.mkObj
    [ ( "diagnostics"
      , Json.arr
          #[ JsonSupport.passDiagnostic
               "lean_semantic_search.boundary.ready"
               "standalone semantic-search boundary is available"
               (Json.mkObj [("package", Json.str "lean-semantic-search")])
           , JsonSupport.passDiagnostic
               "lean_semantic_search.features.ready"
               "semantic feature extraction is available"
               (Json.mkObj
                 [ ("canonical_version", Json.str Canonical.version)
                 , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
                 ])
           ] )
    , ( "metadata"
      , Json.mkObj
          [ ("schema_version", Json.str JsonSupport.schemaVersion)
          , ("canonical_version", Json.str Canonical.version)
          , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
          ] )
    ]

@[export lean_semantic_search_metadata]
def metadata (_requestJson : String) : IO String :=
  pure metadataPayload.compress

@[export lean_semantic_search_doctor]
def doctor (_requestJson : String) : IO String :=
  pure doctorPayload.compress

@[export lean_semantic_search_declaration_features]
unsafe def declarationFeatures (requestJson : String) : IO String := do
  let payloadResult := JsonSupport.parsePayload requestJson
  match payloadResult with
  | .error err =>
      pure <|
        (JsonSupport.errorResponseJson
          JsonSupport.declarationCommand
          JsonSupport.declarationFeatureCommandVersion
          JsonSupport.semanticFeatureVersion
          err).compress
  | .ok payload =>
      match ModuleExtraction.parseModules payload with
      | .error err =>
          pure <|
            (JsonSupport.errorResponseJson
              JsonSupport.declarationCommand
              JsonSupport.declarationFeatureCommandVersion
              JsonSupport.semanticFeatureVersion
              err).compress
      | .ok modules =>
          match ← DeclarationFeatures.run payload modules with
          | .error err =>
              pure <|
                (JsonSupport.errorResponseJson
                  JsonSupport.declarationCommand
                  JsonSupport.declarationFeatureCommandVersion
                  JsonSupport.semanticFeatureVersion
                  err).compress
          | .ok rows =>
              pure <|
                (JsonSupport.responseJson
                  JsonSupport.declarationCommand
                  JsonSupport.declarationFeatureCommandVersion
                  JsonSupport.semanticFeatureVersion
                  rows
                  #[]).compress

@[export lean_semantic_search_proof_goal_features]
unsafe def proofGoalFeatures (requestJson : String) : IO String := do
  match JsonSupport.parsePayload requestJson with
  | .error err =>
      pure <|
        (JsonSupport.errorResponseJson
          JsonSupport.proofGoalCommand
          JsonSupport.proofGoalFeatureCommandVersion
          JsonSupport.semanticFeatureVersion
          err).compress
  | .ok payload =>
      match ← GoalElaboration.run payload with
      | .error err =>
          pure <|
            (JsonSupport.errorResponseJson
              JsonSupport.proofGoalCommand
              JsonSupport.proofGoalFeatureCommandVersion
              JsonSupport.semanticFeatureVersion
              err).compress
      | .ok rows =>
          pure <|
            (JsonSupport.responseJson
              JsonSupport.proofGoalCommand
              JsonSupport.proofGoalFeatureCommandVersion
              JsonSupport.semanticFeatureVersion
              rows
              #[]).compress

@[export lean_semantic_search_stream_declaration_features]
def streamDeclarationFeatures (_requestJson : String) (_handle _trampoline : USize) : IO UInt8 :=
  pure 0

end LeanSemanticSearch.Capability
