import LeanSemanticSearch.Hashing
import LeanSemanticSearch.LeanCompat

/-!
Canonical expression fingerprints for semantic search.

The keys emitted here are opaque equality tokens: callers compare them but must
never interpret their bytes. Traversal order, binder scheduling, universe
encoding, and the key format stay private so they can change without breaking
callers.

This module is pure: it serializes the package-owned `StatementShape` (produced
by `LeanCompat`) and never touches a `Lean.Expr`. A Lean toolchain change cannot
reach it.
-/

namespace LeanSemanticSearch.Canonical

open Lean (Json)
open LeanSemanticSearch.LeanCompat
open LeanSemanticSearch.Hashing (stableHash)

/-- Semantic algorithm marker for canonical expression fingerprints. -/
def version : String := "canonical.expr.v3"

/-- Opaque semantic keys for one declaration statement or proof goal. -/
structure Fingerprints where
  statement : String
  safeBinderPermutation : String
  connectiveShape : String
  conclusionShape : String
  binderCount : Nat

/-- Render the four opaque keys as the response object. `binderCount` is omitted:
    it is a count, not a key, and callers carry it as a sibling field. The JSON
    field names are response-DTO contract and must stay byte-identical. -/
def Fingerprints.toJson (fingerprints : Fingerprints) : Json :=
  Json.mkObj
    [ ("statement", Json.str fingerprints.statement)
    , ("safe_binder_permutation", Json.str fingerprints.safeBinderPermutation)
    , ("connective_shape", Json.str fingerprints.connectiveShape)
    , ("conclusion_shape", Json.str fingerprints.conclusionShape)
    ]

private def fingerprintKey (kind body : String) : String :=
  s!"{version}:{kind}:{stableHash body}"

private abbrev FVarOrdinals := Std.HashMap Lean.Name Nat

private structure SerializerContext where
  fvars : FVarOrdinals

private inductive ExprMode where
  | exact
  | connective
  deriving BEq

private partial def levelKey : LevelShape → String
  | .zero => "0"
  | .succ level => s!"s({levelKey level})"
  | .max left right => s!"max({levelKey left},{levelKey right})"
  | .imax left right => s!"imax({levelKey left},{levelKey right})"
  | .paramOrdinal index => s!"p{index}"
  | .paramName name => s!"p:{name}"
  | .mvar name => s!"m:{name}"

private def binderInfoKey : Lean.BinderInfo → String
  | .default => "explicit"
  | .implicit => "implicit"
  | .strictImplicit => "strictImplicit"
  | .instImplicit => "instImplicit"

private def sortParts (parts : Array String) : String :=
  String.intercalate "," (parts.qsort (· < ·)).toList

private def fvarKey (ctx : SerializerContext) (name : Lean.Name) : String :=
  match ctx.fvars.get? name with
  | some index => s!"v{index}"
  | none => s!"free:{name}"

private def exprNodeBudget : Nat := 4000

private def exprDepthBudget : Nat := 80

private partial def exprKeyCore
    (ctx : SerializerContext)
    (mode : ExprMode)
    (depth : Nat)
    (expr : ExprShape) : StateM Nat String := do
  let remaining ← get
  if remaining == 0 then
    pure "(truncated budget)"
  else if depth == 0 then
    pure "(truncated depth)"
  else
    set (remaining - 1)
    match expr with
    | .bvar index => pure s!"b{index}"
    | .fvar name => pure (fvarKey ctx name)
    | .mvar name => pure s!"mvar:{name}"
    | .sort level => pure s!"(sort {levelKey level})"
    | .const name levels =>
        let levelKeys := levels.map levelKey
        pure s!"(const {name}[{String.intercalate "," levelKeys.toList}])"
    | .app head args =>
        if mode == .connective then
          match head, args.toList with
          | .const ``And _, [left, right] =>
              let leftKey ← exprKeyCore ctx mode (depth - 1) left
              let rightKey ← exprKeyCore ctx mode (depth - 1) right
              pure s!"(And {sortParts #[leftKey, rightKey]})"
          | .const ``Or _, [left, right] =>
              let leftKey ← exprKeyCore ctx mode (depth - 1) left
              let rightKey ← exprKeyCore ctx mode (depth - 1) right
              pure s!"(Or {sortParts #[leftKey, rightKey]})"
          | .const ``Iff _, [left, right] =>
              let leftKey ← exprKeyCore ctx mode (depth - 1) left
              let rightKey ← exprKeyCore ctx mode (depth - 1) right
              pure s!"(Iff {sortParts #[leftKey, rightKey]})"
          | .const ``Eq _, [type, left, right] =>
              let typeKey ← exprKeyCore ctx mode (depth - 1) type
              let leftKey ← exprKeyCore ctx mode (depth - 1) left
              let rightKey ← exprKeyCore ctx mode (depth - 1) right
              pure s!"(Eq {typeKey} {sortParts #[leftKey, rightKey]})"
          | _, _ => appKey ctx mode (depth - 1) head args
        else
          appKey ctx mode (depth - 1) head args
    | .lam info domain body =>
        let domainKey ← exprKeyCore ctx mode (depth - 1) domain
        let bodyKey ← exprKeyCore ctx mode (depth - 1) body
        pure s!"(lam {binderInfoKey info} {domainKey} {bodyKey})"
    | .forallE info domain body =>
        let domainKey ← exprKeyCore ctx mode (depth - 1) domain
        let bodyKey ← exprKeyCore ctx mode (depth - 1) body
        pure s!"(forall {binderInfoKey info} {domainKey} {bodyKey})"
    | .letE type value body =>
        let typeKey ← exprKeyCore ctx mode (depth - 1) type
        let valueKey ← exprKeyCore ctx mode (depth - 1) value
        let bodyKey ← exprKeyCore ctx mode (depth - 1) body
        pure s!"(let {typeKey} {valueKey} {bodyKey})"
    | .natLit value => pure s!"(nat {value})"
    | .strLit value => pure s!"(str {value.length}:{value})"
    | .mdata body => exprKeyCore ctx mode (depth - 1) body
    | .proj typeName index body =>
        let bodyKey ← exprKeyCore ctx mode (depth - 1) body
        pure s!"(proj {typeName}.{index} {bodyKey})"
where
  appKey
      (ctx : SerializerContext)
      (mode : ExprMode)
      (depth : Nat)
      (head : ExprShape)
      (args : Array ExprShape) : StateM Nat String := do
    let headKey ← exprKeyCore ctx mode depth head
    let mut parts := #[]
    for arg in args do
      parts := parts.push (← exprKeyCore ctx mode depth arg)
    pure s!"(app {headKey} [{String.intercalate "," parts.toList}])"

private def exprKey (ctx : SerializerContext) (mode : ExprMode) (expr : ExprShape) : String :=
  (exprKeyCore ctx mode exprDepthBudget expr).run' exprNodeBudget

private def bindFVar (ctx : SerializerContext) (binder : BinderShape) (ordinal : Nat) :
    SerializerContext :=
  { ctx with fvars := ctx.fvars.insert binder.fvarName ordinal }

private def allDepsScheduled (scheduled : Std.HashSet Nat) (deps : Array Nat) : Bool :=
  deps.all fun dep => scheduled.contains dep

private def binderSortKey (ctx : SerializerContext) (binder : BinderShape) : String :=
  s!"{binderInfoKey binder.info}:{exprKey ctx .exact binder.type}"

private partial def scheduleBinders
    (baseCtx : SerializerContext)
    (binders : Array BinderShape) : Array BinderShape := Id.run do
  let mut result := #[]
  let mut scheduled : Std.HashSet Nat := {}
  let mut ctx := baseCtx
  while result.size < binders.size do
    let mut ready := #[]
    for binder in binders do
      if !scheduled.contains binder.index && allDepsScheduled scheduled binder.deps then
        ready := ready.push binder
    if ready.isEmpty then
      for binder in binders do
        if !scheduled.contains binder.index then
          ready := ready.push binder
    let sortedReady :=
      ready.qsort fun left right =>
        let leftKey := binderSortKey ctx left
        let rightKey := binderSortKey ctx right
        if leftKey == rightKey then left.index < right.index else leftKey < rightKey
    match sortedReady[0]? with
    | some next =>
        let ordinal := result.size
        result := result.push next
        scheduled := scheduled.insert next.index
        ctx := bindFVar ctx next ordinal
    | none =>
        return result
  pure result

private def bindersContext (baseCtx : SerializerContext) (binders : Array BinderShape) :
    SerializerContext := Id.run do
  let mut ctx := baseCtx
  let mut ordinal := 0
  for binder in binders do
    ctx := bindFVar ctx binder ordinal
    ordinal := ordinal + 1
  pure ctx

private def statementBody
    (baseCtx : SerializerContext)
    (binders : Array BinderShape)
    (conclusion : ExprShape)
    (mode : ExprMode) : String := Id.run do
  let mut ctx := baseCtx
  let mut ordinal := 0
  let mut binderKeys := #[]
  for binder in binders do
    let domainKey := exprKey ctx mode binder.type
    binderKeys := binderKeys.push s!"({binderInfoKey binder.info} {domainKey})"
    ctx := bindFVar ctx binder ordinal
    ordinal := ordinal + 1
  pure s!"(forall [{String.intercalate "," binderKeys.toList}] {exprKey ctx mode conclusion})"

/-- Compute the opaque fingerprints for a translated statement. Pure: the
    statement has already been lowered to owned shapes by `LeanCompat`. -/
def computeFromStatement (statement : StatementShape) : Fingerprints := Id.run do
  let baseCtx : SerializerContext := { fvars := {} }
  let binders := statement.binders
  let scheduled := scheduleBinders baseCtx binders
  let conclusionCtx := bindersContext baseCtx binders
  let statementKey := statementBody baseCtx binders statement.conclusion .exact
  let safeBinderPermutation := statementBody baseCtx scheduled statement.conclusion .exact
  let connectiveShape := statementBody baseCtx binders statement.conclusion .connective
  let conclusionShape := exprKey conclusionCtx .connective statement.conclusion
  pure
    { statement := fingerprintKey "statement" statementKey
      safeBinderPermutation := fingerprintKey "safe_binder_permutation" safeBinderPermutation
      connectiveShape := fingerprintKey "connective_shape" connectiveShape
      conclusionShape := fingerprintKey "conclusion_shape" conclusionShape
      binderCount := binders.size }

end LeanSemanticSearch.Canonical
