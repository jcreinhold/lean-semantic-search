namespace LeanSemanticSearchTest.Fixtures

theorem alphaLeft (p : Prop) : p → p := fun hp => hp

theorem alphaRight (q : Prop) : q → q := fun hq => hq

theorem featureFixture (p q : Prop) (hp : p) (hq : q) : p ∧ q :=
  And.intro hp hq

end LeanSemanticSearchTest.Fixtures
