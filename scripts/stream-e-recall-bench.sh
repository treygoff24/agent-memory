#!/usr/bin/env bash
set -euo pipefail

cargo_mode=()
mode="--smoke"
sizes="200,1000"
warm_runs="3"

for arg in "$@"; do
  case "$arg" in
    --smoke) mode="--smoke" ;;
    --release) mode="--release"; cargo_mode=(--release) ;;
    --quick) sizes="50"; warm_runs="1" ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

cargo run "${cargo_mode[@]}" -p memoryd --features dev-fixtures --bin stream_e_recall_bench -- "$mode" --sizes "$sizes" --warm-runs "$warm_runs"
