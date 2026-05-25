#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd -P)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

expect_rejection() {
  local label="$1"
  local expected="$2"
  shift 2
  local stdout="$tmp/$label.stdout"
  local stderr="$tmp/$label.stderr"
  local code=0
  bash "$repo_root/scripts/seed-dev-substrate.sh" "$@" >"$stdout" 2>"$stderr" || code=$?
  if [ "$code" -ne 2 ]; then
    echo "$label: expected exit code 2, got $code" >&2
    cat "$stderr" >&2
    exit 1
  fi
  if ! grep -Fq -- "$expected" "$stderr"; then
    echo "$label: missing expected stderr: $expected" >&2
    cat "$stderr" >&2
    exit 1
  fi
}

expect_rejection "missing-repo" "--repo requires a non-empty value" --repo --runtime "$tmp/runtime"
expect_rejection "empty-runtime" "--runtime requires a non-empty value" --runtime ""
expect_rejection "tilde-repo" "literal ~ is not expanded here" --repo '~/memorum-dev'
expect_rejection "same-repo-runtime" "--repo and --runtime must be different paths" --repo "$tmp/same" --runtime "$tmp/same"

expect_rejection "reset-root-repo" "refusing to reset unsafe repo path" --reset --repo / --runtime "$tmp/runtime"
expect_rejection "reset-root-runtime" "refusing to reset unsafe runtime path" --reset --repo "$HOME/memorum-dev" --runtime /
expect_rejection "reset-home-repo" "refusing to reset repo path equal to \$HOME" --reset --repo "$HOME" --runtime "$tmp/runtime"
expect_rejection "reset-repo-root" "refusing to reset repo path that contains this repository" --reset --repo "$repo_root" --runtime "$tmp/runtime"
expect_rejection "reset-unmarked-custom-repo" "refusing to reset unmarked custom repo path" --reset --repo "$tmp/custom-repo" --runtime "$tmp/runtime"
expect_rejection "reset-unmarked-custom-runtime" "refusing to reset unmarked custom runtime path" --reset --repo "$HOME/memorum-dev" --runtime "$tmp/custom-runtime"

echo "seed-dev-substrate.test.sh: all checks passed"
