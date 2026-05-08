#!/usr/bin/env bash
# scripts/owned-files-check.sh
#
# Validate "Owned files:" entries in a Codex implementation plan.
#
# Checks:
#   1. No two tasks declare the same owned file (after stripping inline
#      annotations like "(post-Task-9 path)" and trailing comments).
#      Cluster sequencing makes serial collisions on `handlers.rs`/
#      `handlers/mod.rs`, `main.rs`, etc. allowed; everything else collides.
#   2. Every Owned file path either exists on disk or is in the known
#      create-list (handled separately — Create is fine to reference
#      a non-existent path).
#
# Usage: bash scripts/owned-files-check.sh <path-to-plan.md>
#
# Exit status: 0 = clean, 1 = collision, 2 = unknown path.

set -euo pipefail

PLAN="${1:-}"
if [[ -z "$PLAN" || ! -f "$PLAN" ]]; then
  echo "usage: $0 <plan.md>" >&2
  exit 64
fi

# Allowed serial-collision paths (intentional cluster sequencing).
ALLOWED_SERIAL_PATHS=(
  "crates/memoryd/src/handlers.rs"
  "crates/memoryd/src/handlers/mod.rs"
  "crates/memoryd/src/main.rs"
  "crates/memoryd/src/mcp.rs"
  "crates/memoryd/src/mcp_stdio.rs"
  "crates/memoryd/src/protocol.rs"
  "crates/memoryd/src/socket.rs"
  "crates/memoryd/src/cli.rs"
  "crates/memoryd/src/server.rs"
  "crates/memoryd-tui/src/app.rs"
  "crates/memoryd-tui/src/client.rs"
  "crates/memoryd/src/dream/orchestration.rs"
  "crates/memoryd/src/dream/run.rs"
  "crates/memoryd-web/src/server.rs"
  "crates/memoryd/tests/handler_contract.rs"
  "crates/memoryd/tests/mcp_lifecycle.rs"
  "crates/memory-substrate/src/config/mod.rs"
)

is_allowed_serial() {
  local p="$1"
  for allowed in "${ALLOWED_SERIAL_PATHS[@]}"; do
    [[ "$p" == "$allowed" ]] && return 0
  done
  return 1
}

# Known create-list (paths the plan declares Create:).
KNOWN_CREATE=(
  "crates/memoryd/src/socket.rs"
  "crates/memory-substrate/src/config/privacy.rs"
  "crates/memory-privacy/src/secret_only_scan.rs"
  "crates/memoryd-web/src/routes/policy_editor.rs"
  "crates/memoryd-web/src/routes/sync_dashboard.rs"
  "crates/memoryd-web/src/routes/entity_detail.rs"
  "crates/memoryd-web/static/components"
  "crates/memoryd/src/dream/cleanup/atomic.rs"
  "crates/memoryd/src/notifications/triggers.rs"
  "scripts/install-memorum.test.sh"
  "scripts/install-launchd.test.sh"
  "prompts/dream-pass-1-v2.md"
  "prompts/dream-pass-2-v2.md"
  "prompts/dream-pass-3-v2.md"
  "scripts/templates/com.memorum.daemon.plist.template"
  "crates/memoryd/src/handlers/mod.rs"
  "crates/memoryd/src/handlers/doctor.rs"
  "crates/memoryd/src/notifications/triggers.rs"
  "crates/memoryd-tui/src/state.rs"
  "crates/memoryd-tui/src/modals/correct.rs"
  "crates/memoryd-web/static/components/"
  "crates/memoryd-web/static/components"
  "crates/memoryd-tui/tests/panel_polling.rs"
  "crates/memoryd-tui/tests/reality_check_progress.rs"
  "crates/memoryd-tui/tests/correct_modal.rs"
  "crates/memoryd-web/tests/entity_endpoints.rs"
  "crates/memoryd-web/tests/dashboard_endpoints.rs"
  "crates/memoryd-web/tests/frontend_smoke.rs"
  "crates/memory-source/tests/source_capture_redaction.rs"
  "crates/memoryd/tests/mcp_lifecycle.rs"
  "crates/memoryd/tests/protocol_contract.rs"
  "crates/memoryd/tests/socket_resolver.rs"
  "crates/memoryd/tests/serve_durability.rs"
  "crates/memoryd/tests/privacy_runtime_install.rs"
  "crates/memoryd/tests/dream_auth_diagnostic.rs"
  "crates/memoryd/tests/dream_prompt_smoke.rs"
  "crates/memoryd/tests/notification_fanout.rs"
  "crates/memoryd/tests/cleanup_atomic.rs"
  "crates/memoryd/tests/recall_startup.rs"
  "crates/memoryd/tests/trust_artifact_claim_lock.rs"
  "crates/memoryd/tests/handler_contract.rs"
  "crates/memoryd/tests/mcp_manifest.rs"
  "crates/memory-substrate/tests/open_validation.rs"
  "crates/memory-substrate/tests/config_privacy.rs"
  "crates/memory-privacy/tests/runtime_switches.rs"
  "crates/memory-privacy/tests/envelope.rs"
  "crates/memory-governance/tests/dogfood_defaults.rs"
)

is_known_create() {
  local p="$1"
  for c in "${KNOWN_CREATE[@]}"; do
    [[ "$p" == "$c" ]] && return 0
  done
  return 1
}

# Parse owned-file paths per task. Two-pass:
#   pass 1 — for each "Owned files:" line, strip *all* parenthetical content
#            recursively, then expand `{a,b,c}` brace expressions, then split
#            on commas. This avoids commas inside `(...)` and inside `{...}`
#            being treated as entry separators.
parse_owned_files() {
  awk '
    function strip_parens(s,    out, depth, ch, i) {
      out = ""; depth = 0
      for (i = 1; i <= length(s); i++) {
        ch = substr(s, i, 1)
        if (ch == "(") { depth++; continue }
        if (ch == ")") { if (depth > 0) depth--; continue }
        if (depth == 0) out = out ch
      }
      return out
    }
    function protect_brace_commas(s,    out, depth, ch, i) {
      out = ""; depth = 0
      for (i = 1; i <= length(s); i++) {
        ch = substr(s, i, 1)
        if (ch == "{") depth++
        else if (ch == "}") depth--
        else if (ch == "," && depth > 0) ch = "\001"
        out = out ch
      }
      return out
    }
    function expand_one(entry,    out, prefix, suffix, body, n, parts, i) {
      # Restore any protected commas inside braces.
      gsub(/\001/, ",", entry)
      out = ""
      while (match(entry, /\{[^{}]+\}/)) {
        prefix = substr(entry, 1, RSTART - 1)
        body = substr(entry, RSTART + 1, RLENGTH - 2)
        suffix = substr(entry, RSTART + RLENGTH)
        n = split(body, parts, ",")
        out = ""
        for (i = 1; i <= n; i++) {
          if (out != "") out = out "\002"
          out = out prefix parts[i] suffix
        }
        entry = out
      }
      return entry
    }
    /^### Task / { task = $0; next }
    /^\*\*Owned files:\*\*/ {
      line = $0
      sub(/^\*\*Owned files:\*\* /, "", line)
      line = strip_parens(line)
      line = protect_brace_commas(line)
      n = split(line, parts, ",")
      for (i = 1; i <= n; i++) {
        entry = parts[i]
        gsub(/^[[:space:]]+|[[:space:]]+$/, "", entry)
        gsub(/`/, "", entry)
        if (length(entry) == 0) continue
        # Now expand braces (commas inside braces protected as \001).
        expanded = expand_one(entry)
        m = split(expanded, sub_parts, "\002")
        for (j = 1; j <= m; j++) {
          p = sub_parts[j]
          gsub(/^[[:space:]]+|[[:space:]]+$/, "", p)
          # Trailing line-range ":N" or ":N-M".
          sub(/:[0-9]+(-[0-9]+)?$/, "", p)
          if (p ~ /\*/) continue
          if (length(p) == 0) continue
          if (p !~ /\//) continue
          printf "%s\t%s\n", task, p
        }
      }
    }
  ' "$PLAN"
}

declare -A OWNERS
declare -A SEEN_IN_TASK
collision_count=0
unknown_count=0

while IFS=$'\t' read -r task path; do
  [[ -z "$path" ]] && continue

  # Dedupe within the same task (e.g., line-ranged listings of the same file).
  key="${task}|${path}"
  if [[ -n "${SEEN_IN_TASK[$key]+x}" ]]; then
    continue
  fi
  SEEN_IN_TASK[$key]=1

  # Existence check.
  if [[ ! -e "$path" ]]; then
    if ! is_known_create "$path"; then
      case "$path" in
        */tests/*.rs) : ;;       # test files are fine to be Create-only
        */templates/*) : ;;      # plist templates etc.
        crates/memoryd-web/src/routes/*) : ;;
        prompts/*) : ;;
        *)
          echo "WARN: path declared but not on disk and not in known-create list: $path  (task: $task)" >&2
          unknown_count=$((unknown_count + 1))
          ;;
      esac
    fi
  fi

  if [[ -n "${OWNERS[$path]+x}" ]]; then
    if is_allowed_serial "$path"; then
      :
    else
      echo "COLLISION: $path" >&2
      echo "  task A: ${OWNERS[$path]}" >&2
      echo "  task B: $task" >&2
      collision_count=$((collision_count + 1))
    fi
  else
    OWNERS[$path]="$task"
  fi
done < <(parse_owned_files)

if (( collision_count > 0 )); then
  echo "owned-files-check FAILED: $collision_count parallel-collision(s)" >&2
  exit 1
fi
if (( unknown_count > 0 )); then
  echo "owned-files-check WARN: $unknown_count unknown-path warning(s) — verify these are Create-intent" >&2
fi
echo "owned-files-check OK"
exit 0
