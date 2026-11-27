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

  -- Omission discovers declarations; an explicit empty selector is intentional
  -- selection of none (used by downstream callers with a computed empty scope).
  let emptyRequest := Json.mkObj
    [ ("modules", ← objField request "modules")
    , ("include_generated", Json.bool true)
    , ("declaration_ids", Json.arr #[])
    ]
  let emptyResponse ← parseJson (← LeanSemanticSearch.Capability.declarationFeatures emptyRequest.compress)
  require (← arrField emptyResponse "rows").isEmpty "explicit empty declaration selection must return no rows"
  require (← arrField emptyResponse "diagnostics").isEmpty "empty selection is valid, not an error"

private unsafe def compileSentinelModule (dir : System.FilePath) (moduleName : Name) : IO Unit := do
  let stem := moduleName.toString
  let sourcePath := (dir / stem).withExtension "lean"
  let oleanPath := (dir / stem).withExtension "olean"
  IO.FS.writeFile sourcePath "def sentinel : Nat := 1\n"
  let output ←
    IO.Process.output
      { cmd := "lean"
        args := #["-R", dir.toString, "-o", oleanPath.toString, sourcePath.toString] }
  require
    (output.exitCode == 0)
    s!"failed to compile ambient sentinel module:\nstdout:\n{output.stdout}\nstderr:\n{output.stderr}"

private unsafe def requireSentinelImport (moduleName : Name) : IO Unit := do
  try
    Lean.enableInitializersExecution
    discard <| importModules #[({ module := moduleName } : Import)] Options.empty (loadExts := true)
  catch error =>
    fail s!"ambient sentinel module was not importable after the command ran: {error}"

private unsafe def withAmbientSentinel (label : String) (action : IO Unit) : IO Unit :=
  IO.FS.withTempDir fun dir => do
    let pid ← IO.Process.getPID
    let moduleName := Name.mkSimple s!"LeanSemanticSearchSentinel{label}{pid}"
    compileSentinelModule dir moduleName
    let originalSearchPath ← searchPathRef.get
    searchPathRef.set (dir :: originalSearchPath)
    try
      action
      requireSentinelImport moduleName
    finally
      searchPathRef.set originalSearchPath

private unsafe def testDeclarationFeaturesPreservesAmbientSearchPath : IO Unit :=
  withAmbientSentinel "Declaration" do
    let request :=
      Json.mkObj
        [ ("modules", Json.arr #[Json.mkObj [("module", Json.str "LeanSemanticSearchTest.Fixtures")]])
        , ("include_generated", Json.bool true)
        ]
    let response ← parseJson (← LeanSemanticSearch.Capability.declarationFeatures request.compress)
    let rows ← arrField response "rows"
    require (!rows.isEmpty) "declaration feature response should contain fixture rows"

private def proofGoalSource : String :=
  String.intercalate "\n"
    [ "import Init"
    , ""
    , "theorem goalFeatureSource (p q : Prop) (hp : p) : And p q → And q p := by"
    , "  intro h"
    , "  exact And.intro h.right hp"
    ]

-- A Lake setup with no import override, options, plugins, or artifacts, so
-- elaboration follows the source header against the test search path.
private def goalSetup : ModuleSetup := { name := `LeanSemanticSearchTest.GoalSource }

private def goalRequest (source declaration : String) : Json :=
  Json.mkObj
    [ ("module", Json.str "LeanSemanticSearchTest.GoalSource")
    , ("source_text", Json.str source)
    , ("declaration", Json.str declaration)
    ]

private unsafe def testProofGoalFeatures : IO Unit := do
  let .ok rows ←
      LeanSemanticSearch.GoalElaboration.runWithSetup goalSetup
        (goalRequest proofGoalSource "goalFeatureSource")
    | fail "the default setup should yield proof-goal features"
  let some row := rows[0]? | fail "expected one proof-goal row"
  require
    ((← strField row "feature_version") == LeanSemanticSearch.JsonSupport.semanticFeatureVersion)
    "proof-goal row should carry the semantic feature version"
  let roleFeatures ← arrField row "role_features"
  require (roleFeatures.size > 0) "proof-goal row should contain role features"
  let text := (Json.arr rows).compress
  require
    (!text.contains "goalsBefore" && !text.contains "raw_expr")
    "proof-goal rows must not expose rendered goals or raw expressions"

private unsafe def testProofGoalFeaturesPreservesAmbientSearchPath : IO Unit :=
  withAmbientSentinel "ProofGoal" do
    let .ok rows ←
        LeanSemanticSearch.GoalElaboration.runWithSetup goalSetup
          (goalRequest proofGoalSource "goalFeatureSource")
      | fail "the default setup should yield proof-goal features"
    require (!rows.isEmpty) "proof-goal rows should not be empty"

-- Each setup field the host passes must take effect, and each failure must stay
-- a structured error rather than fall back to the source header or search path.
private unsafe def testProofGoalSetupControls : IO Unit := do
  let request := goalRequest proofGoalSource "goalFeatureSource"
  let mismatch := { goalSetup with name := `Wrong.Module }
  let .error mismatchError ← LeanSemanticSearch.GoalElaboration.runWithSetup mismatch request
    | fail "a setup for another module must refuse"
  require (mismatchError.kind == .invalidRequest) "module mismatch must remain a request error"

  let implicitSource := "import Init\n\ntheorem implicitGoal (x : α) : x = x := by\n  skip\n  rfl\n"
  let implicitRequest := goalRequest implicitSource "implicitGoal"
  let .ok implicitRows ← LeanSemanticSearch.GoalElaboration.runWithSetup goalSetup implicitRequest
    | fail "Lean's default options should permit auto-bound implicits"
  require (!implicitRows.isEmpty) "default options should yield the implicit proof state"
  let some strictOptions := LeanOptions.fromOptions? (Options.empty.setBool `autoImplicit false)
    | fail "serializable setup options expected"
  let .error strictError ← LeanSemanticSearch.GoalElaboration.runWithSetup
      { goalSetup with options := strictOptions } implicitRequest
    | fail "Lake autoImplicit=false must refuse the different implicit proof state"
  require (strictError.kind == .invalidRequest) "source option failure must remain a structured request error"

  let overriddenSource := proofGoalSource.replace "import Init" "import LeanSemanticSearchTest.MissingForSetup"
  let .ok overriddenRows ← LeanSemanticSearch.GoalElaboration.runWithSetup
      { goalSetup with imports? := some #[{ module := `Init }] }
      (goalRequest overriddenSource "goalFeatureSource")
    | fail "Lake imports must supersede the source header"
  require (!overriddenRows.isEmpty) "Lake-selected imports should produce goal features"

  let absentPlugin := (← IO.currentDir) / ".lake" / "LeanSemanticSearchTest.missing-plugin.dylib"
  require (!(← absentPlugin.pathExists)) "missing-plugin control path must not exist"
  let .error pluginError ← LeanSemanticSearch.GoalElaboration.runWithSetup
      { goalSetup with plugins := #[Plugin.ofFilePath absentPlugin] } request
    | fail "a missing setup plugin must refuse instead of using default imports"
  require (pluginError.kind == .importFailed) "missing plugin must remain an import error"

  let absentArtifact := (← IO.currentDir) / ".lake" / "LeanSemanticSearchTest.missing-artifact.olean"
  require (!(← absentArtifact.pathExists)) "missing-artifact control path must not exist"
  let artifacts : NameMap ImportArtifacts := ({} : NameMap ImportArtifacts).insert `Init
    (.ofArrays #[#[absentArtifact], #[]])
  let .error artifactError ← LeanSemanticSearch.GoalElaboration.runWithSetup
      { goalSetup with importArts := artifacts } request
    | fail "a missing Lake-selected artifact must not fall back to the search path"
  require (artifactError.kind == .importFailed) "missing artifact must remain an import error"

-- Goal elaboration reads the whole file, as `lean` does: a command sees the
-- header, and diagnostics carry file lines.
private unsafe def testProofGoalSeesTheHeader : IO Unit := do
  let headerSource := String.intercalate "\n"
    [ "import Lean"
    , ""
    , "open Lean Elab Command in"
    , "elab \"#require_header\" : command => do"
    , "  unless (← getFileMap).source.startsWith \"import Lean\" do"
    , "    throwError \"the file was elaborated without its header\""
    , ""
    , "#require_header"
    , ""
    , "theorem headerGoal (p : Prop) (hp : p) : p := by"
    , "  exact hp"
    ]
  let .ok rows ← LeanSemanticSearch.GoalElaboration.runWithSetup goalSetup
      (goalRequest headerSource "headerGoal")
    | fail "a command that reads the file must see its header"
  require (!rows.isEmpty) "the header-reading source should yield goal features"

  let broken := proofGoalSource ++ "\n\ntheorem brokenGoal : False := by\n  exact True.intro\n"
  let .error brokenError ← LeanSemanticSearch.GoalElaboration.runWithSetup goalSetup
      (goalRequest broken "brokenGoal")
    | fail "a source with an elaboration error must refuse"
  let some details := brokenError.details | fail "an elaboration error must carry its messages"
  let some message := (← arrField details "messages")[0]? | fail "expected an elaboration message"
  let .ok line := message.getObjValAs? Nat "line" | fail "an elaboration message must carry a line"
  require (line == 8) s!"elaboration messages must carry file lines, not body lines; got line {line}"

private unsafe def testDiagnostics : IO Unit := do
  let malformed ← parseJson (← LeanSemanticSearch.Capability.declarationFeatures "{")
  let malformedDiagnostics ← arrField malformed "diagnostics"
  require (!malformedDiagnostics.isEmpty) "malformed JSON should return diagnostics"
  let missingSource := Json.mkObj [("module", Json.str "LeanSemanticSearchTest.GoalSource")]
  let .error missingError ← LeanSemanticSearch.GoalElaboration.runWithSetup goalSetup missingSource
    | fail "a proof-goal request without source must refuse"
  require (missingError.kind == .invalidRequest) "missing proof-goal source must be a request error"

private def requireInvalidJsonResponse
    (text command commandVersion : String) : IO Unit := do
  let response ← parseJson text
  require
    ((← strField response "schema_version") == LeanSemanticSearch.JsonSupport.schemaVersion)
    "invalid requests must retain the capability schema version"
  require ((← strField response "command") == command) "invalid requests must retain the command"
  require
    ((← strField response "command_version") == commandVersion)
    "invalid requests must retain the command version"
  require
    ((← strField response "feature_version") == LeanSemanticSearch.JsonSupport.semanticFeatureVersion)
    "invalid requests must retain the feature version"
  require (← arrField response "rows").isEmpty "invalid requests must return no feature rows"
  let diagnostics ← arrField response "diagnostics"
  require (diagnostics.size == 1) "invalid requests must return one diagnostic"
  let some diagnostic := diagnostics[0]? | fail "missing invalid-request diagnostic"
  require ((← strField diagnostic "severity") == "error") "invalid requests must report an error"
  require
    ((← strField diagnostic "code") == "lean_semantic_search.request.invalid")
    "invalid requests must report the request diagnostic code"
  require
    ((← strField diagnostic "message").startsWith "could not parse request JSON:")
    "duplicate keys must be rejected by the JSON parser before request validation"

-- Lean 4.36 rejects duplicate JSON object keys, including nested and escaped
-- keys. The declaration command must keep these parser failures in the
-- envelope. Proof-goal requests arrive as parsed `Json`; their host parses them.
private unsafe def testDuplicateJsonKeys : IO Unit := do
  let declarationRequests :=
    #[ r#"{"modules":[{"module":"Init"}],"modules":[{"module":"Init"}]}"#
     , r#"{"modules":[{"module":"Init","module":"Init"}]}"#
     , r#"{"modules":[{"module":"Init"}],"\u006dodules":[{"module":"Init"}]}"#
     ]
  for request in declarationRequests do
    requireInvalidJsonResponse
      (← LeanSemanticSearch.Capability.declarationFeatures request)
      LeanSemanticSearch.JsonSupport.declarationCommand
      LeanSemanticSearch.JsonSupport.declarationFeatureCommandVersion

unsafe def main (_args : List String) : IO UInt32 := do
  Lean.enableInitializersExecution
  initSearchPath (← getBuildDir)
  testCanonicalAlphaStability
  testGoldenFingerprints
  testGeneratedFiltering
  testDeclarationFeatures
  testDeclarationFeaturesPreservesAmbientSearchPath
  testProofGoalFeatures
  testProofGoalFeaturesPreservesAmbientSearchPath
  testProofGoalSetupControls
  testProofGoalSeesTheHeader
  testDiagnostics
  testDuplicateJsonKeys
  pure 0

end LeanSemanticSearchTest
