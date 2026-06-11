import LeanSemanticSearch.Hashing
import LeanSemanticSearch.Json
import LeanSemanticSearch.LeanCompat

/-!
Role-aware semantic facts shared by declaration and proof-goal extraction.

The public interface is intentionally small: callers inside this package ask for
role facts from a translated statement and receive opaque keys plus low-signal
markers. The key encoding and broad-head list remain private here.

This module is pure: heads, propositional status, and used constants are
precomputed by `LeanCompat` into the `StatementShape`, so role assignment never
touches a `Lean.Expr`.
-/

namespace LeanSemanticSearch.RoleFeatures

open Lean (Json Name)
open LeanSemanticSearch.LeanCompat (StatementShape)
open LeanSemanticSearch.Hashing (stableHash)

inductive Role where
  | conclusionConst
  | conclusionHead
  | hypothesisConst
  | hypothesisHead
  | binderDomainHead
  deriving BEq

namespace Role

def asString : Role → String
  | .conclusionConst => "conclusion_const"
  | .conclusionHead => "conclusion_head"
  | .hypothesisConst => "hypothesis_const"
  | .hypothesisHead => "hypothesis_head"
  | .binderDomainHead => "binder_domain_head"

end Role

structure RoleFeature where
  role : Role
  name : Name

def RoleFeature.sortKey (feature : RoleFeature) : String :=
  s!"{feature.role.asString}:{feature.name}"

private def roleKey (feature : RoleFeature) : String :=
  let text := feature.sortKey
  s!"{JsonSupport.roleKeyVersion}:{stableHash text}"

def RoleFeature.toJson (feature : RoleFeature) : Json :=
  Json.mkObj
    [ ("role", Json.str feature.role.asString)
    , ("key", Json.str (roleKey feature))
    , ("display", Json.str feature.name.toString)
    ]

/-- Keep the first occurrence of each distinct feature, deduplicating on the
    injective `sortKey` ("{role}:{name}") with a hash set — O(n) rather than the
    previous per-insert linear scan (O(n²) over a statement's features). The
    observable output is unchanged: `featuresJson`/`sortedFeatures` re-sort by the
    same key, so only the distinct *set*, not insertion order, is ever emitted. -/
private def distinctFeatures (features : Array RoleFeature) : Array RoleFeature := Id.run do
  let mut seen : Std.HashSet String := {}
  let mut result := #[]
  for feature in features do
    let key := feature.sortKey
    unless seen.contains key do
      seen := seen.insert key
      result := result.push feature
  pure result

def sortedFeatures (features : Array RoleFeature) : Array RoleFeature :=
  features.qsort fun left right => left.sortKey < right.sortKey

def featuresJson (features : Array RoleFeature) : Json :=
  Json.arr (sortedFeatures features |>.map RoleFeature.toJson)

-- Heads that appear in a large fraction of statements (equality, the logical
-- connectives, basic order and membership). They barely narrow a search, so
-- features built on them are marked low-signal.
private def broadHeadNames : Std.HashSet String :=
  [ "Eq"
  , "Iff"
  , "Exists"
  , "Nonempty"
  , "False"
  , "True"
  , "Ne"
  , "Not"
  , "And"
  , "Or"
  , "LE.le"
  , "LT.lt"
  , "Membership.mem"
  , "HasSubset.Subset"
  ].foldl (fun set name => set.insert name) {}

private def isBroadHead (name : Name) : Bool :=
  broadHeadNames.contains name.toString

private def addConstants
    (role : Role)
    (constants : Array Name)
    (features : Array RoleFeature) : Array RoleFeature :=
  constants.foldl (init := features) fun result name => result.push { role, name }

private def addHead
    (role : Role)
    (head? : Option Name)
    (features : Array RoleFeature) : Array RoleFeature :=
  match head? with
  | some name => features.push { role, name }
  | none => features

def lowSignalMarkers (features : Array RoleFeature) : Array String := Id.run do
  let mut markers := #[]
  for feature in features do
    match feature.role with
    | .conclusionHead | .hypothesisHead | .binderDomainHead =>
        if isBroadHead feature.name then
          let marker := s!"broad_head:{feature.name}"
          if !markers.contains marker then
            markers := markers.push marker
    | .conclusionConst | .hypothesisConst => pure ()
  pure <| markers.qsort (· < ·)

def markersJson (markers : Array String) : Json :=
  Json.arr (markers.map Json.str)

/-- Assign role features from a translated statement. The conclusion contributes
    its constants and head; each binder contributes either hypothesis facts (when
    its type is a proposition) or just its domain head. -/
def factsFromStatement (statement : StatementShape) :
    Array RoleFeature × Array String := Id.run do
  let mut features := #[]
  features := addConstants .conclusionConst statement.conclusionConsts features
  features := addHead .conclusionHead statement.conclusionHead? features
  for binder in statement.binders do
    if binder.isProp then
      features := addConstants .hypothesisConst binder.usedConsts features
      features := addHead .hypothesisHead binder.headConst? features
    else
      features := addHead .binderDomainHead binder.headConst? features
  features := distinctFeatures features
  pure (sortedFeatures features, lowSignalMarkers features)

end LeanSemanticSearch.RoleFeatures
