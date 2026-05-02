#!/usr/bin/env bash
set -euo pipefail

if grep -R -n -E 'memory(_|-)?substrate' crates/memoryd-web/Cargo.toml crates/memoryd-web/src crates/memoryd-web/tests 2>/dev/null; then
  echo "memoryd-web must use memoryd protocol DTO re-exports instead of depending on memory-substrate directly" >&2
  exit 1
fi

cargo run -p memory-test-support --bin rust_boundary_check -- "$@"
