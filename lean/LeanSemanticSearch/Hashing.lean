/-!
The stable string hash behind every opaque key this package emits.

Both the canonical fingerprint keys and the role-feature keys hash a private
encoded string into their opaque token. That hash is one decision — the digest
must stay byte-identical across releases or every emitted key drifts — so it
lives in exactly one place. Callers treat the result as opaque; only the two
key encoders consume it.

This is FNV-1a (64-bit) rendered as decimal. The constants and fold must not
change without a deliberate version bump of every key that depends on them; the
golden-fingerprint test guards against accidental drift.
-/

namespace LeanSemanticSearch.Hashing

private def seed : UInt64 := 14695981039346656037

private def prime : UInt64 := 1099511628211

/-- Hash an encoded key body into its stable opaque digest. -/
def stableHash (text : String) : String :=
  toString <|
    text.foldl (fun acc char => (acc ^^^ char.toNat.toUInt64) * prime) seed

end LeanSemanticSearch.Hashing
