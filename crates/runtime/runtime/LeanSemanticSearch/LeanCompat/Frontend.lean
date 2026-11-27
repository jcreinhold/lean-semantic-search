import Lean
import Lean.Elab.InfoTree.Util
import LeanSemanticSearch.LeanCompat.Shape
import LeanSemanticSearch.LeanCompat.Translate

/-!
The side-effecting boundary: module import, environment enumeration, declaration
classification, source elaboration, and proof-goal selection. This is the only
module that names the volatile import/elaboration/info-tree/environment surface
(`importModules`, `findDeclarationRanges?`, `Environment.header`, `InfoTree`,
`ContextInfo`, `MetavarContext`, the parser/frontend pipeline). It returns owned
`DeclSource`/`GoalSnapshot` records and `CompatError`; nothing throws or leaks a
`Lean.Expr` across it.
-/

namespace LeanSemanticSearch.LeanCompat

open Lean
open Lean.Elab
open Lean.Meta

/-! ## Declaration extraction -/

private def dottedName (text : String) : Name :=
  (text.splitOn ".").foldl
    (init := Name.anonymous)
    fun current segment =>
      if segment.isEmpty then current else Name.str current segment

private def shortName : Name → String
  | .anonymous => "_anonymous"
  | .str _ segment => segment
  | .num parent _ => shortName parent

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

private def spanOfRanges (ranges : DeclarationRanges) : SourceSpan :=
  { startLine := ranges.range.pos.line
    startColumn := ranges.range.pos.column
    endLine := ranges.range.endPos.line
    endColumn := ranges.range.endPos.column }

private def declarationId (origin module : String) (declName : Name) : String :=
  s!"{origin}:{module}:{declName}"

private def collectModuleDeclSources (req : ImportRequest) (moduleRef : ModuleRef) :
    MetaM (Array DeclSource) := do
  let env ← getEnv
  let moduleName := dottedName moduleRef.module
  let some moduleIdx := env.header.moduleNames.idxOf? moduleName | return #[]
  let moduleData := env.header.moduleData[moduleIdx]!
  let mut sources := #[]
  for declName in moduleData.constNames do
    let some constInfo := env.find? declName | continue
    let range? ← findDeclarationRanges? declName
    let generatedByShape ← isGeneratedDeclaration declName
    let generated := generatedByShape || range?.isNone
    if generated && !req.includeGenerated then
      continue
    if isPrivateName declName && !req.includePrivate then
      continue
    let statement ← statementOfConstant constInfo
    sources := sources.push
      { declarationId := declarationId moduleRef.origin moduleRef.module declName
        statement
        range? := range?.map spanOfRanges
        generated }
  pure sources

private def collectAllDeclSources (req : ImportRequest) : MetaM (Array DeclSource) := do
  let mut sources := #[]
  for moduleRef in req.modules do
    sources := sources ++ (← collectModuleDeclSources req moduleRef)
  pure sources

private def uniqueModuleImports (modules : Array ModuleRef) : Array Import := Id.run do
  let mut seen : Std.HashSet String := {}
  let mut imports := #[]
  for moduleRef in modules do
    if !seen.contains moduleRef.module then
      seen := seen.insert moduleRef.module
      imports := imports.push ({ module := dottedName moduleRef.module } : Import)
  imports

private def moduleNamesJson (modules : Array ModuleRef) : Json :=
  Json.arr (modules.map fun moduleRef => Json.str moduleRef.module)

/-- Import the requested modules and enumerate accepted declarations as owned
    `DeclSource` records. The caller must install Lean's module search path
    before invoking this function. This function imports against the current
    search path and never calls `initSearchPath` or rebuilds the path from
    `LEAN_PATH`. Hides the environment layout, declaration-range lookup, the
    generated/private classification, and the `Expr`→`StatementShape`
    translation. -/
unsafe def collectDeclSources (req : ImportRequest) : IO (Except CompatError (Array DeclSource)) := do
  Lean.enableInitializersExecution
  let imports := uniqueModuleImports req.modules
  let env? ←
    try
      pure (Except.ok (← importModules imports Options.empty (loadExts := true)))
    catch error =>
      pure <|
        Except.error <|
          CompatError.importFailed
            s!"could not import requested modules: {error}"
            (some <| Json.mkObj [("modules", moduleNamesJson req.modules)])
  match env? with
  | .error err => pure (.error err)
  | .ok env =>
      let coreContext : Core.Context :=
        { fileName := "<lean-semantic-search-module-extraction>"
          fileMap := default
          options := Options.empty }
      try
        let (sources, _, _) ←
          MetaM.toIO (collectAllDeclSources req) coreContext { env := env } {} {}
        pure (.ok sources)
      catch error =>
        pure <| .error <| CompatError.internalError s!"declaration processing failed: {error}"

/-! ## Proof-goal selection -/

private structure TacticCandidate where
  span : SourceSpan
  ctx : ContextInfo
  mctxBefore : MetavarContext
  goalsBefore : List MVarId

private def rangeOfStx (fileMap : FileMap) (stx : Syntax) : Option SourceSpan :=
  match stx.getRange? with
  | none => none
  | some ⟨sp, ep⟩ =>
      let s := fileMap.toPosition sp
      let e := fileMap.toPosition ep
      some
        { startLine := s.line
          startColumn := s.column + 1
          endLine := e.line
          endColumn := e.column + 1 }

private def rangeContains (span : SourceSpan) (line column : Nat) : Bool :=
  if line < span.startLine || line > span.endLine then
    false
  else if line == span.startLine && column < span.startColumn then
    false
  else if line == span.endLine && column > span.endColumn then
    false
  else
    true

-- A size score for tie-breaking overlapping tactic spans: the *smallest*
-- enclosing span is the most specific match for a position. Line difference
-- dominates column difference (one line always outweighs any column delta), so
-- line span is weighted by a constant larger than any realistic column count
-- rather than comparing lines and columns as separate keys.
private def rangeArea (span : SourceSpan) : Nat :=
  let lineSpan := span.endLine - span.startLine
  let colSpan := span.endColumn - span.startColumn
  lineSpan * 1000000 + colSpan

private def goalShortName : String → String
  | text =>
      match text.splitOn "." |>.reverse with
      | head :: _ => head
      | [] => text

private def parentDeclMatches (wanted : String) (actual? : Option Name) : Bool :=
  match actual? with
  | none => false
  | some actual =>
      let actualText := actual.toString
      actualText == wanted || goalShortName actualText == wanted

private def spanLess (left right : SourceSpan) : Bool :=
  left.startLine < right.startLine ||
    (left.startLine == right.startLine && left.startColumn < right.startColumn) ||
    (left.startLine == right.startLine && left.startColumn == right.startColumn &&
      rangeArea left < rangeArea right)

private def collectTacticCandidates (fileMap : FileMap) (trees : PersistentArray InfoTree) :
    IO (Array TacticCandidate) := do
  let mut tactics := #[]
  for tree in trees do
    tactics ← tree.foldInfoM (init := tactics) fun ctx info acc => do
      match info with
      | .ofTacticInfo ti =>
          match rangeOfStx fileMap ti.stx with
          | some span =>
              pure <| acc.push
                { span
                  ctx
                  mctxBefore := ti.mctxBefore
                  goalsBefore := ti.goalsBefore }
          | none => pure acc
      | _ => pure acc
  pure <| tactics.qsort fun left right => spanLess left.span right.span

private def selectCandidate
    (req : GoalRequest)
    (candidates : Array TacticCandidate) :
    Except CompatError (String × TacticCandidate) := do
  let byDeclaration : Array TacticCandidate :=
    match req.declaration? with
    | some declaration =>
        candidates.filter fun candidate => parentDeclMatches declaration candidate.ctx.parentDecl?
    | none => candidates
  let byPosition : Array TacticCandidate :=
    match req.position? with
    | some position =>
        byDeclaration.filter fun candidate =>
          rangeContains candidate.span position.line position.column
    | none => byDeclaration
  match byPosition[0]? with
  | some candidate =>
      let span := candidate.span
      let goalId :=
        s!"{req.module}:{span.startLine}:{span.startColumn}:{span.endLine}:{span.endColumn}"
      pure (goalId, candidate)
  | none =>
      throw <|
        CompatError.proofGoalUnavailable
          "no proof goal matched the requested source selector"
          (some <|
            Json.mkObj
              [ ("module", Json.str req.module)
              , ("declaration", req.declaration?.map Json.str |>.getD Json.null)
              ])

private def localFVars (lctx : LocalContext) : Array Expr :=
  lctx.foldl
    (init := #[])
    fun acc localDecl =>
      if localDecl.isImplementationDetail then
        acc
      else
        acc.push localDecl.toExpr

private def snapshotFromCandidate (goalId : String) (candidate : TacticCandidate) :
    IO (Except CompatError GoalSnapshot) := do
  try
    let statement ←
      ({ candidate.ctx with mctx := candidate.mctxBefore }).runMetaM {} do
        setMCtx candidate.mctxBefore
        match candidate.goalsBefore with
        | goal :: _ =>
            goal.withContext do
              let decl ← goal.getDecl
              statementOfGoal (localFVars decl.lctx) decl.type
        | [] =>
            throwError "selected tactic has no goals before execution"
    pure <| .ok { goalId, statement }
  catch error =>
    pure <|
      .error
        (CompatError.proofGoalUnavailable s!"selected proof goal could not be inspected: {error}")

/-- The first eight messages `lean` would print, as JSON. Like `lean`, this skips
    the messages Lean marks silent, such as a formatter plugin's records. -/
private def messageDiagnosticsJson (messages : MessageLog) : IO Json := do
  let mut out := #[]
  for message in messages.toArray.filter (!·.isSilent) do
    if out.size >= 8 then
      break
    let text ← message.data.toString
    out := out.push <|
      Json.mkObj
        [ ("message", Json.str text)
        , ("line", Json.num message.pos.line)
        , ("column", Json.num message.pos.column)
        ]
  pure <| Json.arr out

/-- Elaborate source with the exact module setup that Lake selected for it. The
    caller runs this in a fresh, separately supervised Lean process, keeps the
    setup's artifacts and native images in place for the whole call, and chooses
    the working directory and search path; relative setup paths keep Lake's
    meaning. This function loads the setup's dynlibs and plugins and uses its
    imports, artifact map, options, package and module mode, with no fallback to
    the source header or an ambient cache. It never calls `initSearchPath`. It
    elaborates the whole file, header included, as `lean` does, so goal spans
    and diagnostics carry file positions. -/
unsafe def selectGoalSnapshotWithSetup (setup : ModuleSetup) (req : GoalRequest) :
    IO (Except CompatError GoalSnapshot) := do
  if setup.name != req.module.toName then
    return .error <| CompatError.invalidRequest
      s!"Lake setup module {setup.name} differs from requested module {req.module}"
  let opts := setup.options.toOptions
  let inputCtx := Parser.mkInputContext req.sourceText req.fileLabel
  let (header, parserState, headerMessages) ← Parser.parseHeader inputCtx
  if headerMessages.hasErrors then
    return .error <| CompatError.invalidRequest "could not parse source header"
  try
    Lean.enableInitializersExecution
    setup.dynlibs.forM Lean.loadDynlib
    let (commandEnv, initialMessages) ←
      Elab.processHeaderCore (Elab.HeaderSyntax.startPos header)
        (setup.imports?.getD (Elab.HeaderSyntax.imports header))
        (setup.isModule || Elab.HeaderSyntax.isModule header) opts headerMessages inputCtx
        (plugins := setup.plugins) (mainModule := setup.name) (package? := setup.package?)
        (arts := setup.importArts) (headerStx? := some header)
    if initialMessages.hasErrors then
      return .error <|
        CompatError.importFailed "Lake-selected source imports failed"
          (some <| Json.mkObj [("messages", (← messageDiagnosticsJson initialMessages))])
    let mut commandState := Command.mkState commandEnv initialMessages opts
    commandState := { commandState with infoState.enabled := true }
    if let some namespaceText := req.namespaceName? then
      let head := commandState.scopes.headD { header := "", opts }
      commandState := { commandState with scopes := [{ head with currNamespace := namespaceText.toName }] }
    -- Elaborate the rest of the whole file, as `lean` does: elaborators and
    -- linters see the header, and every position is a file position.
    let finalState ← Elab.IO.processCommands inputCtx parserState commandState
    if finalState.commandState.messages.hasErrors then
      return .error <|
        CompatError.invalidRequest
          "source elaboration produced errors"
          (some <| Json.mkObj [("messages", (← messageDiagnosticsJson finalState.commandState.messages))])
    let candidates ← collectTacticCandidates req.sourceText.toFileMap finalState.commandState.infoState.trees
    match selectCandidate req candidates with
    | .error err => pure <| .error err
    | .ok (goalId, candidate) => snapshotFromCandidate goalId candidate
  catch error =>
    pure <|
      .error
        (CompatError.internalError s!"proof-goal source elaboration failed: {error}")

end LeanSemanticSearch.LeanCompat
