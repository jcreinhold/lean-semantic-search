import Lake
open Lake DSL

package «lean-semantic-search» where
  version := v!"0.1.0"

@[default_target]
lean_lib LeanSemanticSearch where
  roots := #[`LeanSemanticSearch]
  globs := #[.andSubmodules `LeanSemanticSearch]

lean_lib LeanSemanticSearchTest where
  roots := #[`LeanSemanticSearchTest]
  globs := #[.andSubmodules `LeanSemanticSearchTest]

@[test_driver]
lean_exe tests where
  root := `LeanSemanticSearchTests
