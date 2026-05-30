#!/usr/bin/env bash
# .claude/hooks/contract-guard.sh — PostToolUse advisory (Edit|Write|MultiEdit).
#
# Surfaces this repo's defining invariant — the Lean and Rust sides must
# stay in lockstep — as a *non-blocking* reminder. The edit has already
# happened; we only print guidance to stderr and exit 2 so the text is
# returned to Claude to act on. We never undo anything. Blocking (a
# PreToolUse veto) would be wrong here: lockstep spans two files in two
# languages, so the first of two necessary edits must be allowed through.
#
# The two contracts (CLAUDE.md, docs/architecture/01-capability-contract.md):
#
#   1. VERSION LOCKSTEP. The contract version *values* (canonical.expr.vN,
#      features.roles.vN, features.role_key.vN, declaration_features.vN,
#      proof_goal_features.vN, lean-semantic-search.capability.vN) are defined
#      as constants in `crates/contract` and mirrored as defs in the Lean
#      `Json` module. Changing one of those value literals means the mirror
#      side AND docs/architecture/01-capability-contract.md must move in the
#      same change. There is no cross-language test that catches drift.
#
#      The retrieval policy version (lean-semantic-search.retrieval.vN /
#      RETRIEVAL_POLICY_VERSION) is deliberately NOT part of this lockstep:
#      it versions a ranking decision, not a Lean fact, so it lives only in
#      `crates/retrieval` and is mirrored in neither Json.lean nor the
#      capability contract doc (see docs/architecture/03-retrieval.md and
#      04-persistence.md). It gets its own, separate reminder.
#
#   2. EXPORT/COMMAND LOCKSTEP. The five `@[export lean_semantic_search_*]`
#      functions in Capability.lean correspond one-to-one to the
#      *_EXPORT / *_COMMAND constants in crates/capability/src/lib.rs and
#      the advertised commands in the contract doc. Rename/add/remove one
#      and all three must move together.
#
# To avoid false positives, the version check matches the version *value*
# literals (e.g. "canonical.expr.v3"), not bare *_VERSION identifier
# references. Importing or using a version constant by name therefore does
# not trip the hook; only writing a version value does. It stays a plain
# grep (advisory only) — an AST-aware check is the wrong altitude for a hook.
set -euo pipefail

command -v jq >/dev/null 2>&1 || exit 0
input="$(cat)"
file="$(printf '%s' "$input" | jq -r '.tool_input.file_path // empty')"
[ -n "$file" ] && [ -f "$file" ] || exit 0

# Only Rust sources and the Lean package carry these contracts.
case "$file" in
*.rs | *.lean) : ;;
*) exit 0 ;;
esac

msgs=()

# 1. Version lockstep. Match the version *value* literals, not bare
#    identifier references, so importing/using a constant by name does not
#    fire. The mirrored contract versions and the retrieval-only policy
#    version get separate, accurate reminders.
contract_version_re='canonical\.expr\.v[0-9]|features\.roles\.v[0-9]|features\.role_key\.v[0-9]|lean-semantic-search\.capability\.v[0-9]|(declaration|proof_goal)_features\.v[0-9]'
retrieval_version_re='lean-semantic-search\.retrieval\.v[0-9]'
if grep -Eq "$contract_version_re" "$file"; then
	msgs+=("$(
		cat <<EOF
• You changed a contract version value in $file. These are mirrored across
  the Lean↔Rust boundary and have no cross-language test, so move all three
  together:
    - crates/contract/src/lib.rs constants: CAPABILITY_SCHEMA_VERSION,
      DECLARATION_FEATURE_COMMAND_VERSION, PROOF_GOAL_FEATURE_COMMAND_VERSION,
      CANONICAL_FEATURE_VERSION, SEMANTIC_FEATURE_VERSION
    - lean/LeanSemanticSearch/Json.lean defs: schemaVersion,
      declarationFeatureCommandVersion, proofGoalFeatureCommandVersion,
      semanticFeatureVersion, roleKeyVersion
    - docs/architecture/01-capability-contract.md
EOF
	)")
fi
if grep -Eq "$retrieval_version_re" "$file"; then
	msgs+=("$(
		cat <<EOF
• You changed the retrieval policy version in $file. This one is
  retrieval-only by design — it versions a ranking decision, not a Lean fact
  — so do NOT mirror it in Json.lean or the capability contract doc. Keep
  these in sync instead:
    - crates/retrieval/src/policy.rs (POLICY_VERSION) and the
      RETRIEVAL_POLICY_VERSION re-export
    - CHANGELOG.md and docs/architecture/04-persistence.md
EOF
	)")
fi

# 2. Export/command lockstep. Only the two files that own the worker ABI,
#    and only when the body touches an export/command symbol.
case "$file" in
*/lean/LeanSemanticSearch/Capability.lean | lean/LeanSemanticSearch/Capability.lean | \
	*/crates/capability/src/lib.rs | crates/capability/src/lib.rs)
	if grep -Eq '@\[export[[:space:]]+lean_semantic_search_|_EXPORT\b|_COMMAND\b' "$file"; then
		msgs+=("$(
			cat <<EOF
• You touched a worker export/command name in $file. The five
  @[export lean_semantic_search_*] functions in Capability.lean map
  one-to-one to the *_EXPORT / *_COMMAND constants in
  crates/capability/src/lib.rs and the advertised commands in
  docs/architecture/01-capability-contract.md. Rename, add, or remove an
  entry in all three together.
EOF
		)")
	fi
	;;
esac

if [ "${#msgs[@]}" -gt 0 ]; then
	printf 'contract-guard:\n' >&2
	printf '%s\n' "${msgs[@]}" >&2
	exit 2
fi

exit 0
