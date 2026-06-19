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
# ProgramArguments[0] must be an ABSOLUTE path. launchd resolves the executable
# itself and ignores the plist's EnvironmentVariables PATH, so a bare "memoryd"
# fails to spawn (EX_CONFIG / exit 78). Regression guard.
assert_contains "/memoryd</string>"
if grep -Fq -- "<string>memoryd</string>" "$out"; then
  echo "plist uses a bare 'memoryd' program name; launchd cannot resolve it" >&2
  cat "$out" >&2
  exit 1
fi
assert_contains "<string>--repo</string>"
assert_contains "<string>$repo</string>"
assert_contains "<string>$runtime/memoryd.sock</string>"
assert_contains "<key>KeepAlive</key>"
assert_contains "<string>$runtime/daemon.out.log</string>"
assert_contains "<string>$runtime/daemon.err.log</string>"
assert_contains ".cargo/bin"
# launchd does not inherit the shell PATH, so the daemon PATH must include the
# user-local bin dir where `claude` typically resolves. Regression guard.
assert_contains "/.local/bin"
# launchd user agents lack USER, which Claude's macOS keychain auth needs.
assert_contains "<key>USER</key>"
assert_contains "<string>$runtime/dream-scheduled.out.log</string>"

# All templated placeholders must be rendered away.
for placeholder in "{{PATH}}" "{{CLAUDE_CONFIG_DIR_ENTRY}}" "{{MEMORYD_BIN}}" "{{HOME}}"; do
  if grep -Fq -- "$placeholder" "$out"; then
    echo "unrendered placeholder remained: $placeholder" >&2
    cat "$out" >&2
    exit 1
  fi
done

# Without --claude-config-dir, no CLAUDE_CONFIG_DIR is injected (the daemon
# auto-detects). The installer never reads an ambient CLAUDE_CONFIG_DIR.
if grep -Fq -- "CLAUDE_CONFIG_DIR" "$out"; then
  echo "CLAUDE_CONFIG_DIR injected without --claude-config-dir flag" >&2
  cat "$out" >&2
  exit 1
fi

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

# --claude-config-dir pins CLAUDE_CONFIG_DIR into the EnvironmentVariables dict.
ccd_out="$tmp/ccd.out"
ccd_profile="$physical_tmp/profile-personal"
bash "$repo_root/scripts/install-launchd.sh" \
  --repo "$repo" \
  --runtime "$runtime" \
  --claude-config-dir "$ccd_profile" \
  --dry-run >"$ccd_out"
assert_file_contains "$ccd_out" "<key>CLAUDE_CONFIG_DIR</key>" "claude config dir key"
assert_file_contains "$ccd_out" "<string>$ccd_profile</string>" "claude config dir value"
# Both agents (daemon + scheduled dream) must carry the pin.
ccd_key_count="$(grep -c -- "<key>CLAUDE_CONFIG_DIR</key>" "$ccd_out" || true)"
if [ "$ccd_key_count" -ne 2 ]; then
  echo "expected CLAUDE_CONFIG_DIR in both agents, found $ccd_key_count" >&2
  cat "$ccd_out" >&2
  exit 1
fi

expect_rejection "tilde-claude-config-dir" "literal ~ is not expanded here" \
  --repo "$repo" --runtime "$runtime" --claude-config-dir '~/.claude' --dry-run
expect_rejection "empty-claude-config-dir-value" "--claude-config-dir requires a non-empty value" \
  --repo "$repo" --runtime "$runtime" --claude-config-dir --dry-run

echo "install-launchd.test.sh: all checks passed"
