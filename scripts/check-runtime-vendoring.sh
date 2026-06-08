#!/usr/bin/env bash
# Prove the packaged runtime crate payload matches the canonical Lean runtime
# file set, then build that packaged payload as a downstream host would.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_ROOT="$REPO_ROOT/crates/runtime/runtime"
TMPDIR_ROOT="${TMPDIR:-/tmp}"
WORKDIR="$(mktemp -d "${TMPDIR_ROOT%/}/lean-semantic-search-runtime.XXXXXX")"

cleanup() {
	rm -rf "$WORKDIR"
}
trap cleanup EXIT

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		printf 'required command not found: %s%s\n' "$1" "${2:+ ($2)}" >&2
		exit 2
	fi
}

active_toolchain() {
	if command -v elan >/dev/null 2>&1; then
		local toolchain
		toolchain="$(
			elan show 2>/dev/null |
				awk '
					/^active toolchain$/ { active = 1; next }
					active && /^-+$/ { next }
					active && NF { print $1; exit }
				'
		)"
		if [[ -n "$toolchain" ]]; then
			printf '%s\n' "$toolchain"
			return 0
		fi
	fi

	local version
	version="$(lean --version | sed -n 's/^Lean (version \([^,)]*\).*/\1/p')"
	if [[ -n "$version" ]]; then
		printf 'leanprover/lean4:v%s\n' "$version"
		return 0
	fi

	printf 'could not determine a Lean toolchain from elan or lean --version\n' >&2
	return 1
}

compare_file() {
	local source="$1"
	local packaged="$2"
	if ! cmp -s "$REPO_ROOT/$source" "$RUNTIME_ROOT/$packaged"; then
		printf 'runtime payload differs from canonical source: %s -> %s\n' "$source" "$packaged" >&2
		diff -u "$REPO_ROOT/$source" "$RUNTIME_ROOT/$packaged" >&2 || true
		exit 1
	fi
}

copy_packaged_file() {
	local src="$1"
	mkdir -p "$WORKDIR/$(dirname "$src")"
	cp "$RUNTIME_ROOT/$src" "$WORKDIR/$src"
}

require_cmd git
require_cmd lake "install via elan + leanprover/lean4"
require_cmd lean "install via elan + leanprover/lean4"

compare_file lean/lakefile.lean lakefile.lean
compare_file lean/lake-manifest.json lake-manifest.json
compare_file lean/LeanSemanticSearch.lean LeanSemanticSearch.lean
compare_file lean/README.md README.md
compare_file lean/VENDORING.md VENDORING.md
compare_file LICENSE-APACHE LICENSE-APACHE
compare_file LICENSE-MIT LICENSE-MIT

if ! diff -ru "$REPO_ROOT/lean/LeanSemanticSearch" "$RUNTIME_ROOT/LeanSemanticSearch"; then
	printf 'runtime LeanSemanticSearch/ payload differs from canonical source\n' >&2
	exit 1
fi

for required in \
	lakefile.lean \
	lake-manifest.json \
	LeanSemanticSearch.lean \
	LeanSemanticSearch/Capability.lean \
	README.md \
	VENDORING.md \
	LICENSE-APACHE \
	LICENSE-MIT
do
	if [[ ! -f "$RUNTIME_ROOT/$required" ]]; then
		printf 'runtime payload is missing required path: %s\n' "$required" >&2
		exit 1
	fi
done

for excluded in \
	.lake \
	lean-toolchain \
	Main.lean \
	LeanSemanticSearchTest.lean \
	LeanSemanticSearchTest
do
	if [[ -e "$RUNTIME_ROOT/$excluded" ]]; then
		printf 'runtime payload unexpectedly contains excluded path: %s\n' "$excluded" >&2
		exit 1
	fi
done

if find "$RUNTIME_ROOT" \( -name '*.olean' -o -name '*.ilean' -o -name '*.c' -o -name '*.so' -o -name '*.dylib' \) -print -quit |
	grep -q .
then
	printf 'runtime payload unexpectedly contains build artifacts\n' >&2
	exit 1
fi

(
	cd "$RUNTIME_ROOT"
	find . -type f -print0
) |
	while IFS= read -r -d '' path; do
		copy_packaged_file "${path#./}"
	done

printf '%s\n' "$(active_toolchain)" >"$WORKDIR/lean-toolchain"

(
	cd "$WORKDIR"
	lake build LeanSemanticSearch
)
