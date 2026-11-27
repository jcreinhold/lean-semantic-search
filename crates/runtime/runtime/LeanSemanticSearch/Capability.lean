import LeanSemanticSearch.DeclarationFeatures
import LeanSemanticSearch.GoalElaboration

/-!
Command entry points for hosts that run this package in their own Lean process.

`declarationFeatures` answers the declaration-feature command with a complete
JSON envelope. Proof-goal features need the Lake module setup of their source,
so a host calls `GoalElaboration.runWithSetup`, which this module imports, and
wraps its rows with `JsonSupport.responseJson`. This package owns the command
names, versions, and payloads; `docs/architecture/01-capability-contract.md`
records them.
-/

namespace LeanSemanticSearch.Capability

open Lean (Json)

private def declarationEnvelope (result : Except JsonSupport.Error (Array Json)) : Json :=
  match result with
  | .ok rows =>
      JsonSupport.responseJson
        JsonSupport.declarationCommand
        JsonSupport.declarationFeatureCommandVersion
        JsonSupport.semanticFeatureVersion
        rows
        #[]
  | .error err =>
      JsonSupport.errorResponseJson
        JsonSupport.declarationCommand
        JsonSupport.declarationFeatureCommandVersion
        JsonSupport.semanticFeatureVersion
        err

/-- Answer one declaration-feature request. Malformed JSON, invalid fields, and
    import failures come back as an error diagnostic inside the envelope. The
    caller installs the module search path first. -/
unsafe def declarationFeatures (requestJson : String) : IO String := do
  let result ←
    match JsonSupport.parsePayload requestJson with
    | .error err => pure (.error err)
    | .ok payload =>
        match ModuleExtraction.parseModules payload with
        | .error err => pure (.error err)
        | .ok modules => DeclarationFeatures.run payload modules
  pure (declarationEnvelope result).compress

end LeanSemanticSearch.Capability
