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
#   1. VERSION LOCKSTEP. Schema/algorithm version strings are centralized
#      as constants in `crates/contract` (mirrored by `RETRIEVAL_POLICY_VERSION`
#      in `crates/retrieval`) and mirrored as defs in the Lean `Json`
#      module. Touching a version literal or a *_VERSION constant means the
#      mirror side AND docs/architecture/01-capability-contract.md must move
#      in the same change. There is no cross-language test that catches drift.
#
#   2. EXPORT/COMMAND LOCKSTEP. The five `@[export lean_semantic_search_*]`
#      functions in Capability.lean correspond one-to-one to the
#      *_EXPORT / *_COMMAND constants in crates/capability/src/lib.rs and
#      the advertised commands in the contract doc. Rename/add/remove one
#      and all three must move together.
#
# KNOWN TRADEOFF: these are plain greps over the edited file, so they can
# false-positive (e.g. editing a comment or test that mentions a version
# literal). They are advisory only, so a rare extra nudge is cheap; an
# AST-aware check is the wrong altitude for a hook.
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

# 1. Version lockstep. Fires on the canonical version-owning files, or on
#    any in-scope file that contains a known version literal / *_VERSION
#    constant.
version_literal_re='canonical\.expr\.v[0-9]|features\.roles\.v[0-9]|features\.role_key\.v[0-9]|lean-semantic-search\.[a-z]+\.v[0-9]|(declaration|proof_goal)_features\.v[0-9]|_VERSION\b'
case "$file" in
*/crates/contract/src/lib.rs | crates/contract/src/lib.rs | \
	*/crates/retrieval/src/lib.rs | crates/retrieval/src/lib.rs | \
	*/lean/LeanSemanticSearch/Json.lean | lean/LeanSemanticSearch/Json.lean)
	touched_version=1
	;;
*)
	if grep -Eq "$version_literal_re" "$file"; then
		touched_version=1
	else
		touched_version=0
	fi
	;;
esac
if [ "${touched_version:-0}" = 1 ]; then
	msgs+=("• You touched a schema/algorithm version in $file. Per CLAUDE.md these are a contract: the version constants in crates/contract (and RETRIEVAL_POLICY_VERSION in crates/retrieval) are mirrored by the defs in lean/LeanSemanticSearch/Json.lean. Update BOTH sides AND docs/architecture/01-capability-contract.md in the same change. Rust: CAPABILITY_SCHEMA_VERSION / DECLARATION_FEATURE_COMMAND_VERSION / PROOF_GOAL_FEATURE_COMMAND_VERSION / CANONICAL_FEATURE_VERSION / SEMANTIC_FEATURE_VERSION. Lean: schemaVersion / declarationFeatureCommandVersion / proofGoalFeatureCommandVersion / semanticFeatureVersion / roleKeyVersion.")
fi

# 2. Export/command lockstep. Only the two files that own the worker ABI,
#    and only when the body touches an export/command symbol.
case "$file" in
*/lean/LeanSemanticSearch/Capability.lean | lean/LeanSemanticSearch/Capability.lean | \
	*/crates/capability/src/lib.rs | crates/capability/src/lib.rs)
	if grep -Eq '@\[export[[:space:]]+lean_semantic_search_|_EXPORT\b|_COMMAND\b' "$file"; then
		msgs+=("• You touched a worker export/command name in $file. The five @[export lean_semantic_search_*] functions in Capability.lean correspond one-to-one to the *_EXPORT / *_COMMAND constants in crates/capability/src/lib.rs and the advertised commands in docs/architecture/01-capability-contract.md. Change all three together (renaming, adding, or removing an entry).")
	fi
	;;
esac

if [ "${#msgs[@]}" -gt 0 ]; then
	printf 'contract-guard:\n' >&2
	printf '%s\n' "${msgs[@]}" >&2
	exit 2
fi

exit 0
