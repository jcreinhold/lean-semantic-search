import LeanSemanticSearch.Canonical
import LeanSemanticSearch.DeclarationFeatures
import LeanSemanticSearch.RoleFeatures

/-!
Proof-goal semantic feature rows.

Callers provide an already-open Lean goal as expression facts. This module owns
feature extraction from that goal, not source elaboration or tactic selection.
-/

namespace LeanSemanticSearch.GoalFeatures

open Lean
open Lean.Meta

def rowPayload
    (goalId : String)
    (fingerprints : Canonical.Fingerprints)
    (roleFeatures : Array RoleFeatures.RoleFeature)
    (markers : Array String) : Json :=
  Json.mkObj
    [ ("goal_id", Json.str goalId)
    , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
    , ("fingerprints", DeclarationFeatures.fingerprintsJson fingerprints)
    , ("role_features", RoleFeatures.featuresJson roleFeatures)
    , ("low_signal_markers", RoleFeatures.markersJson markers)
    ]

def rowFromGoal (goalId : String) (fvars : Array Expr) (target : Expr) : MetaM Json := do
  let fingerprints ← Canonical.computeFromTelescopeWithLevels [] fvars target
  let (roleFeatures, markers) ← RoleFeatures.factsFromTelescope fvars target
  pure <| rowPayload goalId fingerprints roleFeatures markers

end LeanSemanticSearch.GoalFeatures
