import LeanSemanticSearch
import LeanSemanticSearchTest.Fixtures

open Lean
open Lean.Meta

namespace LeanSemanticSearchTest

private def fail {α : Type} (message : String) : IO α :=
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

private unsafe def withFixtureMeta {α : Type} (action : MetaM α) : IO α := do
  Lean.enableInitializersExecution
  initSearchPath (← getBuildDir)
  let env ← importModules #[({ module := `LeanSemanticSearchTest.Fixtures } : Import)] Options.empty (loadExts := true)
  let coreContext : Core.Context :=
    { fileName := "<lean-semantic-search-test>"
      fileMap := default
      options := Options.empty }
  let (result, _, _) ← MetaM.toIO action coreContext { env := env } {} {}
  pure result

private unsafe def fingerprintsOf (declName : Lean.Name) :
    MetaM LeanSemanticSearch.Canonical.Fingerprints := do
  let some info := (← getEnv).find? declName | throwError s!"missing {declName}"
  let statement ← LeanSemanticSearch.LeanCompat.statementOfConstant info
  pure (LeanSemanticSearch.Canonical.computeFromStatement statement)

private unsafe def testCanonicalAlphaStability : IO Unit := do
  let (left, right) ←
    withFixtureMeta do
      let left ← fingerprintsOf ``LeanSemanticSearchTest.Fixtures.alphaLeft
      let right ← fingerprintsOf ``LeanSemanticSearchTest.Fixtures.alphaRight
      pure (left, right)
  require (left.statement == right.statement) "alpha-equivalent statements should share a canonical fingerprint"
  require
    (left.safeBinderPermutation == right.safeBinderPermutation)
    "alpha-equivalent statements should share the binder-permutation-safe fingerprint"

-- Golden fingerprints for `featureFixture`, captured on v4.30.0 before the
-- owned-IR refactor. Any drift here means the canonical encoding changed: either
-- a real regression in the translator/serializer, or a deliberate version bump
-- (which must move `canonical.expr.v3` and the contract constants together).
private unsafe def testGoldenFingerprints : IO Unit := do
  let fp ← withFixtureMeta (fingerprintsOf ``LeanSemanticSearchTest.Fixtures.featureFixture)
  require
    (fp.statement == "canonical.expr.v3:statement:5331087004229474883")
    s!"featureFixture statement fingerprint drifted: {fp.statement}"
  require
    (fp.safeBinderPermutation == "canonical.expr.v3:safe_binder_permutation:5331087004229474883")
    s!"featureFixture safe-binder-permutation fingerprint drifted: {fp.safeBinderPermutation}"
  require
    (fp.connectiveShape == "canonical.expr.v3:connective_shape:15625476516034344464")
    s!"featureFixture connective-shape fingerprint drifted: {fp.connectiveShape}"
  require
    (fp.conclusionShape == "canonical.expr.v3:conclusion_shape:6267851093983210902")
    s!"featureFixture conclusion-shape fingerprint drifted: {fp.conclusionShape}"
  require (fp.binderCount == 4) s!"featureFixture binder count drifted: {fp.binderCount}"

private unsafe def testMetadataAndDoctor : IO Unit := do
  let metadata ← parseJson (← LeanSemanticSearch.Capability.metadata "")
  let extra ← objField metadata "extra"
  require
    ((← strField extra "schema_version") == LeanSemanticSearch.JsonSupport.schemaVersion)
    "metadata must carry the schema version"
  require (!(← arrField metadata "commands").isEmpty) "metadata must advertise commands"
  require (!(← arrField metadata "capabilities").isEmpty) "metadata must advertise capabilities"
  let doctor ← parseJson (← LeanSemanticSearch.Capability.doctor "")
  require (!(← arrField doctor "diagnostics").isEmpty) "doctor must report diagnostics"

-- The classifier must filter compiler-generated declarations (here the
-- `usesMatch.match_1` matcher) unless generated declarations are requested.
private unsafe def testGeneratedFiltering : IO Unit := do
  let request :=
    Json.mkObj
      [ ("modules", Json.arr #[Json.mkObj [("module", Json.str "LeanSemanticSearchTest.Fixtures")]]) ]
  let response ← parseJson (← LeanSemanticSearch.Capability.declarationFeatures request.compress)
  let rows ← arrField response "rows"
  let mut ids := #[]
  for row in rows do
    ids := ids.push (← strField row "declaration_id")
  require
    (ids.any fun id => id.endsWith "usesMatch")
    "usesMatch should be extracted when generated declarations are excluded"
  require
    (!ids.any fun id => (id.splitOn ".match_").length > 1)
    "generated matcher declarations must be filtered out by default"

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
  testGoldenFingerprints
  testMetadataAndDoctor
  testGeneratedFiltering
  testDeclarationFeatures
  testProofGoalFeatures
  testDiagnostics
  pure 0

end LeanSemanticSearchTest
