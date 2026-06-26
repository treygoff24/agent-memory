#!/usr/bin/env bash
set -euo pipefail
tier=""; profile=""; output=""
while [ $# -gt 0 ]; do
  case "$1" in
    --tier) tier="${2:?}"; shift ;;
    --profile) profile="${2:?}"; shift ;;
    --output) output="${2:?}"; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$tier" ] && [ -n "$profile" ] && [ -n "$output" ] || { echo "usage: bench-gate.sh --tier smoke|release --profile PROFILE --output PATH" >&2; exit 2; }
case "$(basename "$output")" in baseline.*.json) echo "refusing to overwrite baseline" >&2; exit 1;; esac
mkdir -p "$(dirname "$output")"
runs=5; corpus=200; seed="0xA1750f7"; cargo_flags=()
if [ "$tier" = "release" ]; then runs=9; corpus=10000; seed="0xA175e1ea5e"; cargo_flags=(--release); fi
runs="${BENCH_RUNS_OVERRIDE:-$runs}"
corpus="${BENCH_CORPUS_OVERRIDE:-$corpus}"
cargo run -q "${cargo_flags[@]}" -p memory-substrate --features bench-harness --bin stream_a_bench -- \
  --tier "$tier" \
  --profile "$profile" \
  --output "$output" \
  --runs "$runs" \
  --corpus "$corpus" \
  --seed "$seed"
echo "bench gate $tier wrote $output"
