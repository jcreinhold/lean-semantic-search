#!/usr/bin/env bash
# Prove the packaged runtime crate payload matches the canonical Lean runtime
# file set and that `cargo package` ships it. The payload is byte-identical to
# `lean/`, so `lake -d lean build` already compiles it; the opt-in runtime build
# test builds the materialized package.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_ROOT="$REPO_ROOT/crates/runtime/runtime"

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		printf 'required command not found: %s%s\n' "$1" "${2:+ ($2)}" >&2
		exit 2
	fi
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

require_cmd cargo

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

package_list="$(cd "$REPO_ROOT" && cargo package -p lean-semantic-search-runtime --list --allow-dirty)"
for required in \
	runtime/lakefile.lean \
	runtime/lake-manifest.json \
	runtime/LeanSemanticSearch.lean \
	runtime/LeanSemanticSearch/Capability.lean \
	runtime/README.md \
	runtime/VENDORING.md \
	runtime/LICENSE-APACHE \
	runtime/LICENSE-MIT
do
	if ! printf '%s\n' "$package_list" | grep -Fxq "$required"; then
		printf 'cargo package output is missing runtime payload path: %s\n' "$required" >&2
		exit 1
	fi
done
