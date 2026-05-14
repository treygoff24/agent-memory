#!/usr/bin/env bash
set -euo pipefail

started_at="$(date +%s)"

finish() {
  local ended_at
  ended_at="$(date +%s)"
  echo "check-fast duration: $((ended_at - started_at))s"
}
trap finish EXIT

phase() {
  echo
  echo "==> $*"
}

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${MEMORUM_CHECK_JOBS:-2}}"
echo "using CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS"

if [[ "${MEMORUM_CHECK_USE_SCCACHE:-0}" == "1" ]] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  echo "using RUSTC_WRAPPER=$RUSTC_WRAPPER"
else
  unset RUSTC_WRAPPER
fi

phase "rustfmt"
cargo fmt --all -- --check

phase "shell syntax"
# Paths are repo-relative; this script (and the others under scripts/) assume
# CWD is the repo root.
while IFS= read -r script; do
  bash -n "$script"
done < <(find scripts -maxdepth 1 -type f -name '*.sh' | sort)

phase "cargo metadata"
cargo metadata --locked --format-version=1 --no-deps >/dev/null

if [[ "${MEMORUM_CHECK_FAST_COMPILE:-0}" == "1" ]]; then
  phase "optional rust compile check"
  # Exclude memoryd-web from the fast Rust compile path because its build.rs
  # shells out to the dashboard frontend production build. Also avoid
  # --all-targets here: test/example/bench target compilation belongs in
  # check:local, not the inner-loop gate.
  cargo check --workspace --exclude memoryd-web --locked
else
  echo "skipping Rust compile in check-fast; run targeted cargo check/test or check:local before claiming completion"
fi

if [[ "${MEMORUM_CHECK_FAST_FORMAT:-0}" == "1" ]]; then
  if command -v pnpm >/dev/null 2>&1 && [ -f package.json ]; then
    phase "oxfmt"
    pnpm exec oxfmt --check --ignore-path .oxfmtignore .

    phase "oxlint"
    pnpm exec oxlint .
  else
    echo "warning: pnpm/package.json unavailable; skipping JS format/lint checks" >&2
  fi
else
  echo "skipping whole-repo oxfmt/oxlint in check-fast; check:local/check:full run them"
fi

if command -v specgate >/dev/null 2>&1; then
  phase "specgate validate"
  specgate validate
else
  echo "warning: specgate not installed; skipping specgate validate" >&2
fi

phase "docs command validity"
./scripts/docs-command-validity.sh

phase "baseline discipline"
./scripts/check-baseline-discipline.sh

echo
echo "check-fast passed"
