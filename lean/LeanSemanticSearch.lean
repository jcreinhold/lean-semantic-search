import LeanSemanticSearch.Capability
import LeanSemanticSearch.Canonical
import LeanSemanticSearch.DeclarationFeatures
import LeanSemanticSearch.GoalElaboration
import LeanSemanticSearch.GoalFeatures
import LeanSemanticSearch.Hashing
import LeanSemanticSearch.Json
import LeanSemanticSearch.LeanCompat
import LeanSemanticSearch.ModuleExtraction
import LeanSemanticSearch.RoleFeatures

/-!
Root module for the Lean semantic search package.

Hosts call `Capability.declarationFeatures` for declaration features and
`GoalElaboration.runWithSetup` for proof-goal features.
-/
