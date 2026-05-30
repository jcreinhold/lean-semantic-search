import LeanSemanticSearch.GoalFeatures
import LeanSemanticSearch.ModuleExtraction

/-!
Source-backed proof-goal extraction.

The exported command passes source text and a selector. This module elaborates
the source, selects a tactic proof state, and exposes only semantic feature rows
derived from Lean expressions and local declarations.
-/

namespace LeanSemanticSearch.GoalElaboration

open Lean
open Lean.Elab
open Lean.Meta

structure SourcePosition where
  line : Nat
  column : Nat
  deriving Repr

structure Request where
  module : String
  sourceText : String
  fileLabel : String
  declaration? : Option String
  position? : Option SourcePosition
  namespace? : Option String

private structure SourceSpan where
  startLine : Nat
  startColumn : Nat
  endLine : Nat
  endColumn : Nat

private structure SourceDocument where
  source : String
  fileMap : FileMap
  bodyFileMap : FileMap
  lineOffset : Nat

private structure TacticCandidate where
  span : SourceSpan
  ctx : ContextInfo
  mctxBefore : MetavarContext
  goalsBefore : List MVarId

private def SourceDocument.fromSources (source bodySource : String) (lineOffset : Nat) :
    SourceDocument :=
  { source, fileMap := source.toFileMap, bodyFileMap := bodySource.toFileMap, lineOffset }

private def SourceDocument.fileLineToBody (doc : SourceDocument) (line : Nat) : Nat :=
  if line > doc.lineOffset then line - doc.lineOffset else 0

private def SourceDocument.bodySpanToFile (doc : SourceDocument) (span : SourceSpan) : SourceSpan :=
  { span with startLine := span.startLine + doc.lineOffset, endLine := span.endLine + doc.lineOffset }

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

-- Order spans by size so the tightest enclosing range wins. Line difference
-- dominates; the large multiplier keeps any column difference from outranking
-- it (no source line is a million columns wide).
private def rangeArea (span : SourceSpan) : Nat :=
  let lineSpan := span.endLine - span.startLine
  let colSpan := span.endColumn - span.startColumn
  lineSpan * 1000000 + colSpan

private def shortName : String → String
  | text =>
      match text.splitOn "." |>.reverse with
      | head :: _ => head
      | [] => text

-- Accept either the fully-qualified name or its final component, so a caller can
-- name a declaration as `t` or `My.Project.t` and still match.
private def parentDeclMatches (wanted : String) (actual? : Option Name) : Bool :=
  match actual? with
  | none => false
  | some actual =>
      let actualText := actual.toString
      actualText == wanted || shortName actualText == wanted

private def spanLess (left right : SourceSpan) : Bool :=
  left.startLine < right.startLine ||
    (left.startLine == right.startLine && left.startColumn < right.startColumn) ||
    (left.startLine == right.startLine && left.startColumn == right.startColumn &&
      rangeArea left < rangeArea right)

private def collectTacticCandidates (doc : SourceDocument) (trees : PersistentArray InfoTree) :
    IO (Array TacticCandidate) := do
  let mut tactics := #[]
  for tree in trees do
    tactics ← tree.foldInfoM (init := tactics) fun ctx info acc => do
      match info with
      | .ofTacticInfo ti =>
          match rangeOfStx doc.bodyFileMap ti.stx with
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
    (doc : SourceDocument)
    (request : Request)
    (candidates : Array TacticCandidate) :
    Except JsonSupport.Error (String × TacticCandidate) := do
  let byDeclaration : Array TacticCandidate :=
    match request.declaration? with
    | some declaration =>
        candidates.filter fun (candidate : TacticCandidate) => parentDeclMatches declaration candidate.ctx.parentDecl?
    | none => candidates
  let byPosition : Array TacticCandidate :=
    match request.position? with
    | some position =>
        let bodyLine := doc.fileLineToBody position.line
        byDeclaration.filter fun (candidate : TacticCandidate) =>
          rangeContains candidate.span bodyLine position.column
    | none => byDeclaration
  match byPosition[0]? with
  | some candidate =>
      let fileSpan := doc.bodySpanToFile candidate.span
      let goalId :=
        s!"{request.module}:{fileSpan.startLine}:{fileSpan.startColumn}:{fileSpan.endLine}:{fileSpan.endColumn}"
      pure (goalId, candidate)
  | none =>
      throw <|
        JsonSupport.proofGoalUnavailable
          "no proof goal matched the requested source selector"
          (some <|
            Json.mkObj
              [ ("module", Json.str request.module)
              , ("declaration", request.declaration?.map Json.str |>.getD Json.null)
              ])

private def localFVars (lctx : LocalContext) : Array Expr :=
  lctx.foldl
    (init := #[])
    fun acc localDecl =>
      if localDecl.isImplementationDetail then
        acc
      else
        acc.push localDecl.toExpr

private def rowFromCandidate (goalId : String) (candidate : TacticCandidate) :
    IO (Except JsonSupport.Error Json) := do
  try
    let row ←
      ({ candidate.ctx with mctx := candidate.mctxBefore }).runMetaM {} do
        setMCtx candidate.mctxBefore
        match candidate.goalsBefore with
        | goal :: _ =>
            goal.withContext do
              let decl ← goal.getDecl
              GoalFeatures.rowFromGoal goalId (localFVars decl.lctx) decl.type
        | [] =>
            throwError "selected tactic has no goals before execution"
    pure <| .ok row
  catch error =>
    pure <|
      .error
        (JsonSupport.proofGoalUnavailable s!"selected proof goal could not be inspected: {error}")

private def countNewlines (s : String) : Nat :=
  s.foldl (init := 0) fun n c => if c == '\n' then n + 1 else n

private def messageDiagnosticsJson (messages : MessageLog) : IO Json := do
  let mut out := #[]
  for message in messages.toArray do
    -- Cap the diagnostics carried in the envelope; a failing elaboration can
    -- produce many messages, and the first few suffice to explain the failure.
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

private def parsePosition (payload : Json) : Except JsonSupport.Error (Option SourcePosition) := do
  match JsonSupport.optionalField payload "position" with
  | none | some Json.null => pure none
  | some positionJson =>
      let line ←
        match JsonSupport.optionalField positionJson "line" with
        | some value =>
            match value.getNat? with
            | .ok n => pure n
            | .error _ => throw <| JsonSupport.invalidRequest "`position.line` must be a natural number"
        | none => throw <| JsonSupport.invalidRequest "`position.line` is required"
      let column ←
        match JsonSupport.optionalField positionJson "column" with
        | some value =>
            match value.getNat? with
            | .ok n => pure n
            | .error _ => throw <| JsonSupport.invalidRequest "`position.column` must be a natural number"
        | none => throw <| JsonSupport.invalidRequest "`position.column` is required"
      if line == 0 || column == 0 then
        throw <| JsonSupport.invalidRequest "`position` uses 1-based line and column numbers"
      pure <| some { line, column }

def parseRequest (payload : Json) : Except JsonSupport.Error Request := do
  let moduleName ← JsonSupport.requiredString payload "module"
  let sourceText? ← JsonSupport.optionalStringAliases payload #["source_text", "source"]
  let sourceText ←
    match sourceText? with
    | some text => pure text
    | none => throw <| JsonSupport.invalidRequest "`source_text` is required for proof-goal features"
  let fileLabel? ← JsonSupport.optionalString payload "file_label"
  let declaration? ← JsonSupport.optionalString payload "declaration"
  let namespace? ← JsonSupport.optionalString payload "namespace"
  let position? ← parsePosition payload
  pure
    { module := moduleName
      sourceText
      fileLabel := fileLabel?.getD s!"{moduleName}.lean"
      declaration?
      position?
      namespace? }

unsafe def run (payload : Json) : IO (Except JsonSupport.Error (Array Json)) := do
  match parseRequest payload with
  | .error err => pure <| .error err
  | .ok request =>
      let opts := Options.empty
      let inputCtx := Parser.mkInputContext request.sourceText request.fileLabel
      let (header, parserState, headerMessages) ← Parser.parseHeader inputCtx
      if headerMessages.hasErrors then
        return .error <| JsonSupport.invalidRequest "could not parse source header"
      try
        Lean.enableInitializersExecution
        initSearchPath (← getBuildDir)
        let headerImports := Elab.headerToImports header (includeInit := true)
        let commandEnv ← importModules headerImports opts (loadExts := true)
        let initialMessages := headerMessages
        let mut commandState := Command.mkState commandEnv initialMessages opts
        commandState := { commandState with infoState.enabled := true }
        if let some namespaceText := request.namespace? then
          let head := commandState.scopes.headD { header := "", opts }
          commandState := { commandState with scopes := [{ head with currNamespace := namespaceText.toName }] }
        let headerSource := String.Pos.Raw.extract request.sourceText 0 parserState.pos
        let bodySource := String.Pos.Raw.extract request.sourceText parserState.pos request.sourceText.rawEndPos
        let bodyInputCtx := Parser.mkInputContext bodySource request.fileLabel
        let finalState ← Elab.IO.processCommands bodyInputCtx { : Parser.ModuleParserState } commandState
        if finalState.commandState.messages.hasErrors then
          return .error <|
            JsonSupport.invalidRequest
              "source elaboration produced errors"
              (some <| Json.mkObj [("messages", (← messageDiagnosticsJson finalState.commandState.messages))])
        let doc := SourceDocument.fromSources request.sourceText bodySource (countNewlines headerSource)
        let candidates ← collectTacticCandidates doc finalState.commandState.infoState.trees
        match selectCandidate doc request candidates with
        | .error err => pure <| .error err
        | .ok (goalId, candidate) =>
            match ← rowFromCandidate goalId candidate with
            | .ok row => pure <| .ok #[row]
            | .error err => pure <| .error err
      catch error =>
        pure <|
          .error
            (JsonSupport.internalError s!"proof-goal source elaboration failed: {error}")

end LeanSemanticSearch.GoalElaboration
