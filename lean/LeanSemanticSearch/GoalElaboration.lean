import LeanSemanticSearch.GoalFeatures
import LeanSemanticSearch.LeanCompat

/-!
Source-backed proof-goal extraction.

The exported command passes source text and a selector. This module parses that
request and maps the boundary's result onto the command envelope; the actual
elaboration, tactic selection, and goal translation live behind
`LeanCompat.selectGoalSnapshot`, which exposes only semantic feature facts.
-/

namespace LeanSemanticSearch.GoalElaboration

open Lean (Json)
open LeanSemanticSearch.LeanCompat (GoalRequest GoalPosition)

private def parsePosition (payload : Json) : Except JsonSupport.Error (Option GoalPosition) := do
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

def parseRequest (payload : Json) : Except JsonSupport.Error GoalRequest := do
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
      namespaceName? := namespace? }

unsafe def run (payload : Json) : IO (Except JsonSupport.Error (Array Json)) := do
  match parseRequest payload with
  | .error err => pure <| .error err
  | .ok request =>
      match ← LeanCompat.selectGoalSnapshot request with
      | .error compatErr => pure <| .error (JsonSupport.Error.ofCompat compatErr)
      | .ok snapshot => pure <| .ok #[GoalFeatures.rowFromSnapshot snapshot]

end LeanSemanticSearch.GoalElaboration
