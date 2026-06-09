import Lake
open Lake DSL

/-- Lean elaboration options shared across the library, the test support
    library, and the test driver. Mirrors the strictness used elsewhere in the
    Lean tooling: no auto-bound implicits, a bounded synthesis-pending depth,
    and unicode function arrows when pretty-printing. -/
abbrev leanSemanticSearchLeanOptions : Array LeanOption := #[
  ⟨`autoImplicit, false⟩,
  ⟨`maxSynthPendingDepth, .ofNat 3⟩,
  ⟨`pp.unicode.fun, true⟩
]

package «lean-semantic-search» where
  version := v!"0.3.0"

@[default_target]
lean_lib LeanSemanticSearch where
  leanOptions := leanSemanticSearchLeanOptions
  roots := #[`LeanSemanticSearch]
  globs := #[.andSubmodules `LeanSemanticSearch]

-- Test support: helpers and importable fixtures. Kept outside the
-- `LeanSemanticSearch` glob so test modules never ship in the library; the
-- fixtures need their own `.olean` because the tests import them at runtime.
lean_lib LeanSemanticSearchTest where
  leanOptions := leanSemanticSearchLeanOptions
  roots := #[`LeanSemanticSearchTest]
  globs := #[.andSubmodules `LeanSemanticSearchTest]

@[test_driver]
lean_exe tests where
  leanOptions := leanSemanticSearchLeanOptions
  root := `Main
  -- The tests import modules and elaborate Lean source at runtime, which runs
  -- through the interpreter; link the shared runtime so native externals
  -- (e.g. `IO.getRandomBytes`) resolve instead of failing at startup.
  supportInterpreter := true
