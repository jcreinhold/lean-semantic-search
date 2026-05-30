import Lean

/-!
Package-owned intermediate representation for semantic feature extraction.

These types are the stable vocabulary the rest of the package speaks. They are a
*lossy, structural projection* of Lean's compiler types (`Expr`, `Level`,
`LocalDecl`, `DeclarationRanges`): they keep exactly what the canonical
fingerprint and the role features read, and nothing more. The translation from
Lean core types lives in `LeanCompat.Translate`; nothing outside `LeanCompat`
constructs these from a `Lean.Expr`.

Why this exists: every cross-version break in this package came from feature
logic reaching directly into volatile compiler internals. Once feature logic is
expressed over these owned, closed types, a Lean core API change can only break
the single translator — never the fingerprint algorithm, the role assignment, or
the JSON envelope. A new `Expr`/`Level` constructor produces exactly one loud,
localized error in `Translate`, never a silent miscompute here.

Stable leaf types (`Lean.Name`, `Lean.BinderInfo`) are kept as-is: they are
ubiquitous, have been stable across many releases, and re-wrapping them would add
indirection without hiding anything volatile.
-/

namespace LeanSemanticSearch.LeanCompat

/-- A universe level, with parameter references already resolved against the
    enclosing declaration's level parameters. `paramOrdinal` is the position of a
    bound level parameter; `paramName` is a parameter not in scope (the fallback
    the serializer renders as `p:{name}`). -/
inductive LevelShape where
  | zero
  | succ (level : LevelShape)
  | max (left right : LevelShape)
  | imax (left right : LevelShape)
  | paramOrdinal (index : Nat)
  | paramName (name : Lean.Name)
  | mvar (name : Lean.Name)
  deriving Inhabited

/-- The fingerprint-relevant skeleton of an expression. Faithful to the
    structure the serializer walks: application spines are *not* pre-flattened
    here at the variant level — `app` carries a non-application head plus its
    argument vector, exactly as the canonical traversal consumes them.

    `fvar`/`mvar` carry the underlying `FVarId`/`MVarId` *name* (the stable
    identity the serializer keys on), not the Lean wrapper. `mdata` is retained
    as a node (carrying no payload) so the serializer's node/depth budget
    accounting matches the original traversal exactly. -/
inductive ExprShape where
  | bvar (index : Nat)
  | fvar (name : Lean.Name)
  | mvar (name : Lean.Name)
  | sort (level : LevelShape)
  | const (name : Lean.Name) (levels : Array LevelShape)
  | app (head : ExprShape) (args : Array ExprShape)
  | lam (info : Lean.BinderInfo) (domain body : ExprShape)
  | forallE (info : Lean.BinderInfo) (domain body : ExprShape)
  | letE (type value body : ExprShape)
  | natLit (value : Nat)
  | strLit (value : String)
  | mdata (body : ExprShape)
  | proj (typeName : Lean.Name) (index : Nat) (struct : ExprShape)
  deriving Inhabited

/-- One binder of a statement telescope, fully translated. The translator
    precomputes everything the feature logic needs so that downstream code is
    pure: `type` for the canonical key and binder scheduling, `deps` (which
    earlier binders this binder's type mentions) for dependency-safe scheduling,
    and `isProp`/`headConst?`/`usedConsts` for role-feature assignment.
    `fvarName` is the binder's own free-variable identity, used to resolve its
    ordinal during serialization. -/
structure BinderShape where
  /-- Original telescope position. Stable identity referenced by `deps` and by
      dependency-safe scheduling, which reorders binders away from this position. -/
  index : Nat
  fvarName : Lean.Name
  info : Lean.BinderInfo
  type : ExprShape
  deps : Array Nat
  isProp : Bool
  headConst? : Option Lean.Name
  usedConsts : Array Lean.Name

/-- A declaration statement or proof goal, translated to owned types. Carries
    both the data the canonical fingerprint needs (binders + conclusion shapes)
    and the data the role features need (heads + used constants), because both
    are produced by the one telescope traversal that shares them. -/
structure StatementShape where
  binders : Array BinderShape
  conclusion : ExprShape
  conclusionHead? : Option Lean.Name
  conclusionConsts : Array Lean.Name

/-- A source span in 1-based line/column coordinates. Replaces Lean's
    `DeclarationRanges` and the inline span struct the goal path used, so neither
    leaks past the boundary. -/
structure SourceSpan where
  startLine : Nat
  startColumn : Nat
  endLine : Nat
  endColumn : Nat
  deriving Repr

/-- A declaration selected for extraction. No `ConstantInfo`, `Environment`, or
    `DeclarationRanges` crosses this boundary. -/
structure DeclSource where
  declarationId : String
  statement : StatementShape
  range? : Option SourceSpan
  generated : Bool

/-- A proof goal selected from elaborated source. -/
structure GoalSnapshot where
  goalId : String
  statement : StatementShape

/-- A module to import and extract, with the origin label used to build opaque
    declaration ids. -/
structure ModuleRef where
  module : String
  origin : String

/-- Everything the declaration-extraction boundary needs: which modules to
    import and the private/generated inclusion policy. -/
structure ImportRequest where
  modules : Array ModuleRef
  includePrivate : Bool
  includeGenerated : Bool

/-- A 1-based source position used to select a proof goal. -/
structure GoalPosition where
  line : Nat
  column : Nat

/-- Everything the proof-goal boundary needs: the source to elaborate, a file
    label for diagnostics, and the optional declaration / position / namespace
    selectors that pick which tactic state to snapshot. -/
structure GoalRequest where
  module : String
  sourceText : String
  fileLabel : String
  declaration? : Option String
  position? : Option GoalPosition
  namespaceName? : Option String

/-- Errors the boundary can report, mirroring the categories the JSON layer
    renders. There is deliberately no "unknown shape" variant: `Translate`
    matches `Expr`/`Level` exhaustively, so an unrecognized core constructor is a
    compile error there, not a runtime error here — nothing constructs such a
    case. Errors are returned, never thrown across the boundary. -/
inductive CompatError where
  | invalidRequest (message : String) (details? : Option Lean.Json := none)
  | importFailed (message : String) (details? : Option Lean.Json := none)
  | proofGoalUnavailable (message : String) (details? : Option Lean.Json := none)
  | internalError (message : String) (details? : Option Lean.Json := none)

end LeanSemanticSearch.LeanCompat
