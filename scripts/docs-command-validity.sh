#!/usr/bin/env bash
set -euo pipefail

paths=(README.md docs/runbooks docs/api docs/dev)
failed=0
if rg -n 'cargo run -p memoryd --' "${paths[@]}" 2>/dev/null; then
  echo "docs contain stale cargo command; use cargo run --bin memoryd --" >&2
  failed=1
fi
if rg -n '/tmp/memoryd\.sock' "${paths[@]}" 2>/dev/null; then
  echo "docs contain stale shared socket path; use <runtime>/memoryd.sock or command defaults" >&2
  failed=1
fi
exit "$failed"
