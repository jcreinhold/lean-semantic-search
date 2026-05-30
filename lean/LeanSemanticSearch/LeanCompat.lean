import LeanSemanticSearch.LeanCompat.Shape
import LeanSemanticSearch.LeanCompat.Translate
import LeanSemanticSearch.LeanCompat.Frontend

/-!
The compatibility boundary for the Lean compiler API.

`LeanCompat` is the only part of the package allowed to name volatile Lean
internals (`Expr`, `Level`, `Meta`, `Elab`, `Environment` internals, `InfoTree`,
the import/elaboration pipeline). It exposes a small, stable, domain-shaped
interface — owned IR types (`Shape`), the `Expr`/`Level` translator
(`Translate`), and the import/elaboration boundary (`Frontend`) — so that a Lean
toolchain bump can only break this directory, never the feature logic.
-/
