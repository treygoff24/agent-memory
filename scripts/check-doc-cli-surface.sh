#!/usr/bin/env bash
set -euo pipefail

# check-doc-cli-surface.sh — verify every --flag token in docs/agent-onboarding.md
# is present in the real `memoryd <subcommand> --help` output for the subcommand
# it belongs to.
#
# Rationale: docs-command-validity.sh is grep-only and cannot catch an invented
# flag. This script builds and runs the real binary so invented flags fail hard.
# Deterministic and offline — no network required.

WORKTREE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GUIDE="$WORKTREE_ROOT/docs/agent-onboarding.md"
CARGO_BIN="memoryd"
CARGO_PACKAGE="memoryd"

if [[ ! -f "$GUIDE" ]]; then
  echo "ERROR: guide not found at $GUIDE" >&2
  exit 1
fi

# Build the binary once so subsequent --help calls are fast.
echo "Building $CARGO_BIN..." >&2
cargo build -q -p "$CARGO_PACKAGE" --bin "$CARGO_BIN" 2>&1 >&2

# Resolve the built binary path from cargo metadata.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps 2>/dev/null | \
  python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["target_directory"])')"
MEMORYD_BIN="$TARGET_DIR/debug/$CARGO_BIN"

if [[ ! -x "$MEMORYD_BIN" ]]; then
  echo "ERROR: built binary not found at $MEMORYD_BIN" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Fetch the real flag set for each subcommand the guide references.
# ---------------------------------------------------------------------------

get_flags() {
  local subcmd="$1"
  # Use python3 to extract --flag tokens so we avoid grep receiving a flag
  # token as its own argument (ugrep / macOS grep treat "--flag" as an option).
  "$MEMORYD_BIN" "$subcmd" --help 2>/dev/null \
    | python3 -c '
import sys, re
flags = set()
for line in sys.stdin:
    for m in re.finditer(r"--([a-z][a-z0-9_-]+)", line):
        flags.add("--" + m.group(1))
for f in sorted(flags):
    print(f)
'
}

echo "Fetching real flag sets..." >&2
declare -A SUBCOMMAND_FLAGS
for subcmd in init doctor status serve import; do
  SUBCOMMAND_FLAGS["$subcmd"]="$(get_flags "$subcmd")"
  echo "  memoryd $subcmd: $(echo "${SUBCOMMAND_FLAGS[$subcmd]}" | tr '\n' ' ')" >&2
done

# ---------------------------------------------------------------------------
# Parse the guide: track subcommand scope per line, extract flags, check.
# ---------------------------------------------------------------------------

failed=0

mapfile -t guide_lines < "$GUIDE"

declare -a LINE_SUBCMD
current_subcmd=""
for i in "${!guide_lines[@]}"; do
  line="${guide_lines[$i]}"
  # Detect subcommand attribution: "memoryd init", "memoryd doctor", etc.
  if [[ "$line" =~ memoryd[[:space:]]+(init|doctor|status|serve|import) ]]; then
    current_subcmd="${BASH_REMATCH[1]}"
  fi
  LINE_SUBCMD[$i]="$current_subcmd"
done

# Extract all --flag tokens from a line using python3 (avoids passing --flag
# tokens to grep/fgrep which may interpret them as options).
extract_flags_from_line() {
  python3 -c '
import sys, re
line = sys.stdin.read()
flags = set()
for m in re.finditer(r"--([a-z][a-z0-9_-]+)", line):
    token = "--" + m.group(1)
    if token != "--help":
        flags.add(token)
for f in sorted(flags):
    print(f)
' <<< "$1"
}

flag_in_set() {
  local flag="$1"
  local flagset="$2"
  python3 -c '
import sys
flag = sys.argv[1]
flagset = sys.argv[2]
print("yes" if flag in flagset.split() else "no")
' "$flag" "$flagset"
}

declare -A checked_pairs

lineno=0
for line in "${guide_lines[@]}"; do
  subcmd_at_line="${LINE_SUBCMD[$lineno]:-}"
  lineno=$((lineno + 1))

  [[ -z "$subcmd_at_line" ]] && continue

  flags_on_line="$(extract_flags_from_line "$line")"
  [[ -z "$flags_on_line" ]] && continue

  while IFS= read -r flag; do
    [[ -z "$flag" ]] && continue
    pair="${subcmd_at_line}:${flag}"
    [[ -n "${checked_pairs[$pair]+set}" ]] && continue
    checked_pairs["$pair"]=1

    subflags="${SUBCOMMAND_FLAGS[$subcmd_at_line]:-}"
    result="$(flag_in_set "$flag" "$subflags")"
    if [[ "$result" == "yes" ]]; then
      echo "  OK: $flag is valid for memoryd $subcmd_at_line" >&2
    else
      echo "ERROR: flag '$flag' appears near 'memoryd $subcmd_at_line' in $GUIDE but is NOT in 'memoryd $subcmd_at_line --help'" >&2
      echo "  Real flags for 'memoryd $subcmd_at_line':" >&2
      echo "$subflags" | sed 's/^/    /' >&2
      failed=1
    fi
  done <<< "$flags_on_line"
done

if [[ $failed -ne 0 ]]; then
  echo "" >&2
  echo "FAIL: docs/agent-onboarding.md references flags not present in the real CLI surface." >&2
  echo "      Fix the guide to use only flags shown in 'memoryd <subcmd> --help'." >&2
  exit 1
fi

echo "" >&2
echo "PASS: all --flag tokens in $GUIDE match the real memoryd CLI surface." >&2
