#!/usr/bin/env bash
set -euo pipefail

started_at="$(date +%s)"

finish() {
  local ended_at
  ended_at="$(date +%s)"
  echo "check-local duration: $((ended_at - started_at))s"
}
trap finish EXIT

phase() {
  echo
  echo "==> $*"
}

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${MEMORUM_CHECK_JOBS:-4}}"
export MEMORUM_CHECK_JOBS="$CARGO_BUILD_JOBS"
export MEMORUM_TEST_THREADS="${MEMORUM_TEST_THREADS:-2}"
echo "using CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS"
echo "using MEMORUM_TEST_THREADS=$MEMORUM_TEST_THREADS"

if [[ "${MEMORUM_CHECK_USE_SCCACHE:-0}" == "1" ]] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  echo "using RUSTC_WRAPPER=$RUSTC_WRAPPER"
else
  unset RUSTC_WRAPPER
fi

phase "fast gate"
./scripts/check-fast.sh

if command -v pnpm >/dev/null 2>&1 && [ -f package.json ]; then
  phase "root oxfmt"
  pnpm exec oxfmt --check --ignore-path .oxfmtignore .

  phase "root oxlint"
  pnpm exec oxlint .
else
  echo "warning: pnpm/package.json unavailable; skipping JS format/lint checks" >&2
fi

if command -v specgate >/dev/null 2>&1; then
  phase "specgate check"
  specgate check --output-mode deterministic

  phase "specgate doctor"
  specgate doctor ownership --project-root . --format json
else
  echo "warning: specgate not installed; skipping specgate check/doctor" >&2
fi

phase "rust clippy"
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

phase "rust tests"
cargo test --workspace --locked -- --test-threads="$MEMORUM_TEST_THREADS"

phase "rust docs"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

phase "rust boundary smoke"
./scripts/rust-boundary-check.sh

phase "two-clone convergence smoke"
./scripts/two-clone-convergence.sh --smoke

if [[ "${MEMORUM_CHECK_FRONTEND:-1}" == "1" && -f crates/memoryd-web/frontend/package.json ]]; then
  phase "dashboard frontend local gate"
  (cd crates/memoryd-web/frontend && pnpm run check:local)
else
  echo "warning: dashboard frontend local gate skipped; report why if frontend was in scope" >&2
fi

echo
echo "check-local passed"
