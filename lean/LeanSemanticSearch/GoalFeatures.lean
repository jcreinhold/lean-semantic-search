import LeanSemanticSearch.Canonical
import LeanSemanticSearch.RoleFeatures

/-!
Proof-goal semantic feature rows.

Callers provide an already-selected proof goal as a translated `GoalSnapshot`.
This module owns feature extraction from that snapshot, not source elaboration or
tactic selection. It is pure.
-/

namespace LeanSemanticSearch.GoalFeatures

open Lean (Json)
open LeanSemanticSearch.LeanCompat (GoalSnapshot)

def rowPayload
    (goalId : String)
    (fingerprints : Canonical.Fingerprints)
    (roleFeatures : Array RoleFeatures.RoleFeature)
    (markers : Array String) : Json :=
  Json.mkObj
    [ ("goal_id", Json.str goalId)
    , ("feature_version", Json.str JsonSupport.semanticFeatureVersion)
    , ("fingerprints", fingerprints.toJson)
    , ("role_features", RoleFeatures.featuresJson roleFeatures)
    , ("low_signal_markers", RoleFeatures.markersJson markers)
    ]

def rowFromSnapshot (snapshot : GoalSnapshot) : Json :=
  let fingerprints := Canonical.computeFromStatement snapshot.statement
  let (roleFeatures, markers) := RoleFeatures.factsFromStatement snapshot.statement
  rowPayload snapshot.goalId fingerprints roleFeatures markers

end LeanSemanticSearch.GoalFeatures
