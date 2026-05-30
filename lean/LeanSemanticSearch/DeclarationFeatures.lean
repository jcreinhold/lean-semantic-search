import LeanSemanticSearch.Canonical
import LeanSemanticSearch.ModuleExtraction
import LeanSemanticSearch.RoleFeatures

/-!
Declaration semantic feature rows.

This module combines canonical fingerprints with role features for extracted
declarations. It owns row contents but not import mechanics or command
envelopes. It is pure: the heavy `MetaM` work happened behind `LeanCompat`, so a
row is built from an owned `DeclSource`.
-/

namespace LeanSemanticSearch.DeclarationFeatures

open Lean (Json)
open LeanSemanticSearch.LeanCompat (DeclSource)

private def selectDeclarations
    (ids? : Option (Array String))
    (declarations : Array DeclSource) :
    Except JsonSupport.Error (Array DeclSource) := do
  match ids? with
  | none => pure declarations
  | some ids =>
      let mut selected := #[]
      let mut missing := #[]
      for id in ids do
        match declarations.find? fun declaration => declaration.declarationId == id with
        | some declaration => selected := selected.push declaration
        | none => missing := missing.push id
      if !missing.isEmpty then
        throw <|
          JsonSupport.invalidRequest
            "unknown declaration id requested by `declaration_features`"
            (some <| Json.mkObj [("declaration_ids", JsonSupport.stringArrayJson missing)])
      pure selected

def rowPayload
    (declaration : DeclSource)
    (fingerprints : Canonical.Fingerprints)
    (roleFeatures : Array RoleFeatures.RoleFeature)
    (markers : Array String) : Json :=
  let source? := ModuleExtraction.sourceSpanJson? declaration.range?
  Json.mkObj
    [ ("declaration_id", Json.str declaration.declarationId)
    , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
    , ("fingerprints", fingerprints.toJson)
    , ("role_features", RoleFeatures.featuresJson roleFeatures)
    , ("binder_count", Json.num fingerprints.binderCount)
    , ("low_signal_markers", RoleFeatures.markersJson markers)
    , ("source", source?.getD Json.null)
    ]

private def semanticFacts (declaration : DeclSource) :
    Canonical.Fingerprints × Array RoleFeatures.RoleFeature × Array String :=
  let fingerprints := Canonical.computeFromStatement declaration.statement
  let (roleFeatures, markers) := RoleFeatures.factsFromStatement declaration.statement
  (fingerprints, roleFeatures, markers)

def featureRows (declarations : Array DeclSource) : Array Json := Id.run do
  let mut rows := #[]
  for declaration in declarations do
    let (fingerprints, roleFeatures, markers) := semanticFacts declaration
    rows := rows.push (rowPayload declaration fingerprints roleFeatures markers)
  pure rows

unsafe def runProfiled (payload : Json) (modules : Array ModuleExtraction.ModuleSpec) :
    IO (Except JsonSupport.Error ModuleExtraction.RunOutput) := do
  match JsonSupport.stringArrayField? payload "declaration_ids" with
  | .error err => pure <| .error err
  | .ok ids? =>
      match ←
        ModuleExtraction.withDeclSourcesProfiled payload modules fun _options declSources =>
          (selectDeclarations ids? declSources).map featureRows
      with
      | .error err => pure <| .error err
      | .ok (rows, stats) =>
          pure <| .ok { rows, stats := { stats with rowCount := rows.size } }

unsafe def run (payload : Json) (modules : Array ModuleExtraction.ModuleSpec) :
    IO (Except JsonSupport.Error (Array Json)) := do
  match ← runProfiled payload modules with
  | .error err => pure <| .error err
  | .ok output => pure <| .ok output.rows

end LeanSemanticSearch.DeclarationFeatures
