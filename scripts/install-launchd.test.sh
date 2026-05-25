#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
tmp="$(mktemp -d)"
physical_tmp="$(cd "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT

repo="$physical_tmp/repo"
runtime="$physical_tmp/runtime"
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

assert_file_contains() {
  local file="$1"
  local needle="$2"
  local label="${3:-$needle}"
  if ! grep -Fq -- "$needle" "$file"; then
    echo "missing expected launchd output ($label): $needle" >&2
    cat "$file" >&2
    exit 1
  fi
}

xml_escape_expected() {
  awk '
    BEGIN { ORS = "" }
    {
      if (NR > 1) {
        printf "\n"
      }
      gsub(/&/, "\\&amp;")
      gsub(/</, "\\&lt;")
      gsub(/>/, "\\&gt;")
      gsub(/"/, "\\&quot;")
      gsub(/\047/, "\\&apos;")
      printf "%s", $0
    }
  ' <<<"$1"
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

dream_only="$tmp/dream-only.out"
bash "$repo_root/scripts/install-launchd.sh" --repo "$repo" --runtime "$runtime" --dream-scheduler --dry-run >"$dream_only"
assert_file_contains "$dream_only" "com.memorum.dream-scheduled" "dream-only label"
if grep -Fq "com.memorum.daemon" "$dream_only"; then
  echo "dream-only dry run included daemon" >&2
  cat "$dream_only" >&2
  exit 1
fi

special_repo="$physical_tmp/repo space & # <tag> \"quoted\" 'apos'"
special_runtime="$physical_tmp/runtime space & # <tag> \"quoted\" 'apos'"
mkdir -p "$special_repo"
special_out="$tmp/special.out"
bash "$repo_root/scripts/install-launchd.sh" \
  --repo "$special_repo" \
  --runtime "$special_runtime" \
  --dry-run >"$special_out"
escaped_repo="$(xml_escape_expected "$special_repo")"
escaped_runtime="$(xml_escape_expected "$special_runtime")"
assert_file_contains "$special_out" "<string>$escaped_repo</string>" "escaped repo path"
assert_file_contains "$special_out" "<string>$escaped_runtime</string>" "escaped runtime path"
assert_file_contains "$special_out" "<string>$escaped_runtime/memoryd.sock</string>" "escaped socket path"
if [ -e "$special_runtime" ]; then
  echo "launchd dry run should not create the runtime directory" >&2
  exit 1
fi

expect_rejection() {
  local label="$1"
  local expected="$2"
  shift 2
  local case_stdout="$tmp/$label.stdout"
  local case_stderr="$tmp/$label.stderr"
  local case_code=0
  bash "$repo_root/scripts/install-launchd.sh" "$@" >"$case_stdout" 2>"$case_stderr" || case_code=$?
  if [ "$case_code" -ne 2 ]; then
    echo "$label: expected exit code 2, got $case_code" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
  if ! grep -Fq -- "$expected" "$case_stderr"; then
    echo "$label: missing expected stderr: $expected" >&2
    cat "$case_stderr" >&2
    exit 1
  fi
}

expect_rejection "missing-repo-value" "--repo requires a non-empty value" --repo --runtime "$runtime" --dry-run
expect_rejection "empty-runtime-value" "--runtime requires a non-empty value" --repo "$repo" --runtime "" --dry-run
expect_rejection "tilde-repo" "literal ~ is not expanded here" --repo '~/memorum' --runtime "$runtime" --dry-run
expect_rejection "unknown-flag" "unknown argument: --wat" --repo "$repo" --runtime "$runtime" --wat --dry-run

echo "install-launchd.test.sh: all checks passed"
