#!/usr/bin/env bash
set -euo pipefail

# check-doc-cli-surface.sh — verify every --flag token in docs/agent-onboarding.md
# is present in the real `memoryd <subcommand> --help` output for the subcommand
# it belongs to.
#
# Rationale: docs-command-validity.sh is grep-only and cannot catch an invented
# flag. This script builds and runs the real binary so invented flags fail hard.
# Deterministic and offline — no network required.
#
# Portability: written for Bash 3.2 (the frozen system bash on macOS). Avoids
# Bash 4 features (`mapfile`, associative arrays) so it runs under `/bin/bash`,
# not just a Homebrew bash that happens to be first on PATH.

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

# Bash 3.2 has no associative arrays, so hold each subcommand's flag set in a
# plain variable named SUBFLAGS_<subcmd> and look it up indirectly.
echo "Fetching real flag sets..." >&2
SUBCOMMANDS="init doctor status serve import"
for subcmd in $SUBCOMMANDS; do
  # `get_flags` emits one flag per line; collapse to a single-space-separated
  # set so the native `flag_in_set` glob match is an exact token test.
  flags="$(get_flags "$subcmd" | tr '\n' ' ')"
  eval "SUBFLAGS_${subcmd}=\$flags"
  echo "  memoryd $subcmd: $flags" >&2
done

# Echo the recorded flag set for a subcommand (empty if none recorded).
subcommand_flags() {
  eval "printf '%s' \"\${SUBFLAGS_$1:-}\""
}

# Native (no-subprocess) whitespace-set membership test. `flagset` is stored
# space-separated (see normalization at fetch time), so a space-padded glob
# match is an exact token test. `$flag` carries no glob metacharacters. This
# replaces a per-flag python3 spawn.
flag_in_set() {
  local flag="$1"
  local flagset="$2"
  case " $flagset " in
    *" $flag "*) return 0 ;;
    *) return 1 ;;
  esac
}

# Extract all --flag tokens from a line using python3 (avoids passing --flag
# tokens to grep/fgrep which may interpret them as options). The regex
# tokenization is the justified python3 use; set membership is native (above).
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

# ---------------------------------------------------------------------------
# Parse the guide: scope each flag to the subcommand of its own command block,
# extract flags, check. Read line-by-line (no `mapfile`) so this runs under
# Bash 3.2.
#
# Scope discipline: `current_subcmd` resets at every code-fence boundary so a
# flag inside a fenced example is only ever attributed to a `memoryd <subcmd>`
# *within that same fence* — never carried across an unrelated earlier block,
# which was the mis-attribution risk. Outside fences it also resets at Markdown
# section headers.
#
# Unattributed flags (prose mentions with no `memoryd <subcmd>` in their block,
# e.g. a flag-reference table or a "map their answer to --flag" instruction) are
# *skipped*, not errored: this gate validates flags shown in real command
# examples and deliberately does not police flag tokens in narrative prose,
# where there is no command to scope them to.
# ---------------------------------------------------------------------------

failed=0
current_subcmd=""
in_fence=0
checked_pairs=""

while IFS= read -r line; do
  # Code-fence boundary: entering or leaving a fenced block resets scope so
  # flags in a new block are not attributed to the previous block's subcommand.
  case "$line" in
    '```'*)
      if [[ $in_fence -eq 0 ]]; then in_fence=1; else in_fence=0; fi
      current_subcmd=""
      continue
      ;;
  esac

  # Markdown header (outside fences): a new section starts a fresh scope. Gated
  # on `in_fence` so a `# ...` shell comment inside a fenced example does not
  # reset the in-block subcommand scope.
  if [[ $in_fence -eq 0 ]]; then
    case "$line" in
      '#'*) current_subcmd="" ;;
    esac
  fi

  # Detect subcommand attribution on this line: "memoryd init", etc.
  if [[ "$line" =~ memoryd[[:space:]]+(init|doctor|status|serve|import) ]]; then
    current_subcmd="${BASH_REMATCH[1]}"
  fi

  # Skip flags that have no command in scope (narrative prose / tables).
  [[ -z "$current_subcmd" ]] && continue

  flags_on_line="$(extract_flags_from_line "$line")"
  [[ -z "$flags_on_line" ]] && continue

  while IFS= read -r flag; do
    [[ -z "$flag" ]] && continue

    pair="${current_subcmd}:${flag}"
    case " $checked_pairs " in
      *" $pair "*) continue ;;
    esac
    checked_pairs="$checked_pairs $pair"

    subflags="$(subcommand_flags "$current_subcmd")"
    if flag_in_set "$flag" "$subflags"; then
      echo "  OK: $flag is valid for memoryd $current_subcmd" >&2
    else
      echo "ERROR: flag '$flag' appears near 'memoryd $current_subcmd' in $GUIDE but is NOT in 'memoryd $current_subcmd --help'" >&2
      echo "  Real flags for 'memoryd $current_subcmd': $subflags" >&2
      failed=1
    fi
  done <<< "$flags_on_line"
done < "$GUIDE"

if [[ $failed -ne 0 ]]; then
  echo "" >&2
  echo "FAIL: docs/agent-onboarding.md references flags not present in the real CLI surface." >&2
  echo "      Fix the guide to use only flags shown in 'memoryd <subcmd> --help'." >&2
  exit 1
fi

echo "" >&2
echo "PASS: all --flag tokens in $GUIDE match the real memoryd CLI surface." >&2
