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

if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  echo "using RUSTC_WRAPPER=$RUSTC_WRAPPER"
else
  echo "warning: sccache not installed; Rust compilation will not use sccache" >&2
fi

if command -v cargo-nextest >/dev/null 2>&1; then
  echo "cargo-nextest detected; dogfood gates stay on targeted cargo test commands"
else
  echo "warning: cargo-nextest not installed; not needed for check-fast" >&2
fi

phase "rustfmt"
cargo fmt --all -- --check

phase "shell syntax"
# Paths are repo-relative; this script (and the others under scripts/) assume CWD is
# the repo root, matching how check.sh and check-dogfood.sh are invoked.
bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh scripts/install-launchd.sh scripts/docs-command-validity.sh scripts/install-memorum.test.sh scripts/install-launchd.test.sh

phase "docs command validity"
./scripts/docs-command-validity.sh

phase "installer test"
./scripts/install-memorum.test.sh

phase "targeted dogfood clippy"
cargo clippy -p memoryd -p memoryd-tui -p memorum-eval -p memorum-coordination --all-targets -- -D warnings

phase "live-harness test compile"
cargo check -p memorum-eval --features live-harness --tests --locked

if command -v pnpm >/dev/null 2>&1 && [ -f package.json ]; then
  phase "oxfmt"
  pnpm exec oxfmt --check --ignore-path .oxfmtignore .

  phase "oxlint"
  pnpm exec oxlint .
else
  echo "warning: pnpm/package.json unavailable; skipping JS format/lint checks" >&2
fi

if command -v specgate >/dev/null 2>&1; then
  phase "specgate validate"
  specgate validate

  phase "specgate check"
  specgate check --output-mode deterministic

  phase "specgate doctor"
  specgate doctor ownership --project-root . --format json
else
  echo "warning: specgate not installed; skipping specgate gates" >&2
fi

phase "baseline discipline"
./scripts/check-baseline-discipline.sh

echo
echo "check-fast passed"
