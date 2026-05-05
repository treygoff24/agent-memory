#!/usr/bin/env bash
set -euo pipefail

started_at="$(date +%s)"

finish() {
  local ended_at
  ended_at="$(date +%s)"
  echo "check-dogfood duration: $((ended_at - started_at))s"
}
trap finish EXIT

phase() {
  echo
  echo "==> $*"
}

phase "fast dogfood gate"
./scripts/check-fast.sh

phase "TUI recall panel"
cargo test -p memoryd-tui recall_panel -- --nocapture

phase "TUI panic restore"
cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1

phase "doctor health"
cargo test -p memoryd --lib doctor_health -- --nocapture

phase "doctor CLI unhealthy exit"
cargo test -p memoryd --test cli_contract doctor_unhealthy_exit -- --nocapture

phase "startup recall peer-update references"
cargo test -p memoryd --lib surfaced_peer_update_references -- --nocapture

phase "live-harness wrapper skip honesty without provider keys"
env -u MEMORUM_EVAL_CLAUDE_KEY -u MEMORUM_EVAL_CODEX_KEY \
  cargo test -p memorum-eval --features live-harness --test live -- --nocapture --test-threads=1

phase "memoryd minimal feature compile"
cargo check -p memoryd --no-default-features --locked

echo
echo "check-dogfood passed"
