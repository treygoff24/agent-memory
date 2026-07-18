#!/usr/bin/env bash
set -euo pipefail

export MEMORUM_DYNAMICS=off

# Long Rust gates on macOS can wedge freshly built executables behind
# syspolicyd/CSExattrCrypto when they use the repo target dir or sccache.
# Use an isolated target dir by default and make the risky accelerators opt-in.
cleanup_cargo_target_dir=0
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  # Portable across BSD and GNU mktemp (GNU requires >=3 X's in the template).
  CARGO_TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/memorum-check-target.XXXXXXXX")"
  export CARGO_TARGET_DIR
  cleanup_cargo_target_dir=1
fi

cleanup() {
  if [[ "$cleanup_cargo_target_dir" -eq 1 && "${MEMORUM_CHECK_KEEP_TARGET:-0}" != "1" ]]; then
    rm -rf "$CARGO_TARGET_DIR"
  fi
}
trap cleanup EXIT

# Optional rustc caching. Disabled unless explicitly requested because it is
# part of the known local macOS gate hang pattern.
if [[ "${MEMORUM_CHECK_USE_SCCACHE:-0}" == "1" ]] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="sccache"
else
  unset RUSTC_WRAPPER
fi

# Optional faster test runner. `brew install cargo-nextest` (or `cargo install cargo-nextest`).
# nextest skips doctests by design, so when we use it we run `cargo test --doc` separately.
if [[ "${MEMORUM_CHECK_USE_NEXTEST:-0}" == "1" ]] && command -v cargo-nextest >/dev/null 2>&1; then
  USE_NEXTEST=1
else
  USE_NEXTEST=0
fi

# Optional: run the cargo-heavy lint/test/doc phase at background QoS so a full
# gate doesn't roast the machine while you keep working. On Apple Silicon this
# schedules the work onto efficiency cores. OFF by default; deliberately NOT
# applied to the perf-sensitive bench phase below — throttling it would corrupt
# the baselines. `MEMORUM_CHECK_NICE=1 bash scripts/check.sh` to opt in.
if [[ "${MEMORUM_CHECK_NICE:-0}" == "1" ]] && command -v taskpolicy >/dev/null 2>&1; then
  NICE="taskpolicy -b"
else
  NICE=""
fi

# Phase 1: cheap independent checks, fanned out in parallel
# Each step writes to its own log; we surface the log only on failure so a
# clean run stays readable.

declare -a parallel_pids=()
declare -a parallel_logs=()
declare -a parallel_names=()

run_parallel() {
  local name="$1"; shift
  local log
  log=$(mktemp -t "check-${name}.XXXXXX")
  ( "$@" >"$log" 2>&1 ) &
  parallel_pids+=("$!")
  parallel_logs+=("$log")
  parallel_names+=("$name")
}

wait_parallel() {
  local fail=0 i
  for i in "${!parallel_pids[@]}"; do
    if wait "${parallel_pids[$i]}"; then
      echo "[ok] ${parallel_names[$i]}"
    else
      echo "[FAIL] ${parallel_names[$i]} failed:" >&2
      cat "${parallel_logs[$i]}" >&2
      fail=1
    fi
    rm -f "${parallel_logs[$i]}"
  done
  parallel_pids=()
  parallel_logs=()
  parallel_names=()
  if [[ "$fail" -ne 0 ]]; then
    exit 1
  fi
}

run_parallel fmt              cargo fmt --all -- --check
if command -v pnpm >/dev/null 2>&1 && [ -f package.json ]; then
  run_parallel oxfmt          pnpm exec oxfmt --check --ignore-path .oxfmtignore .
  run_parallel oxlint         pnpm exec oxlint .
else
  echo "warning: pnpm/package.json unavailable; skipping JS format/lint checks" >&2
fi
run_parallel docs-validity    ./scripts/docs-command-validity.sh
# Dependency-CVE scan over Cargo.lock (RUSTSEC advisories). Optional tool, so we
# only run it when present — `cargo install cargo-audit` (or cargo-deny) to opt
# in. cargo-audit needs network to refresh its advisory DB; a fetch failure is
# treated as a skip (warning, not gate failure) so offline runs stay green,
# while an actual vulnerable-dependency finding fails the gate.
if command -v cargo-audit >/dev/null 2>&1; then
  run_parallel cargo-audit    ./scripts/cargo-audit-gate.sh
elif command -v cargo-deny >/dev/null 2>&1; then
  run_parallel cargo-deny-root cargo deny check advisories
  run_parallel cargo-deny-fuzz bash -lc 'cd fuzz && cargo deny check advisories'
else
  echo "warning: neither cargo-audit nor cargo-deny installed; skipping dependency-CVE scan" >&2
fi
run_parallel installer-test   ./scripts/install-memorum.test.sh
run_parallel baseline         ./scripts/check-baseline-discipline.sh
if command -v specgate >/dev/null 2>&1; then
  run_parallel specgate-validate  specgate validate
  run_parallel specgate-check     specgate check --output-mode deterministic
  run_parallel specgate-doctor    specgate doctor ownership --project-root . --format json
else
  echo "warning: specgate not installed; skipping specgate gates" >&2
fi
wait_parallel

# Phase 2: cargo-heavy work, serial (shared target dir)

$NICE cargo clippy --workspace --all-targets --all-features -- -D warnings

# memoryd-web gates its dashboard fixtures behind the non-default `dev-fixtures`
# feature so production builds can never embed fake numbers. The test suite needs
# those fixtures, so enable the feature for the workspace test runs (production
# `cargo install`/`cargo build` stays feature-off and fixture-free). memoryd's
# dev-fixtures feature is also needed so gated bench-bin unit tests are built
# and run by the gate without exposing those bins to feature-off installs.
TEST_FIXTURE_FEATURES="memoryd-web/dev-fixtures,memoryd/dev-fixtures"

if [[ "$USE_NEXTEST" -eq 1 ]]; then
  $NICE cargo nextest run --workspace --features "$TEST_FIXTURE_FEATURES"
  $NICE cargo nextest run --workspace --release --features "$TEST_FIXTURE_FEATURES"
  # nextest doesn't run doctests; cover them so we don't lose coverage vs `cargo test`.
  $NICE cargo test --workspace --doc --features "$TEST_FIXTURE_FEATURES"
else
  $NICE cargo test --workspace --features "$TEST_FIXTURE_FEATURES"
  $NICE cargo test --workspace --release --features "$TEST_FIXTURE_FEATURES"
fi

$NICE env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Phase 3: convergence / durability / bench (perf-sensitive, serial)

./scripts/rust-boundary-check.sh
./scripts/two-clone-convergence.sh --full
BENCH_PROFILE="${BENCH_PROFILE:-$(./scripts/detect-bench-profile.sh)}"
./scripts/durability-probe-gate.sh --matrix apfs,tmpfs,ext4,einval,best-effort --output bench/durability-results.json
./scripts/bench-gate.sh --tier smoke --profile "$BENCH_PROFILE" --output "bench/results.${BENCH_PROFILE}.smoke.json"
./scripts/bench-gate.sh --tier release --profile "$BENCH_PROFILE" --output "bench/results.${BENCH_PROFILE}.json"
./scripts/bench-regression-check.sh \
  --profile "$BENCH_PROFILE" \
  --results "bench/results.${BENCH_PROFILE}.json" \
  --baseline "bench/baseline.${BENCH_PROFILE}.json"
