namespace LeanSemanticSearchTest.Fixtures

theorem alphaLeft (p : Prop) : p → p := fun hp => hp

theorem alphaRight (q : Prop) : q → q := fun hq => hq

theorem featureFixture (p q : Prop) (hp : p) (hq : q) : p ∧ q :=
  And.intro hp hq

-- A definition whose body elaborates a `match`, which generates an auxiliary
-- `usesMatch.match_1` declaration. The extraction classifier must filter that
-- aux declaration out unless generated declarations are explicitly requested.
def usesMatch (n : Nat) : Nat :=
  match n with
  | 0 => 0
  | k + 1 => k

end LeanSemanticSearchTest.Fixtures
