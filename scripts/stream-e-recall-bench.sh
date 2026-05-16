#!/usr/bin/env bash
set -euo pipefail

cargo_mode=()
mode=""
sizes="200,1000"
warm_runs="3"

set_mode() {
  local next="$1"
  if [ -n "$mode" ] && [ "$mode" != "$next" ]; then
    echo "--smoke and --release are mutually exclusive" >&2
    exit 2
  fi
  mode="$next"
}

for arg in "$@"; do
  case "$arg" in
    --smoke) set_mode "--smoke" ;;
    --release) set_mode "--release"; cargo_mode=(--release) ;;
    --quick) sizes="50"; warm_runs="1" ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

mode="${mode:---smoke}"
cargo run "${cargo_mode[@]}" -p memoryd --bin stream_e_recall_bench -- "$mode" --sizes "$sizes" --warm-runs "$warm_runs"
