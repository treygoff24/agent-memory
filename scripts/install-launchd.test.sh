#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

repo="$tmp/repo"
runtime="$tmp/runtime"
mkdir -p "$repo" "$runtime"
out="$tmp/launchd.out"

bash "$repo_root/scripts/install-launchd.sh" --repo "$repo" --runtime "$runtime" --dry-run >"$out"

assert_contains() {
  local needle="$1"
  if ! grep -Fq -- "$needle" "$out"; then
    echo "missing expected launchd output: $needle" >&2
    cat "$out" >&2
    exit 1
  fi
}

assert_contains "<string>com.memorum.daemon</string>"
assert_contains "<string>com.memorum.dream-scheduled</string>"
assert_contains "<string>serve</string>"
assert_contains "<string>--repo</string>"
assert_contains "<string>$repo</string>"
assert_contains "<string>$runtime/memoryd.sock</string>"
assert_contains "<key>KeepAlive</key>"
assert_contains "<string>$runtime/daemon.out.log</string>"
assert_contains "<string>$runtime/daemon.err.log</string>"
assert_contains ".cargo/bin"
assert_contains "<string>$runtime/dream-scheduled.out.log</string>"

daemon_only="$tmp/daemon-only.out"
bash "$repo_root/scripts/install-launchd.sh" --repo "$repo" --runtime "$runtime" --daemon --dry-run >"$daemon_only"
grep -Fq "com.memorum.daemon" "$daemon_only"
if grep -Fq "com.memorum.dream-scheduled" "$daemon_only"; then
  echo "daemon-only dry run included dream scheduler" >&2
  cat "$daemon_only" >&2
  exit 1
fi
