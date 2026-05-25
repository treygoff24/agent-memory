#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
tmp="$(mktemp -d)"
physical_tmp="$(cd "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT

repo="$physical_tmp/repo"
runtime="$physical_tmp/runtime"
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

# --- Relative path canonicalization ---
rel_tmp="$(mktemp -d)"
rel_physical="$(cd "$rel_tmp" && pwd -P)"
rel_out="$rel_tmp/install.out"
(
  cd "$rel_tmp"
  bash "$repo_root/scripts/install-memorum.sh" \
    --dry-run \
    --force-reinstall \
    --repo repo \
    --runtime runtime >"$rel_out"
)
rel_repo="$rel_physical/repo"
rel_runtime="$rel_physical/runtime"
rel_assert() {
  local needle="$1"
  local label="$2"
  if ! grep -Fq -- "$needle" "$rel_out"; then
    echo "relative-path test ($label): missing expected output: $needle" >&2
    cat "$rel_out" >&2
    exit 1
  fi
}
rel_assert "memoryd serve --init --repo $rel_repo --runtime $rel_runtime --socket $rel_runtime/memoryd.sock" "serve"
rel_assert "memoryd status --socket $rel_runtime/memoryd.sock" "status"
rel_assert "claude mcp add memorum memoryd -- mcp --socket \"$rel_runtime/memoryd.sock\"" "claude"
rel_assert '"args": ["mcp", "--socket", "'"$rel_runtime"'/memoryd.sock"]' "json"
rel_not_contains() {
  local needle="$1"
  local label="$2"
  if grep -Fq -- "$needle" "$rel_out"; then
    echo "relative-path test ($label): unexpected output: $needle" >&2
    cat "$rel_out" >&2
    exit 1
  fi
}
rel_not_contains "--repo repo" "relative-repo"
rel_not_contains "/tmp/memoryd.sock" "tmp-socket"
rm -rf "$rel_tmp"

# --- Literal tilde rejection ---
tilde_tmp="$(mktemp -d)"
tilde_stdout="$tilde_tmp/stdout"
tilde_stderr="$tilde_tmp/stderr"
tilde_code=0
bash "$repo_root/scripts/install-memorum.sh" \
  --dry-run \
  --force-reinstall \
  --repo '~/memorum' \
  >"$tilde_stdout" 2>"$tilde_stderr" || tilde_code=$?
if [ "$tilde_code" -ne 2 ]; then
  echo "tilde test: expected exit code 2, got $tilde_code" >&2
  cat "$tilde_stderr" >&2
  exit 1
fi
if ! grep -Fq 'literal ~ is not expanded here' "$tilde_stderr"; then
  echo "tilde test: stderr missing literal-tilde error message" >&2
  cat "$tilde_stderr" >&2
  exit 1
fi
if grep -Fq -- 'MCP client snippet' "$tilde_stdout"; then
  echo "tilde test: stdout should not contain MCP snippet after tilde rejection" >&2
  cat "$tilde_stdout" >&2
  exit 1
fi
if grep -Fq -- '~/' "$tilde_stdout"; then
  echo "tilde test: stdout contains literal ~ in a path" >&2
  cat "$tilde_stdout" >&2
  exit 1
fi
rm -rf "$tilde_tmp"
