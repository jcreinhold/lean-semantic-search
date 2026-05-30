import LeanSemanticSearch.Canonical
import LeanSemanticSearch.ModuleExtraction
import LeanSemanticSearch.RoleFeatures

/-!
Declaration semantic feature rows.

This module combines canonical fingerprints with role features for imported
declarations. It owns row contents but not module import mechanics or command
envelopes.
-/

namespace LeanSemanticSearch.DeclarationFeatures

open Lean
open Lean.Meta

private def selectDeclarations
    (ids? : Option (Array String))
    (declarations : Array ModuleExtraction.AcceptedDeclaration) :
    Except JsonSupport.Error (Array ModuleExtraction.AcceptedDeclaration) := do
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

def fingerprintsJson (fingerprints : Canonical.Fingerprints) : Json :=
  Json.mkObj
    [ ("statement", Json.str fingerprints.statement)
    , ("safe_binder_permutation", Json.str fingerprints.safeBinderPermutation)
    , ("connective_shape", Json.str fingerprints.connectiveShape)
    , ("conclusion_shape", Json.str fingerprints.conclusionShape)
    ]

def rowPayload
    (options : ModuleExtraction.Options)
    (declaration : ModuleExtraction.AcceptedDeclaration)
    (fingerprints : Canonical.Fingerprints)
    (roleFeatures : Array RoleFeatures.RoleFeature)
    (markers : Array String) : Json :=
  let source? :=
    ModuleExtraction.sourceSpanJson? options declaration.moduleSpec declaration.range?
  Json.mkObj
    [ ("declaration_id", Json.str declaration.declarationId)
    , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
    , ("fingerprints", fingerprintsJson fingerprints)
    , ("role_features", RoleFeatures.featuresJson roleFeatures)
    , ("binder_count", Json.num fingerprints.binderCount)
    , ("low_signal_markers", RoleFeatures.markersJson markers)
    , ("source", source?.getD Json.null)
    ]

private def semanticFacts
    (declaration : ModuleExtraction.AcceptedDeclaration) :
    MetaM (Canonical.Fingerprints × Array RoleFeatures.RoleFeature × Array String) := do
  forallTelescope declaration.constInfo.type fun fvars conclusion => do
    let fingerprints ←
      Canonical.computeFromTelescope declaration.constInfo fvars conclusion
    let (roleFeatures, markers) ← RoleFeatures.factsFromTelescope fvars conclusion
    pure (fingerprints, roleFeatures, markers)

def featureRows
    (options : ModuleExtraction.Options)
    (declarations : Array ModuleExtraction.AcceptedDeclaration) : MetaM (Array Json) := do
  let mut rows := #[]
  for declaration in declarations do
    let (fingerprints, roleFeatures, markers) ← semanticFacts declaration
    rows := rows.push (rowPayload options declaration fingerprints roleFeatures markers)
  pure rows

unsafe def runProfiled (payload : Json) (modules : Array ModuleExtraction.ModuleSpec) :
    IO (Except JsonSupport.Error ModuleExtraction.RunOutput) := do
  match JsonSupport.stringArrayField? payload "declaration_ids" with
  | .error err => pure <| .error err
  | .ok ids? =>
      match ←
        ModuleExtraction.withAcceptedDeclarationsProfiled payload modules fun options declarations => do
          match selectDeclarations ids? declarations with
          | .error err => pure <| Except.error err
          | .ok selected => Except.ok <$> featureRows options selected
      with
      | .error err => pure <| .error err
      | .ok (.error err, _stats) => pure <| .error err
      | .ok (.ok rows, stats) =>
          pure <| .ok { rows, stats := { stats with rowCount := rows.size } }

unsafe def run (payload : Json) (modules : Array ModuleExtraction.ModuleSpec) :
    IO (Except JsonSupport.Error (Array Json)) := do
  match ← runProfiled payload modules with
  | .error err => pure <| .error err
  | .ok output => pure <| .ok output.rows

end LeanSemanticSearch.DeclarationFeatures
