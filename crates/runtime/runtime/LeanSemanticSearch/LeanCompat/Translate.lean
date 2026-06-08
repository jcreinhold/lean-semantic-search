import Lean
import LeanSemanticSearch.LeanCompat.Shape

/-!
The one place in the package that pattern-matches `Lean.Expr` and `Lean.Level`.

`statementOfConstant`/`statementOfGoal` run the telescope, read every
`LocalDecl`, and translate the conclusion and each binder type into the owned
`StatementShape`. Every fact the downstream feature logic needs — the structural
shape, binder dependencies, propositional status, head constant, and used
constants — is computed here, so that `Canonical` and `RoleFeatures` never touch
a `Lean.Expr`.

If a future Lean release adds an `Expr` or `Level` constructor, the matches below
fail to compile — loudly, in this file, with a clear site to map the new shape.
That is the intended failure mode: the matches are deliberately exhaustive so a
new constructor can never be silently bucketed (which would corrupt the opaque
canonical fingerprint keys).
-/

namespace LeanSemanticSearch.LeanCompat

open Lean
open Lean.Meta

/-- Resolve a normalized level into the owned `LevelShape`, mapping bound
    parameters to their ordinal position. -/
private partial def translateLevel (params : Std.HashMap Name Nat) : Level → LevelShape
  | .zero => .zero
  | .succ level => .succ (translateLevel params level)
  | .max left right => .max (translateLevel params left) (translateLevel params right)
  | .imax left right => .imax (translateLevel params left) (translateLevel params right)
  | .param name =>
      match params.get? name with
      | some index => .paramOrdinal index
      | none => .paramName name
  | .mvar mvarId => .mvar mvarId.name

/-- Translate one expression into the owned `ExprShape`. Faithful and total:
    the structure is preserved 1:1 (levels are normalized exactly where the
    canonical traversal normalized them), so the serializer over `ExprShape`
    reproduces the canonical keys byte-for-byte. -/
private partial def translateExpr (params : Std.HashMap Name Nat) : Expr → ExprShape
  | .bvar index => .bvar index
  | .fvar fvarId => .fvar fvarId.name
  | .mvar mvarId => .mvar mvarId.name
  | .sort level => .sort (translateLevel params level.normalize)
  | .const name levels => .const name (levels.toArray.map fun l => translateLevel params l.normalize)
  | e@(.app ..) =>
      -- Flatten the application spine head-first, mirroring `appFnArgs`.
      let rec go (current : Expr) (args : Array Expr) : Expr × Array Expr :=
        match current with
        | .app fn arg => go fn (args.push arg)
        | other => (other, args.reverse)
      let (head, args) := go e #[]
      .app (translateExpr params head) (args.map (translateExpr params))
  | .lam _ domain body info =>
      .lam info (translateExpr params domain) (translateExpr params body)
  | .forallE _ domain body info =>
      .forallE info (translateExpr params domain) (translateExpr params body)
  | .letE _ type value body _ =>
      .letE (translateExpr params type) (translateExpr params value) (translateExpr params body)
  | .lit (.natVal value) => .natLit value
  | .lit (.strVal value) => .strLit value
  | .mdata _ body => .mdata (translateExpr params body)
  | .proj typeName index struct => .proj typeName index (translateExpr params struct)

/-- The application head with metadata and arguments peeled off, used to read a
    statement's head constant. -/
private partial def appHead : Expr → Expr
  | .app fn _ => appHead fn
  | .mdata _ body => appHead body
  | other => other

private def headConst? (expr : Expr) : Option Name :=
  match appHead expr with
  | .const name _ => some name
  | _ => none

private def sortedNamesFromSet (names : NameSet) : Array Name :=
  names.toArray.qsort fun left right => left.toString < right.toString

private def usedConstants (expr : Expr) : Array Name :=
  sortedNamesFromSet expr.getUsedConstantsAsSet

/-- Which entries of `fvars` appear free in `type`, by position. Drives
    dependency-safe binder scheduling. -/
private def dependencies (type : Expr) (fvars : Array Expr) : Array Nat := Id.run do
  let used := (collectFVars {} type).fvarSet
  let mut deps := #[]
  let mut index := 0
  for fvar in fvars do
    if used.contains fvar.fvarId! then
      deps := deps.push index
    index := index + 1
  pure deps

private def levelParamMap (levelParams : List Name) : Std.HashMap Name Nat := Id.run do
  let mut map : Std.HashMap Name Nat := {}
  let mut index := 0
  for param in levelParams do
    map := map.insert param index
    index := index + 1
  pure map

/-- Translate an open telescope (level parameters, binder free variables, and a
    conclusion) into the owned `StatementShape`. Shared by the declaration and
    proof-goal paths; private because both entry points live in this file. -/
private def statementOfTelescope
    (levelParams : List Name) (fvars : Array Expr) (conclusion : Expr) :
    MetaM StatementShape := do
  let params := levelParamMap levelParams
  let mut binders := #[]
  let mut index := 0
  for fvar in fvars do
    let localDecl ← fvar.fvarId!.getDecl
    let type := localDecl.type
    binders := binders.push
      { index
        fvarName := fvar.fvarId!.name
        info := localDecl.binderInfo
        type := translateExpr params type
        deps := dependencies type fvars
        isProp := ← Meta.isProp type
        headConst? := headConst? type
        usedConsts := usedConstants type }
    index := index + 1
  pure
    { binders
      conclusion := translateExpr params conclusion
      conclusionHead? := headConst? conclusion
      conclusionConsts := usedConstants conclusion }

/-- Translate a declaration's signature into a `StatementShape`. -/
def statementOfConstant (info : ConstantInfo) : MetaM StatementShape :=
  forallTelescope info.type fun fvars conclusion =>
    statementOfTelescope info.levelParams fvars conclusion

/-- Translate an open proof goal (its hypotheses and target) into a
    `StatementShape`. Goals carry no level parameters of their own. -/
def statementOfGoal (fvars : Array Expr) (target : Expr) : MetaM StatementShape :=
  statementOfTelescope [] fvars target

end LeanSemanticSearch.LeanCompat
