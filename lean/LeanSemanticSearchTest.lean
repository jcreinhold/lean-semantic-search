import LeanSemanticSearch
import LeanSemanticSearchTest.Fixtures

open Lean
open Lean.Meta

namespace LeanSemanticSearchTest

private def fail (message : String) : IO α :=
  throw <| IO.userError message

private def require (condition : Bool) (message : String) : IO Unit :=
  if condition then pure () else fail message

private def parseJson (text : String) : IO Json :=
  match Json.parse text with
  | .ok json => pure json
  | .error error => fail s!"invalid JSON response: {error}\n{text}"

private def objField (json : Json) (key : String) : IO Json :=
  match json.getObjVal? key with
  | .ok value => pure value
  | .error _ => fail s!"missing JSON field `{key}`"

private def arrField (json : Json) (key : String) : IO (Array Json) := do
  match (← objField json key).getArr? with
  | .ok values => pure values
  | .error _ => fail s!"JSON field `{key}` is not an array"

private def strField (json : Json) (key : String) : IO String := do
  match (← objField json key).getStr? with
  | .ok value => pure value
  | .error _ => fail s!"JSON field `{key}` is not a string"

private def firstRow (json : Json) : IO Json := do
  match (← arrField json "rows")[0]? with
  | some row => pure row
  | none => fail s!"expected at least one feature row in response: {json.compress}"

private unsafe def withFixtureMeta (action : MetaM α) : IO α := do
  Lean.enableInitializersExecution
  initSearchPath (← getBuildDir)
  let env ← importModules #[({ module := `LeanSemanticSearchTest.Fixtures } : Import)] Options.empty (loadExts := true)
  let coreContext : Core.Context :=
    { fileName := "<lean-semantic-search-test>"
      fileMap := default
      options := Options.empty }
  let (result, _, _) ← MetaM.toIO action coreContext { env := env } {} {}
  pure result

private unsafe def testCanonicalAlphaStability : IO Unit := do
  let (left, right) ←
    withFixtureMeta do
      let env ← getEnv
      let some leftInfo := env.find? ``LeanSemanticSearchTest.Fixtures.alphaLeft
        | throwError "missing alphaLeft"
      let some rightInfo := env.find? ``LeanSemanticSearchTest.Fixtures.alphaRight
        | throwError "missing alphaRight"
      let left ← LeanSemanticSearch.Canonical.compute leftInfo
      let right ← LeanSemanticSearch.Canonical.compute rightInfo
      pure (left, right)
  require (left.statement == right.statement) "alpha-equivalent statements should share a canonical fingerprint"
  require
    (left.safeBinderPermutation == right.safeBinderPermutation)
    "alpha-equivalent statements should share the binder-permutation-safe fingerprint"

private unsafe def testDeclarationFeatures : IO Unit := do
  let request :=
    Json.mkObj
      [ ("modules", Json.arr #[Json.mkObj [("module", Json.str "LeanSemanticSearchTest.Fixtures")]])
      , ("include_generated", Json.bool true)
      ]
  let response ← parseJson (← LeanSemanticSearch.Capability.declarationFeatures request.compress)
  require
    ((← strField response "feature_version") == LeanSemanticSearch.JsonSupport.semanticFeatureVersion)
    "declaration response should carry the semantic feature version"
  let row ← firstRow response
  let fingerprints ← objField row "fingerprints"
  discard <| strField fingerprints "statement"
  discard <| strField fingerprints "safe_binder_permutation"
  discard <| arrField row "role_features"
  discard <| arrField row "low_signal_markers"
  discard <| objField row "source"
  require
    (!response.compress.contains "raw_expr")
    "declaration feature response must not expose raw expressions"

private def proofGoalSource : String :=
  String.intercalate "\n"
    [ "import Init"
    , ""
    , "theorem goalFeatureSource (p q : Prop) (hp : p) : And p q → And q p := by"
    , "  intro h"
    , "  exact And.intro h.right hp"
    ]

private unsafe def testProofGoalFeatures : IO Unit := do
  let request :=
    Json.mkObj
      [ ("module", Json.str "LeanSemanticSearchTest.GoalSource")
      , ("source_text", Json.str proofGoalSource)
      , ("declaration", Json.str "goalFeatureSource")
      ]
  let response ← parseJson (← LeanSemanticSearch.Capability.proofGoalFeatures request.compress)
  require
    ((← strField response "feature_version") == LeanSemanticSearch.JsonSupport.semanticFeatureVersion)
    "proof-goal response should carry the semantic feature version"
  let row ← firstRow response
  let roleFeatures ← arrField row "role_features"
  require (roleFeatures.size > 0) "proof-goal row should contain role features"
  require
    (!response.compress.contains "goalsBefore" && !response.compress.contains "raw_expr")
    "proof-goal response must not expose rendered goals or raw expressions"

private unsafe def testDiagnostics : IO Unit := do
  let malformed ← parseJson (← LeanSemanticSearch.Capability.proofGoalFeatures "{")
  let malformedDiagnostics ← arrField malformed "diagnostics"
  require (!malformedDiagnostics.isEmpty) "malformed JSON should return diagnostics"
  let missingSource :=
    Json.mkObj [("module", Json.str "LeanSemanticSearchTest.GoalSource")]
  let missing ← parseJson (← LeanSemanticSearch.Capability.proofGoalFeatures missingSource.compress)
  let missingDiagnostics ← arrField missing "diagnostics"
  require (!missingDiagnostics.isEmpty) "missing proof-goal source should return diagnostics"

unsafe def main (_args : List String) : IO UInt32 := do
  testCanonicalAlphaStability
  testDeclarationFeatures
  testProofGoalFeatures
  testDiagnostics
  pure 0

end LeanSemanticSearchTest
