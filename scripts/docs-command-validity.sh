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

wright_review_handoff="docs/handoffs/2026-05-18-wright-loop-gap-analysis-from-export-v0.1-review.md"
if [[ -f "$wright_review_handoff" ]]; then
  if rg -n 'landed in `main`' "$wright_review_handoff"; then
    echo "wright review handoff must not claim unmerged trial commits landed in main" >&2
    failed=1
  fi
  if ! rg -n 'landed on the `feature/tiered-gate-dashboard-workflow` feature branch' "$wright_review_handoff" >/dev/null; then
    echo "wright review handoff must name the feature branch as the reviewed landing surface" >&2
    failed=1
  fi
  if rg -n 'ReviewBlocked.*back to Claimed|ReviewBlocked.*returns to `Approved`|review found blockers.*returns to `Approved`' "$wright_review_handoff"; then
    echo "wright review handoff must use one ReviewBlocked recovery model without contradictory state transitions" >&2
    failed=1
  fi
  if ! rg -n 'ReviewBlocked.*open and re-claimable' "$wright_review_handoff" >/dev/null; then
    echo "wright review handoff must document ReviewBlocked as open and re-claimable" >&2
    failed=1
  fi
  if rg -n '`cargo fmt --check`.*file-scoped' "$wright_review_handoff"; then
    echo "wright review handoff must not describe impossible file-scoped cargo fmt checking" >&2
    failed=1
  fi
  if ! rg -n 'rustfmt --check <touched-rust-files>|cargo fmt --all -- --check' "$wright_review_handoff" >/dev/null; then
    echo "wright review handoff must specify an actionable rust formatting gate" >&2
    failed=1
  fi
fi
exit "$failed"
