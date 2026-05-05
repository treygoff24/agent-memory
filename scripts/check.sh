#!/usr/bin/env bash
set -euo pipefail

# Optional rustc caching. `brew install sccache` (or `cargo install sccache`).
# Missing sccache is fine — we just don't set the wrapper.
if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="sccache"
fi

# Optional faster test runner. `brew install cargo-nextest` (or `cargo install cargo-nextest`).
# nextest skips doctests by design, so when we use it we run `cargo test --doc` separately.
if command -v cargo-nextest >/dev/null 2>&1; then
  USE_NEXTEST=1
else
  USE_NEXTEST=0
fi

# --- Phase 1: cheap independent checks, fanned out in parallel -------------
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
run_parallel baseline         ./scripts/check-baseline-discipline.sh
if command -v specgate >/dev/null 2>&1; then
  run_parallel specgate-validate  specgate validate
  run_parallel specgate-check     specgate check --output-mode deterministic
  run_parallel specgate-doctor    specgate doctor ownership --project-root . --format json
else
  echo "warning: specgate not installed; skipping specgate gates" >&2
fi
wait_parallel

# --- Phase 2: cargo-heavy work, serial (shared target dir) -----------------

cargo clippy --workspace --all-targets --all-features -- -D warnings

if [[ "$USE_NEXTEST" -eq 1 ]]; then
  cargo nextest run --workspace
  cargo nextest run --workspace --release
  # nextest doesn't run doctests; cover them so we don't lose coverage vs `cargo test`.
  cargo test --workspace --doc
else
  cargo test --workspace
  cargo test --workspace --release
fi

RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# --- Phase 3: convergence / durability / bench (perf-sensitive, serial) ----

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
