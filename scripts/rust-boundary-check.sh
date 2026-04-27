#!/usr/bin/env bash
set -euo pipefail
cargo run -p memory-test-support --bin rust_boundary_check -- "$@"
