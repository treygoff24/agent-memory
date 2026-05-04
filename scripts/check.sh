#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --release
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
pnpm exec oxfmt --check --ignore-path .oxfmtignore .
pnpm exec oxlint .
if command -v specgate >/dev/null 2>&1; then
  specgate validate
  specgate check --output-mode deterministic
  specgate doctor ownership --project-root . --format json
else
  echo "warning: specgate not installed; skipping specgate gates" >&2
fi
./scripts/rust-boundary-check.sh
./scripts/check-baseline-discipline.sh
./scripts/two-clone-convergence.sh --full
BENCH_PROFILE="${BENCH_PROFILE:-$(./scripts/detect-bench-profile.sh)}"
./scripts/durability-probe-gate.sh --matrix apfs,tmpfs,ext4,einval,best-effort --output bench/durability-results.json
./scripts/bench-gate.sh --tier smoke --profile "$BENCH_PROFILE" --output "bench/results.${BENCH_PROFILE}.smoke.json"
./scripts/bench-gate.sh --tier release --profile "$BENCH_PROFILE" --output "bench/results.${BENCH_PROFILE}.json"
./scripts/bench-regression-check.sh \
  --profile "$BENCH_PROFILE" \
  --results "bench/results.${BENCH_PROFILE}.json" \
  --baseline "bench/baseline.${BENCH_PROFILE}.json"
