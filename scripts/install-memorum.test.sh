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

# --- Relative path canonicalization (repo/runtime defaults) ---
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

# --- Explicit relative --socket canonicalization ---
socket_tmp="$(mktemp -d)"
socket_physical="$(cd "$socket_tmp" && pwd -P)"
socket_out="$socket_tmp/install.out"
(
  cd "$socket_tmp"
  bash "$repo_root/scripts/install-memorum.sh" \
    --dry-run \
    --force-reinstall \
    --repo "$socket_physical/repo" \
    --runtime "$socket_physical/runtime" \
    --socket custom.sock >"$socket_out"
)
socket_expected="$socket_physical/custom.sock"
if ! grep -Fq -- "memoryd serve --init --repo $socket_physical/repo --runtime $socket_physical/runtime --socket $socket_expected" "$socket_out"; then
  echo "explicit relative --socket test: missing canonicalized serve command" >&2
  cat "$socket_out" >&2
  exit 1
fi
if ! grep -Fq -- "memoryd status --socket $socket_expected" "$socket_out"; then
  echo "explicit relative --socket test: missing canonicalized status command" >&2
  cat "$socket_out" >&2
  exit 1
fi
rm -rf "$socket_tmp"

# --- Empty argument rejection ---
expect_empty_rejection() {
  local flag="$1"
  local label="$2"
  local case_tmp="$tmp/empty-$label"
  mkdir -p "$case_tmp"
  local case_stdout="$case_tmp/stdout"
  local case_stderr="$case_tmp/stderr"
  local case_code=0
  bash "$repo_root/scripts/install-memorum.sh" \
    --dry-run \
    --force-reinstall \
    "$flag" "" \
    >"$case_stdout" 2>"$case_stderr" || case_code=$?
  if [ "$case_code" -ne 2 ]; then
    echo "empty $label test: expected exit code 2, got $case_code" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if ! grep -Fq 'requires a non-empty value' "$case_stderr"; then
    echo "empty $label test: stderr missing non-empty error message" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if grep -Fq -- 'MCP client snippet' "$case_stdout"; then
    echo "empty $label test: stdout should not contain MCP snippet after rejection" >&2
    cat "$case_stdout" >&2
    exit 1
  fi
}

expect_empty_rejection "--repo" "repo"
expect_empty_rejection "--runtime" "runtime"
expect_empty_rejection "--socket" "socket"

expect_missing_rejection_before_next_flag() {
  local flag="$1"
  local next_flag="$2"
  local label="$3"
  local case_tmp="$tmp/missing-$label"
  mkdir -p "$case_tmp"
  local case_stdout="$case_tmp/stdout"
  local case_stderr="$case_tmp/stderr"
  local case_code=0
  bash "$repo_root/scripts/install-memorum.sh" \
    --dry-run \
    --force-reinstall \
    "$flag" "$next_flag" \
    >"$case_stdout" 2>"$case_stderr" || case_code=$?
  if [ "$case_code" -ne 2 ]; then
    echo "missing $label test: expected exit code 2, got $case_code" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if ! grep -Fq 'requires a non-empty value' "$case_stderr"; then
    echo "missing $label test: stderr missing non-empty error message" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if grep -Fq -- 'MCP client snippet' "$case_stdout"; then
    echo "missing $label test: stdout should not contain MCP snippet after rejection" >&2
    cat "$case_stdout" >&2
    exit 1
  fi
}

expect_missing_rejection_before_next_flag "--repo" "--runtime" "repo"
expect_missing_rejection_before_next_flag "--runtime" "--socket" "runtime"
expect_missing_rejection_before_next_flag "--socket" "--with-scheduler" "socket"

# --- Leading literal tilde rejection ---
expect_tilde_rejection() {
  local flag="$1"
  local value="$2"
  local label="$3"
  local case_tmp="$tmp/tilde-$label"
  mkdir -p "$case_tmp"
  local case_stdout="$case_tmp/stdout"
  local case_stderr="$case_tmp/stderr"
  local case_code=0
  bash "$repo_root/scripts/install-memorum.sh" \
    --dry-run \
    --force-reinstall \
    "$flag" "$value" \
    >"$case_stdout" 2>"$case_stderr" || case_code=$?
  if [ "$case_code" -ne 2 ]; then
    echo "tilde $label test: expected exit code 2, got $case_code" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if ! grep -Fq 'literal ~ is not expanded here' "$case_stderr"; then
    echo "tilde $label test: stderr missing literal-tilde error message" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if grep -Fq -- 'MCP client snippet' "$case_stdout"; then
    echo "tilde $label test: stdout should not contain MCP snippet after tilde rejection" >&2
    cat "$case_stdout" >&2
    exit 1
  fi
}

expect_tilde_rejection "--repo" '~/memorum' "repo"
expect_tilde_rejection "--runtime" '~/memorum/.memoryd' "runtime"
expect_tilde_rejection "--socket" '~/memorum/.memoryd/memoryd.sock' "socket"

# Paths containing ~ away from the start should not be rejected.
mid_tilde_tmp="$(mktemp -d)"
mid_tilde_out="$mid_tilde_tmp/install.out"
bash "$repo_root/scripts/install-memorum.sh" \
  --dry-run \
  --force-reinstall \
  --repo "$mid_tilde_tmp/repo~backup" \
  --runtime "$mid_tilde_tmp/runtime" >"$mid_tilde_out"
if ! grep -Fq -- 'MCP client snippet' "$mid_tilde_out"; then
  echo "mid-tilde test: expected installer to accept path with ~ away from start" >&2
  cat "$mid_tilde_out" >&2
  exit 1
fi
rm -rf "$mid_tilde_tmp"

# --- Onboarding docs use platform-neutral placeholders ---
grep -Fq '/absolute/path/to/memorum/.memoryd/memoryd.sock' "$repo_root/docs/mcp-wiring.md"
grep -Fq '[mcp_servers.memorum]' "$repo_root/docs/mcp-wiring.md"
grep -Fq '/absolute/path/to/memorum/.memoryd/memoryd.sock' "$repo_root/README.md"
grep -Fq '/absolute/path/to/memorum/.memoryd/memoryd.sock' "$repo_root/docs/getting-started.md"
if rg -q '/Users/you/' "$repo_root/README.md" "$repo_root/docs/getting-started.md" "$repo_root/docs/mcp-wiring.md"; then
  echo "onboarding docs still contain macOS-specific /Users/you/ placeholder" >&2
  exit 1
fi
if rg -q '^\[mcp\.' "$repo_root/docs/mcp-wiring.md"; then
  echo "docs/mcp-wiring.md still contains stale [mcp.*] Codex header" >&2
  exit 1
fi

# --- docs-command-validity.sh passes on current docs ---
bash "$repo_root/scripts/docs-command-validity.sh"

echo "install-memorum.test.sh: all checks passed"
