import Lean
import Lean.Server.InfoUtils
import LeanSemanticSearch.Json

/-!
Module import and declaration selection for semantic feature commands.

This module hides Lean environment traversal and declaration filtering. It does
not emit semantic rows and does not know downstream retrieval policy.
-/

namespace LeanSemanticSearch.ModuleExtraction

open Lean
open Lean.Meta

structure ModuleSpec where
  module : String
  origin : String
  sourceRoot? : Option String := none
  deriving Repr

structure Options where
  workspaceRoot? : Option String
  includePrivate : Bool
  includeGenerated : Bool
  deriving Repr

structure Context where
  modules : Array ModuleSpec
  options : Options

structure RunStats where
  importMs : Nat
  semanticMs : Nat
  declarationCount : Nat
  rowCount : Nat

structure RunOutput where
  rows : Array Json
  stats : RunStats

structure AcceptedDeclaration where
  moduleSpec : ModuleSpec
  declName : Name
  constInfo : ConstantInfo
  generated : Bool
  range? : Option DeclarationRanges

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
  let workspaceRoot? ← JsonSupport.optionalString payload "workspace_root"
  let includePrivate ← JsonSupport.boolField payload "include_private" false
  let includeGenerated ← JsonSupport.boolField payload "include_generated" false
  pure { workspaceRoot?, includePrivate, includeGenerated }

def dottedName (text : String) : Name :=
  (text.splitOn ".").foldl
    (init := Name.anonymous)
    fun current segment =>
      if segment.isEmpty then current else Name.str current segment

private def shortName : Name → String
  | .anonymous => "_anonymous"
  | .str _ segment => segment
  | .num parent _ => shortName parent

private def visibility (declName : Name) : String :=
  if isPrivateName declName then "private" else "public"

private def generatedShortNames : Array String :=
  #[ "rec"
   , "recOn"
   , "casesOn"
   , "noConfusion"
   , "noConfusionType"
   , "below"
   , "brecOn"
   , "ibelow"
   , "binductionOn"
   , "ctorElim"
   , "elim"
   ]

private def generatedNameShape (declName : Name) : Bool :=
  let text := declName.toString
  let short := shortName declName
  generatedShortNames.contains short ||
    text.contains "._aux_" ||
    text.contains "._unexpand_" ||
    text.contains "._macroRules_" ||
    text.contains ".match_" ||
    text.contains ".proof_" ||
    (short.startsWith "_aux_")

private def isGeneratedDeclaration (declName : Name) : MetaM Bool := do
  let env ← getEnv
  if env.isProjectionFn declName then
    return false
  if isPrivateName declName then
    return false
  let isRecursor ← isRec declName
  let isMatcherDecl ← Lean.Meta.isMatcher declName
  let isMatcherLikeDecl ← Lean.Meta.isMatcherLike declName
  pure <|
    isAuxRecursor env declName ||
      isNoConfusion env declName ||
      isRecursor ||
      isMatcherDecl ||
      isMatcherLikeDecl ||
      declName.isInternal ||
      declName.isInternalDetail ||
      generatedNameShape declName

/-- Build the opaque declaration id shared by declaration and feature rows. -/
def declarationId (moduleSpec : ModuleSpec) (declName : Name) : String :=
  s!"{moduleSpec.origin}:{moduleSpec.module}:{declName}"

def AcceptedDeclaration.declarationId (decl : AcceptedDeclaration) : String :=
  LeanSemanticSearch.ModuleExtraction.declarationId decl.moduleSpec decl.declName

def sourceSpanJson? (_options : Options) (_moduleSpec : ModuleSpec)
    (range? : Option DeclarationRanges) : Option Json :=
  range?.map fun ranges =>
    JsonSupport.sourceSpanJson
      ranges.range.pos.line
      ranges.range.pos.column
      ranges.range.endPos.line
      ranges.range.endPos.column

private def collectModuleDeclarations (context : Context) (moduleSpec : ModuleSpec) :
    MetaM (Array AcceptedDeclaration) := do
  let env ← getEnv
  let moduleName := dottedName moduleSpec.module
  let some moduleIdx := env.header.moduleNames.idxOf? moduleName | return #[]
  let moduleData := env.header.moduleData[moduleIdx]!
  let mut declarations := #[]
  for declName in moduleData.constNames do
    let some constInfo := env.find? declName | continue
    let range? ← findDeclarationRanges? declName
    let generatedByShape ← isGeneratedDeclaration declName
    let generated := generatedByShape || range?.isNone
    if generated && !context.options.includeGenerated then
      continue
    if visibility declName == "private" && !context.options.includePrivate then
      continue
    declarations :=
      declarations.push
        { moduleSpec
          declName
          constInfo
          generated
          range? }
  pure declarations

def collectAcceptedDeclarations (context : Context) : MetaM (Array AcceptedDeclaration) := do
  let mut declarations := #[]
  for moduleSpec in context.modules do
    let moduleDeclarations ← collectModuleDeclarations context moduleSpec
    for declaration in moduleDeclarations do
      declarations := declarations.push declaration
  pure declarations

private def moduleArrayJson (modules : Array ModuleSpec) : Json :=
  Json.arr (modules.map fun moduleSpec => Json.str moduleSpec.module)

private def uniqueModuleImports (modules : Array ModuleSpec) : Array Import := Id.run do
  let mut seen : Std.HashSet String := {}
  let mut imports := #[]
  for moduleSpec in modules do
    if !seen.contains moduleSpec.module then
      seen := seen.insert moduleSpec.module
      imports := imports.push ({ module := dottedName moduleSpec.module } : Import)
  imports

unsafe def importRequestedModules (modules : Array ModuleSpec) :
    IO (Except JsonSupport.Error Environment) := do
  Lean.enableInitializersExecution
  initSearchPath (← getBuildDir)
  let imports := uniqueModuleImports modules
  try
    let env ← importModules imports Options.empty (loadExts := true)
    pure <| .ok env
  catch error =>
    pure <|
      .error
        (JsonSupport.importFailed
          s!"could not import requested modules: {error}"
          (some <| Json.mkObj [("modules", moduleArrayJson modules)]))

unsafe def withAcceptedDeclarationsProfiled {α : Type}
    (payload : Json)
    (modules : Array ModuleSpec)
    (operation : Options → Array AcceptedDeclaration → MetaM α) :
    IO (Except JsonSupport.Error (α × RunStats)) := do
  match parseOptions payload with
  | .error err => pure <| .error err
  | .ok options =>
      let importStarted ← IO.monoMsNow
      match ← importRequestedModules modules with
      | .error err => pure <| .error err
      | .ok env =>
          let importFinished ← IO.monoMsNow
          let context : Context := { modules, options }
          let coreContext : Core.Context :=
            { fileName := "<lean-semantic-search-module-extraction>"
              fileMap := default
              options := Options.empty }
          try
            let semanticStarted ← IO.monoMsNow
            let (result, _, _) ←
              MetaM.toIO
                (do
                  let declarations ← collectAcceptedDeclarations context
                  let result ← operation options declarations
                  pure (result, declarations.size))
                coreContext
                { env := env }
                {}
                {}
            let semanticFinished ← IO.monoMsNow
            let stats : RunStats :=
              { importMs := importFinished - importStarted
                semanticMs := semanticFinished - semanticStarted
                declarationCount := result.2
                rowCount := 0 }
            pure <| .ok (result.1, stats)
          catch error =>
            pure <|
              .error
                (JsonSupport.internalError s!"declaration processing failed: {error}")

end LeanSemanticSearch.ModuleExtraction
