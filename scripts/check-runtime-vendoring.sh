#!/usr/bin/env bash
# Materialize the Lean runtime payload exactly as a downstream host would vendor
# it, then prove that package builds without tests, upstream toolchain pins, or
# build artifacts.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

copy_runtime_file() {
	local src="$1"
	local dst
	case "$src" in
	lean/*)
		dst="${src#lean/}"
		;;
	LICENSE-APACHE | LICENSE-MIT)
		dst="$src"
		;;
	*)
		printf 'unexpected runtime source path: %s\n' "$src" >&2
		return 1
		;;
	esac
	mkdir -p "$WORKDIR/$(dirname "$dst")"
	cp "$REPO_ROOT/$src" "$WORKDIR/$dst"
}

require_cmd git
require_cmd lake "install via elan + leanprover/lean4"
require_cmd lean "install via elan + leanprover/lean4"

cd "$REPO_ROOT"

git ls-files -z --cached --others --exclude-standard -- \
	lean/lakefile.lean \
	lean/lake-manifest.json \
	lean/LeanSemanticSearch.lean \
	'lean/LeanSemanticSearch/**' \
	lean/README.md \
	lean/VENDORING.md \
	LICENSE-APACHE \
	LICENSE-MIT |
	while IFS= read -r -d '' path; do
		copy_runtime_file "$path"
	done

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
	if [[ ! -f "$WORKDIR/$required" ]]; then
		printf 'runtime payload is missing required path: %s\n' "$required" >&2
		exit 1
	fi
done

if [[ -e "$WORKDIR/lean-toolchain" ]]; then
	printf 'runtime payload copied upstream lean/lean-toolchain instead of generating one\n' >&2
	exit 1
fi

for excluded in \
	.lake \
	Main.lean \
	LeanSemanticSearchTest.lean \
	LeanSemanticSearchTest
do
	if [[ -e "$WORKDIR/$excluded" ]]; then
		printf 'runtime payload unexpectedly contains excluded path: %s\n' "$excluded" >&2
		exit 1
	fi
done

if find "$WORKDIR" \( -name '*.olean' -o -name '*.ilean' -o -name '*.c' -o -name '*.so' -o -name '*.dylib' \) -print -quit |
	grep -q .
then
	printf 'runtime payload unexpectedly contains build artifacts\n' >&2
	exit 1
fi

printf '%s\n' "$(active_toolchain)" >"$WORKDIR/lean-toolchain"

(
	cd "$WORKDIR"
	lake build LeanSemanticSearch
)
