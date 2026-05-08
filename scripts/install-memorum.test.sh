#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

repo="$tmp/repo"
runtime="$tmp/runtime"
out="$tmp/install.out"

bash "$repo_root/scripts/install-memorum.sh" \
  --dry-run \
  --force-reinstall \
  --repo "$repo" \
  --runtime "$runtime" >"$out"

assert_contains() {
  local needle="$1"
  if ! grep -Fq -- "$needle" "$out"; then
    echo "missing expected output: $needle" >&2
    cat "$out" >&2
    exit 1
  fi
}

assert_not_contains() {
  local needle="$1"
  if grep -Fq -- "$needle" "$out"; then
    echo "unexpected output: $needle" >&2
    cat "$out" >&2
    exit 1
  fi
}

assert_contains "cargo install --path $repo_root/crates/memoryd --locked"
assert_contains "cargo install --path $repo_root/crates/memoryd-tui --locked"
assert_contains "cargo install --path $repo_root/crates/memoryd-web --locked"
assert_contains "cargo install --path $repo_root/crates/memory-merge-driver --locked"
assert_contains "memoryd serve --init --repo $repo --runtime $runtime --socket $runtime/memoryd.sock"
assert_contains "memoryd status --socket $runtime/memoryd.sock"
assert_contains "claude mcp add memorum memoryd -- mcp --socket \"$runtime/memoryd.sock\""
assert_contains '"args": ["mcp", "--socket", "'"$runtime"'/memoryd.sock"]'
assert_contains "command -v memoryd-merge-driver"
assert_not_contains "/tmp/memoryd.sock"

grep -Fq 'kill -KILL "$existing_pid"' "$repo_root/scripts/install-memorum.sh"
grep -Fq 'readiness_seconds=30' "$repo_root/scripts/install-memorum.sh"
grep -Fq 'PATH="$HOME/.cargo/bin:$PATH"' "$repo_root/scripts/install-memorum.sh"
