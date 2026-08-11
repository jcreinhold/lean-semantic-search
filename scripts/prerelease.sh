#!/usr/bin/env bash
# scripts/prerelease.sh — run every pre-release gate locally.
#
# This is the local mirror of `.github/workflows/release.yml`: it runs
# the same gates as the `verify` job (fmt, clippy, rustdoc, nextest,
# cargo-deny, lake build/test, runtime vendoring) plus the `publish` job's preflight checks
# (tag/version ↔ CHANGELOG consistency and a `cargo publish --workspace
# --dry-run`), and the docs/toml policy checks `ci.yml` runs
# (actionlint, mdwright, taplo). Run it before tagging a `vX.Y.Z`
# release so CI surprises become local failures.
#
# Unlike the FFI-heavy sibling crates, the Rust crates here have no link
# to Lean — there is no LEAN_SYSROOT/LD_LIBRARY_PATH wiring. The Lean
# package builds independently via `lake`, pinned by `lean/lean-toolchain`.
#
# Optional tools (actionlint, taplo, mdwright) are skipped with a notice
# when absent rather than failing the run.
#
# Usage:
#   scripts/prerelease.sh                # all gates
#   scripts/prerelease.sh --quick        # skip the slow `cargo publish` dry-run
#   scripts/prerelease.sh --no-publish   # alias for --quick
#   scripts/prerelease.sh --help

set -euo pipefail

# -- defaults ---------------------------------------------------------------

RUN_PUBLISH=1
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The crate whose version anchors the tag check; all workspace crates
# share `[workspace.package] version`, so any member would do.
VERSION_ANCHOR_CRATE="lean-semantic-search-contract"

# Files the AGENTS.md markdown policy covers (mirrors ci.yml's policy job).
MDWRIGHT_TARGETS=(README.md AGENTS.md docs/architecture/*.md crates/*/README.md lean/README.md lean/VENDORING.md)

# -- logging ----------------------------------------------------------------

# ANSI styling, suppressed when stdout is not a TTY (CI logs, redirects).
if [[ -t 1 ]]; then
	BOLD=$'\033[1m'
	GREEN=$'\033[32m'
	RED=$'\033[31m'
	YELLOW=$'\033[33m'
	RESET=$'\033[0m'
else
	BOLD=""
	GREEN=""
	RED=""
	YELLOW=""
	RESET=""
fi

log_step() { printf '\n%s==>%s %s%s%s\n' "$BOLD" "$RESET" "$BOLD" "$*" "$RESET"; }
log_ok() { printf '%s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
log_warn() { printf '%s!%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
log_err() { printf '%s✗%s %s\n' "$RED" "$RESET" "$*" >&2; }

# -- arg parsing ------------------------------------------------------------

usage() {
	sed -n '2,/^$/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--quick | --no-publish)
		RUN_PUBLISH=0
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		log_err "unknown argument: $1"
		usage >&2
		exit 2
		;;
	esac
done

# -- host preflight ---------------------------------------------------------

require_cmd() {
	if ! command -v "$1" >/dev/null 2>&1; then
		log_err "required command not found: $1${2:+ ($2)}"
		exit 2
	fi
}

require_cmd cargo "install via https://rustup.rs"
require_cmd cargo-nextest "cargo install cargo-nextest --locked"
require_cmd cargo-deny "cargo install cargo-deny --locked"
require_cmd lake "install via elan + leanprover/lean4"
require_cmd python3
require_cmd git

cd "$REPO_ROOT"

# -- gate runner ------------------------------------------------------------

declare -a PASSED=()
declare -a FAILED=()
declare -a SKIPPED=()

# `run_gate NAME COMMAND ARGS...` runs the gate, streams its output
# live, and records pass/fail. The run continues after a failure so a
# single pass surfaces every problem; the summary exits non-zero if any
# gate failed.
run_gate() {
	local name="$1"
	shift
	log_step "$name"
	local start=$SECONDS
	if "$@"; then
		log_ok "$name ($((SECONDS - start))s)"
		PASSED+=("$name")
	else
		local rc=$?
		log_err "$name FAILED in $((SECONDS - start))s (exit $rc)"
		FAILED+=("$name")
	fi
}

# -- gates ------------------------------------------------------------------

run_gate "lake -d lean build" \
	lake -d lean build

run_gate "lake -d lean test" \
	lake -d lean test

run_gate "scripts/check-runtime-vendoring.sh" \
	scripts/check-runtime-vendoring.sh

run_gate "cargo fmt --all --check" \
	cargo fmt --all --check

run_gate "cargo clippy --all-targets -- -D warnings" \
	cargo clippy --all-targets -- -D warnings

# rustdoc lints (broken intra-doc links, redundant explicit link targets)
# are invisible to clippy but degrade the published docs.rs pages, and a
# tagged crates.io version is immutable — catch them before tagging.
run_gate "cargo doc --workspace --no-deps (rustdoc lints)" \
	env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

run_gate "cargo nextest run --workspace --profile ci" \
	cargo nextest run --workspace --profile ci --locked

run_gate "cargo deny check" \
	cargo deny check

# Tag ↔ version ↔ CHANGELOG consistency: the same check release.yml's
# publish job runs, hoisted local so a missing CHANGELOG section fails
# before you tag instead of after. Reads the workspace version and
# asserts a non-empty `## [version]` section exists in CHANGELOG.md.
check_changelog() {
	local version
	version="$(cargo metadata --no-deps --format-version 1 |
		python3 -c "import json,sys; m=json.load(sys.stdin); print(next(p['version'] for p in m['packages'] if p['name']=='${VERSION_ANCHOR_CRATE}'))")"
	log_step "workspace version: v${version}"
	local body
	body="$(awk -v ver="$version" '
		$0 ~ "^## \\[" ver "\\]" { capture = 1; next }
		capture && /^## \[/ { exit }
		capture { print }
	' CHANGELOG.md)"
	if [[ -z "${body//[[:space:]]/}" ]]; then
		log_err "No non-empty '## [${version}]' section in CHANGELOG.md; add one before tagging v${version}."
		return 1
	fi
	printf '%s\n' "$body"
	return 0
}
run_gate "CHANGELOG has a section for the workspace version" \
	check_changelog

# -- optional policy gates --------------------------------------------------

if command -v actionlint >/dev/null 2>&1; then
	run_gate "actionlint workflows" \
		actionlint .github/workflows/ci.yml .github/workflows/release.yml
else
	log_warn "actionlint not installed; skipping (install: go install github.com/rhysd/actionlint/cmd/actionlint@latest)"
	SKIPPED+=("actionlint workflows")
fi

if command -v taplo >/dev/null 2>&1; then
	run_gate "taplo fmt --check" \
		taplo fmt --check
else
	log_warn "taplo not installed; skipping (install: cargo install taplo-cli --locked)"
	SKIPPED+=("taplo fmt --check")
fi

if command -v mdwright >/dev/null 2>&1; then
	run_gate "mdwright fmt --check" \
		mdwright fmt --check "${MDWRIGHT_TARGETS[@]}"
else
	log_warn "mdwright not installed; skipping (install: cargo binstall mdwright)"
	SKIPPED+=("mdwright fmt --check")
fi

# -- publish dry-run (slow; network-bound) ----------------------------------

if [[ "$RUN_PUBLISH" == 1 ]]; then
	run_gate "cargo publish --workspace --dry-run" \
		cargo publish --workspace --dry-run --locked
else
	SKIPPED+=("cargo publish --workspace --dry-run (--quick)")
fi

# -- summary ----------------------------------------------------------------

printf '\n%s====== Pre-release summary ======%s\n' "$BOLD" "$RESET"
printf '\npassed (%d):\n' "${#PASSED[@]}"
for name in "${PASSED[@]}"; do printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$name"; done

if ((${#SKIPPED[@]} > 0)); then
	printf '\nskipped (%d):\n' "${#SKIPPED[@]}"
	for name in "${SKIPPED[@]}"; do printf '  %s-%s %s\n' "$YELLOW" "$RESET" "$name"; done
fi

if ((${#FAILED[@]} > 0)); then
	printf '\n%sfailed (%d):%s\n' "$RED" "${#FAILED[@]}" "$RESET"
	for name in "${FAILED[@]}"; do printf '  %s✗%s %s\n' "$RED" "$RESET" "$name"; done
	exit 1
fi

printf '\n%sAll gates passed.%s\n' "$GREEN" "$RESET"
