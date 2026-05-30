import Lean
import LeanSemanticSearch.Json

/-!
Role-aware semantic facts shared by declaration and proof-goal extraction.

The public interface is intentionally small: callers inside this package ask for
role facts from an expression telescope and receive opaque keys plus low-signal
markers. The key encoding and broad-head list remain private here.
-/

namespace LeanSemanticSearch.RoleFeatures

open Lean
open Lean.Meta

private def hashSeed : UInt64 := 14695981039346656037

private def hashPrime : UInt64 := 1099511628211

private def stableHash (text : String) : String :=
  toString <|
    text.foldl
      (fun acc char => (acc ^^^ char.toNat.toUInt64) * hashPrime)
      hashSeed

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

private def containsFeature (features : Array RoleFeature) (feature : RoleFeature) : Bool :=
  features.any fun existing =>
    existing.role == feature.role && existing.name == feature.name

private def pushFeature (features : Array RoleFeature) (feature : RoleFeature) :
    Array RoleFeature :=
  if containsFeature features feature then features else features.push feature

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

private partial def appHead (expr : Expr) : Expr :=
  match expr with
  | .app fn _ => appHead fn
  | .mdata _ body => appHead body
  | other => other

private def headName? (expr : Expr) : Option Name :=
  match appHead expr with
  | .const name _ => some name
  | _ => none

private def sortedNamesFromSet (names : NameSet) : Array Name :=
  names.toArray.qsort fun left right => left.toString < right.toString

private def addConstants
    (role : Role)
    (expr : Expr)
    (features : Array RoleFeature) : Array RoleFeature := Id.run do
  let mut result := features
  for name in sortedNamesFromSet expr.getUsedConstantsAsSet do
    result := pushFeature result { role, name }
  pure result

private def addHead
    (role : Role)
    (expr : Expr)
    (features : Array RoleFeature) : Array RoleFeature :=
  match headName? expr with
  | some name => pushFeature features { role, name }
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

def factsFromTelescope
    (fvars : Array Expr)
    (conclusion : Expr) : MetaM (Array RoleFeature × Array String) := do
  let mut features := #[]
  features := addConstants .conclusionConst conclusion features
  features := addHead .conclusionHead conclusion features
  for fvar in fvars do
    let localDecl ← fvar.fvarId!.getDecl
    if ← Meta.isProp localDecl.type then
      features := addConstants .hypothesisConst localDecl.type features
      features := addHead .hypothesisHead localDecl.type features
    else
      features := addHead .binderDomainHead localDecl.type features
  pure (sortedFeatures features, lowSignalMarkers features)

end LeanSemanticSearch.RoleFeatures
