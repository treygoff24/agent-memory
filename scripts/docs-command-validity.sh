#!/usr/bin/env bash
set -euo pipefail

# docs-command-validity.sh — scan current onboarding/live-contract docs for stale
# commands, stale auth probes, and unexpanded tilde paths that MCP clients cannot
# interpret.  Fails with exit 1 and offending lines on stderr if any check fires.

paths=(
  README.md
  docs/getting-started.md
  docs/mcp-wiring.md
  docs/runbooks
  docs/api
  docs/dev
  docs/specs/system-v0.2.md
  docs/specs/stream-f-dreaming-v0.3.md
)

failed=0

# Stale cargo command: should use `cargo run --bin memoryd --` not `cargo run -p memoryd --`
if stale_cargo="$(rg -n 'cargo run -p memoryd --' "${paths[@]}" 2>/dev/null)"; then
  if [ -n "$stale_cargo" ]; then
    printf '%s\n' "$stale_cargo" >&2
    echo "docs contain stale cargo command; use cargo run --bin memoryd --" >&2
    failed=1
  fi
fi

# Stale shared socket path: /tmp/memoryd.sock is not the canonical default
if stale_socket="$(rg -n '/tmp/memoryd\.sock' "${paths[@]}" 2>/dev/null)"; then
  if [ -n "$stale_socket" ]; then
    printf '%s\n' "$stale_socket" >&2
    echo "docs contain stale shared socket path; use <runtime>/memoryd.sock or command defaults" >&2
    failed=1
  fi
fi

# Stale Codex auth probe: `codex auth status` is only allowed on lines that
# explicitly mark it as fallback/legacy/older-CLI support.
if stale_codex="$(rg -n 'codex auth status' "${paths[@]}" 2>/dev/null | rg -vi 'fallback|legacy|older cli' || true)"; then
  if [ -n "$stale_codex" ]; then
    printf '%s\n' "$stale_codex" >&2
    echo "docs contain stale Codex auth probe; use codex login status as preferred current command" >&2
    failed=1
  fi
fi

# Stale Claude auth probe: `claude config get auth.user` is only allowed on lines
# that explicitly mark it as fallback/legacy/older-CLI support.
if stale_claude="$(rg -n 'claude config get auth\.user' "${paths[@]}" 2>/dev/null | rg -vi 'fallback|legacy|older cli' || true)"; then
  if [ -n "$stale_claude" ]; then
    printf '%s\n' "$stale_claude" >&2
    echo "docs contain stale Claude auth probe as preferred command; use claude auth status as preferred current command" >&2
    failed=1
  fi
fi

# Unexpanded tilde in current command paths: MCP clients do not expand ~ in
# JSON/TOML, and the installer intentionally rejects literal tilde arguments.
# Catches shell `--socket ~/...`, `--repo ~/...`, `--runtime ~/...`, plus quoted
# onboarding/runtime snippets such as `"~/memorum/.memoryd/memoryd.sock"`.
tilde_shell="$(rg -n -- '--(socket|repo|runtime)[[:space:]]+~/' "${paths[@]}" 2>/dev/null || true)"
tilde_dquote="$(rg -n -- '"~/[^"]*(memoryd\.sock|\.memoryd|memorum)' "${paths[@]}" 2>/dev/null || true)"
tilde_squote="$(rg -n -- "'~/[^']*(memoryd\.sock|\.memoryd|memorum)" "${paths[@]}" 2>/dev/null || true)"
tilde_socket="$(
  {
    printf '%s\n' "$tilde_shell"
    printf '%s\n' "$tilde_dquote"
    printf '%s\n' "$tilde_squote"
  } | grep -v '^$' | sort -u || true
)"
if [ -n "$tilde_socket" ]; then
  printf '%s\n' "$tilde_socket" >&2
  echo "docs contain current commands/config with unexpanded ~; use \$HOME, MEMORUM_* env vars, or an absolute placeholder" >&2
  failed=1
fi

exit "$failed"
