import LeanSemanticSearch.Json
import LeanSemanticSearch.LeanCompat

/-!
Request parsing and timing for the declaration feature command.

This module parses the `modules`/options request and times extraction. The
environment traversal, declaration classification, and source-range lookup live
behind `LeanCompat.collectDeclSources`; building semantic rows from the returned
`DeclSource`s is left to `DeclarationFeatures`, and retrieval policy stays
downstream entirely.
-/

namespace LeanSemanticSearch.ModuleExtraction

open Lean (Json)
open LeanSemanticSearch.LeanCompat (DeclSource ImportRequest ModuleRef SourceSpan)

structure ModuleSpec where
  module : String
  origin : String
  -- Reserved contract field (`source_root`, see crates/contract): accepted so
  -- callers can supply it, but not yet consumed — declaration ranges come from
  -- the imported environment, not from re-reading source. Kept for the planned
  -- source-aware extraction; do not assume it is wired up.
  sourceRoot? : Option String := none
  deriving Repr

structure Options where
  includePrivate : Bool
  includeGenerated : Bool
  deriving Repr

structure RunStats where
  importMs : Nat
  semanticMs : Nat
  declarationCount : Nat
  rowCount : Nat

structure RunOutput where
  rows : Array Json
  stats : RunStats

private def parseModuleSpec (json : Json) : Except JsonSupport.Error ModuleSpec := do
  let moduleName ← JsonSupport.requiredString json "module"
  let origin? ← JsonSupport.optionalString json "origin"
  let sourceRoot? ← JsonSupport.optionalString json "source_root"
  pure { module := moduleName, origin := origin?.getD moduleName, sourceRoot? }

def parseModules (payload : Json) : Except JsonSupport.Error (Array ModuleSpec) := do
  match JsonSupport.optionalField payload "modules" with
  | some (Json.arr values) =>
      let mut modules := #[]
      for value in values do
        modules := modules.push (← parseModuleSpec value)
      if modules.isEmpty then
        throw <| JsonSupport.invalidRequest "`modules` must contain at least one module"
      pure modules
  | some _ => throw <| JsonSupport.invalidRequest "`modules` must be an array"
  | none => throw <| JsonSupport.invalidRequest "missing required array field `modules`"

def parseOptions (payload : Json) : Except JsonSupport.Error Options := do
  let includePrivate ← JsonSupport.boolField payload "include_private" false
  let includeGenerated ← JsonSupport.boolField payload "include_generated" false
  pure { includePrivate, includeGenerated }

/-- Render a source span (already in the package-owned form) as JSON. -/
def sourceSpanJson? (range? : Option SourceSpan) : Option Json :=
  range?.map fun span =>
    JsonSupport.sourceSpanJson span.startLine span.startColumn span.endLine span.endColumn

private def importRequestOf (options : Options) (modules : Array ModuleSpec) : ImportRequest :=
  { modules := modules.map fun spec => ({ module := spec.module, origin := spec.origin } : ModuleRef)
    includePrivate := options.includePrivate
    includeGenerated := options.includeGenerated }

/-- Import the requested modules, hand the extracted `DeclSource`s to `operation`,
    and report timing. The operation is pure: all `MetaM` work happens behind the
    boundary, so the row builder never touches the compiler. -/
unsafe def withDeclSourcesProfiled {α : Type}
    (payload : Json)
    (modules : Array ModuleSpec)
    (operation : Options → Array DeclSource → Except JsonSupport.Error α) :
    IO (Except JsonSupport.Error (α × RunStats)) := do
  match parseOptions payload with
  | .error err => pure <| .error err
  | .ok options =>
      let importStarted ← IO.monoMsNow
      match ← LeanCompat.collectDeclSources (importRequestOf options modules) with
      | .error compatErr => pure <| .error (JsonSupport.Error.ofCompat compatErr)
      | .ok declSources =>
          let importFinished ← IO.monoMsNow
          match operation options declSources with
          | .error err => pure <| .error err
          | .ok result =>
              let semanticFinished ← IO.monoMsNow
              let stats : RunStats :=
                { importMs := importFinished - importStarted
                  semanticMs := semanticFinished - importFinished
                  declarationCount := declSources.size
                  rowCount := 0 }
              pure <| .ok (result, stats)

end LeanSemanticSearch.ModuleExtraction
