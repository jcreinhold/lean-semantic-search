import Lean
import LeanSemanticSearch.LeanCompat.Shape

/-!
JSON support for the standalone semantic-search commands.

This module owns command envelopes, diagnostics, request parsing helpers, and
stable version strings. It intentionally does not know how semantic features are
computed.
-/

namespace LeanSemanticSearch.JsonSupport

open Lean (Json)

def schemaVersion : String := "lean-semantic-search.capability.v1"

def semanticFeatureVersion : String := "features.roles.v3"

def declarationFeatureCommandVersion : String := "declaration_features.v1"

def proofGoalFeatureCommandVersion : String := "proof_goal_features.v1"

def roleKeyVersion : String := "features.role_key.v1"

def declarationCommand : String := "declaration_features"

def proofGoalCommand : String := "proof_goal_features"

inductive ErrorKind where
  | invalidRequest
  | importFailed
  | proofGoalUnavailable
  | internalError
  deriving BEq, Repr

structure Error where
  kind : ErrorKind
  message : String
  details : Option Json := none

def invalidRequest (message : String) (details : Option Json := none) : Error :=
  { kind := .invalidRequest, message, details }

def importFailed (message : String) (details : Option Json := none) : Error :=
  { kind := .importFailed, message, details }

def proofGoalUnavailable (message : String) (details : Option Json := none) : Error :=
  { kind := .proofGoalUnavailable, message, details }

def internalError (message : String) (details : Option Json := none) : Error :=
  { kind := .internalError, message, details }

/-- Map a boundary error reported by `LeanCompat` onto the envelope error type.
    The categories correspond one-to-one. -/
def Error.ofCompat : LeanSemanticSearch.LeanCompat.CompatError → Error
  | .invalidRequest message details? => { kind := .invalidRequest, message, details := details? }
  | .importFailed message details? => { kind := .importFailed, message, details := details? }
  | .proofGoalUnavailable message details? => { kind := .proofGoalUnavailable, message, details := details? }
  | .internalError message details? => { kind := .internalError, message, details := details? }

def optionalField (json : Json) (key : String) : Option Json :=
  match json.getObjVal? key with
  | .ok value => some value
  | .error _ => none

def requiredString (json : Json) (key : String) : Except Error String := do
  match optionalField json key with
  | some value =>
      match value.getStr? with
      | .ok text =>
          if text.isEmpty then
            throw <| invalidRequest s!"`{key}` must not be empty"
          else
            pure text
      | .error _ => throw <| invalidRequest s!"`{key}` must be a string"
  | none => throw <| invalidRequest s!"missing required string field `{key}`"

def optionalString (json : Json) (key : String) : Except Error (Option String) := do
  match optionalField json key with
  | none | some Json.null => pure none
  | some value =>
      match value.getStr? with
      | .ok text => pure <| if text.isEmpty then none else some text
      | .error _ => throw <| invalidRequest s!"`{key}` must be a string or null"

def optionalStringAliases (json : Json) (keys : Array String) : Except Error (Option String) := do
  for key in keys do
    match optionalField json key with
    | some _ => return (← optionalString json key)
    | none => pure ()
  pure none

def optionalNat (json : Json) (key : String) : Except Error (Option Nat) := do
  match optionalField json key with
  | none | some Json.null => pure none
  | some value =>
      match value.getNat? with
      | .ok n => pure (some n)
      | .error _ => throw <| invalidRequest s!"`{key}` must be a natural number or null"

def boolField (json : Json) (key : String) (default : Bool) : Except Error Bool := do
  match optionalField json key with
  | none => pure default
  | some value =>
      match value.getBool? with
      | .ok b => pure b
      | .error _ => throw <| invalidRequest s!"`{key}` must be a boolean"

def stringArrayField? (json : Json) (key : String) : Except Error (Option (Array String)) := do
  match optionalField json key with
  | none | some Json.null => pure none
  | some (Json.arr values) =>
      let mut out := #[]
      for value in values do
        match value with
        | Json.str text =>
            if text.isEmpty then
              throw <| invalidRequest s!"`{key}` must not contain empty strings"
            out := out.push text
        | _ => throw <| invalidRequest s!"`{key}` must contain only strings"
      pure (some out)
  | some _ => throw <| invalidRequest s!"`{key}` must be an array"

def pointJson (line column : Nat) : Json :=
  Json.mkObj [("line", Json.num line), ("column", Json.num column)]

def sourceSpanJson (startLine startColumn endLine endColumn : Nat) : Json :=
  Json.mkObj
    [ ("start", pointJson startLine startColumn)
    , ("end", pointJson endLine endColumn)
    ]

def stringArrayJson (values : Array String) : Json :=
  Json.arr (values.map Json.str)

def diagnosticJson
    (severity code message : String)
    (details : Json := Json.null) : Json :=
  Json.mkObj
    [ ("severity", Json.str severity)
    , ("code", Json.str code)
    , ("message", Json.str message)
    , ("details", details)
    ]

private def errorCode : ErrorKind → String
  | .invalidRequest => "lean_semantic_search.request.invalid"
  | .importFailed => "lean_semantic_search.import.failed"
  | .proofGoalUnavailable => "lean_semantic_search.proof_goal.unavailable"
  | .internalError => "lean_semantic_search.internal"

def errorDiagnostic (error : Error) : Json :=
  diagnosticJson
    "error"
    (errorCode error.kind)
    error.message
    (error.details.getD Json.null)

def passDiagnostic (code message : String) (details : Json := Json.null) : Json :=
  diagnosticJson "pass" code message details

def responseJson
    (command commandVersion featureVersion : String)
    (rows diagnostics : Array Json) : Json :=
  Json.mkObj
    [ ("schema_version", Json.str schemaVersion)
    , ("command", Json.str command)
    , ("command_version", Json.str commandVersion)
    , ("feature_version", Json.str featureVersion)
    , ("rows", Json.arr rows)
    , ("diagnostics", Json.arr diagnostics)
    ]

def errorResponseJson (command commandVersion featureVersion : String) (error : Error) : Json :=
  responseJson command commandVersion featureVersion #[] #[errorDiagnostic error]

def parsePayload (input : String) : Except Error Json :=
  match Json.parse input with
  | .ok json => .ok json
  | .error message => .error <| invalidRequest s!"could not parse request JSON: {message}"

end LeanSemanticSearch.JsonSupport
