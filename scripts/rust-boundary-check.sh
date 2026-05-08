#!/usr/bin/env bash
set -euo pipefail

# Production seam: memoryd-web runtime code must use memoryd protocol DTO
# re-exports, not depend on memory-substrate directly. Integration tests under
# tests/ are allowed to seed real substrate state via [dev-dependencies].
if grep -R -n -E 'memory(_|-)?substrate' crates/memoryd-web/src 2>/dev/null; then
  echo "memoryd-web/src must use memoryd protocol DTO re-exports instead of depending on memory-substrate directly" >&2
  exit 1
fi

# Cargo.toml: substrate is allowed as a [dev-dependencies] entry but not as a
# production [dependencies] entry. Extract only the [dependencies] section.
if awk '/^\[dependencies\]/{in_deps=1; next} /^\[/{in_deps=0} in_deps' crates/memoryd-web/Cargo.toml \
  | grep -E 'memory(_|-)?substrate' >/dev/null 2>&1; then
  echo "memoryd-web/Cargo.toml [dependencies] must not include memory-substrate (move to [dev-dependencies] for integration tests)" >&2
  exit 1
fi

cargo run -p memory-test-support --bin rust_boundary_check -- "$@"
