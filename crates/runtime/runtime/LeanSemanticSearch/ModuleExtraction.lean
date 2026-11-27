import LeanSemanticSearch.Json
import LeanSemanticSearch.LeanCompat

/-!
Request parsing for the declaration feature command.

This module parses the `modules`/options request. The module import,
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
  deriving Repr

structure Options where
  includePrivate : Bool
  includeGenerated : Bool
  deriving Repr

private def parseModuleSpec (json : Json) : Except JsonSupport.Error ModuleSpec := do
  let moduleName ← JsonSupport.requiredString json "module"
  let origin? ← JsonSupport.optionalString json "origin"
  pure { module := moduleName, origin := origin?.getD moduleName }

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

/-- Import the requested modules and return the accepted declarations as owned
    `DeclSource` records. All `MetaM` work happens behind the boundary, so the
    row builder never touches the compiler. -/
unsafe def declSources (payload : Json) (modules : Array ModuleSpec) :
    IO (Except JsonSupport.Error (Array DeclSource)) := do
  match parseOptions payload with
  | .error err => pure <| .error err
  | .ok options =>
      match ← LeanCompat.collectDeclSources (importRequestOf options modules) with
      | .error compatErr => pure <| .error (JsonSupport.Error.ofCompat compatErr)
      | .ok sources => pure <| .ok sources

end LeanSemanticSearch.ModuleExtraction
