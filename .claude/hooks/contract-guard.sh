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
# The contract (AGENTS.md, docs/architecture/01-capability-contract.md):
#
#   The contract version *values* (canonical.expr.vN, features.roles.vN,
#   declaration_features.vN, proof_goal_features.vN,
#   lean-semantic-search.capability.vN) and the command names
#   ("declaration_features", "proof_goal_features") are defined as constants
#   in `crates/contract` and mirrored as defs in the Lean `Json` module.
#   features.role_key.vN lives only in `Json.lean`: role keys are opaque to
#   Rust. Changing one of those literals means the mirror side AND
#   docs/architecture/01-capability-contract.md must move in the same change.
#   There is no cross-language test that catches drift.
#
#   The retrieval policy version (lean-semantic-search.retrieval.vN /
#   RETRIEVAL_POLICY_VERSION) is deliberately NOT part of this lockstep:
#   it versions a ranking decision, not a Lean fact, so it lives only in
#   `crates/retrieval` and is mirrored in neither Json.lean nor the
#   capability contract doc (see docs/architecture/03-retrieval.md and
#   04-persistence.md). It gets its own, separate reminder.
#
# To avoid false positives, the hook fires only when an edit actually *changes*
# the set of contract tokens (version or command-name literals),
# not merely because the edited file *contains* one. The signal is a diff of the
# matched tokens between the committed (HEAD) and working-tree copies of the
# file: adding an import next to `def version` leaves that set unchanged and is
# silent; editing `v3`→`v4` changes it and fires. It stays a plain grep over
# `git show` (advisory only) — an AST-aware check is the wrong altitude for a
# hook. When git is unavailable it degrades to the old presence grep.
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

# Fire iff the *set* of tokens matching $1 differs between the committed (HEAD)
# and working-tree copies of $2 — i.e. this change added, removed, or altered
# one. Editing a file that merely contains an unchanged token is silent. With no
# git checkout available, falls back to a presence grep (the old behavior). A
# new/untracked file treats its whole token set as introduced.
tokens_changed() {
	local re="$1" path="$2" toplevel rel work head
	if ! command -v git >/dev/null 2>&1; then
		grep -Eq "$re" "$path"
		return
	fi
	toplevel="$(git -C "$(dirname "$path")" rev-parse --show-toplevel 2>/dev/null || true)"
	if [ -z "$toplevel" ]; then
		grep -Eq "$re" "$path"
		return
	fi
	rel="${path#"$toplevel"/}"
	work="$(grep -Eo "$re" "$path" 2>/dev/null | sort -u || true)"
	if git -C "$toplevel" cat-file -e "HEAD:$rel" 2>/dev/null; then
		head="$(git -C "$toplevel" show "HEAD:$rel" 2>/dev/null | grep -Eo "$re" | sort -u || true)"
	else
		head=""
	fi
	[ "$work" != "$head" ]
}

msgs=()

# Match the version and command-name *value* literals, not bare identifier
# references, so importing/using a constant by name does not fire. The
# mirrored contract values and the retrieval-only policy version get separate,
# accurate reminders.
contract_version_re='canonical\.expr\.v[0-9]|features\.roles\.v[0-9]|features\.role_key\.v[0-9]|lean-semantic-search\.capability\.v[0-9]|(declaration|proof_goal)_features\.v[0-9]|"(declaration|proof_goal)_features"'
retrieval_version_re='lean-semantic-search\.retrieval\.v[0-9]'
if tokens_changed "$contract_version_re" "$file"; then
	msgs+=("$(
		cat <<EOF
• You changed a contract version or command name in $file. These are
  mirrored across the Lean↔Rust boundary and have no cross-language test, so
  move all three together:
    - crates/contract/src/lib.rs constants: CAPABILITY_SCHEMA_VERSION,
      DECLARATION_FEATURES_COMMAND, DECLARATION_FEATURE_COMMAND_VERSION,
      PROOF_GOAL_FEATURES_COMMAND, PROOF_GOAL_FEATURE_COMMAND_VERSION,
      CANONICAL_FEATURE_VERSION, SEMANTIC_FEATURE_VERSION
    - lean/LeanSemanticSearch/Json.lean defs: schemaVersion,
      declarationCommand, declarationFeatureCommandVersion, proofGoalCommand,
      proofGoalFeatureCommandVersion, semanticFeatureVersion, and
      roleKeyVersion (Lean only)
    - docs/architecture/01-capability-contract.md
EOF
	)")
fi
if tokens_changed "$retrieval_version_re" "$file"; then
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

if [ "${#msgs[@]}" -gt 0 ]; then
	printf 'contract-guard:\n' >&2
	printf '%s\n' "${msgs[@]}" >&2
	exit 2
fi

exit 0
