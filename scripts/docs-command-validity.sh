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

current_operator_docs=(
  README.md
  docs/getting-started.md
  docs/mcp-wiring.md
  docs/api/stream-b-daemon-mcp-api.md
  docs/api/web-source-grounding-api.md
  docs/api/stream-g-observability-api.md
  docs/api/stream-h-eval-api.md
  docs/dev/stream-h-test-catalog.md
  docs/runbooks/dogfooding-day-one.md
)

failed=0

# Stale Codex MCP TOML header: current shape is [mcp_servers.<name>]
if stale_codex_mcp="$(rg -n '^\[mcp\.' README.md docs/getting-started.md docs/mcp-wiring.md 2>/dev/null || true)"; then
  if [ -n "$stale_codex_mcp" ]; then
    printf '%s\n' "$stale_codex_mcp" >&2
    echo "docs contain stale Codex MCP TOML header; use [mcp_servers.<name>]" >&2
    failed=1
  fi
fi

# macOS-specific placeholder paths in onboarding docs
if stale_macos_placeholder="$(rg -n '/Users/you/' README.md docs/getting-started.md docs/mcp-wiring.md 2>/dev/null || true)"; then
  if [ -n "$stale_macos_placeholder" ]; then
    printf '%s\n' "$stale_macos_placeholder" >&2
    echo "onboarding docs contain macOS-specific /Users/you/ placeholder; use /absolute/path/to/memorum" >&2
    failed=1
  fi
fi

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
# Catches shell `--socket ~/...`, `--repo=~/...`, plus quoted onboarding/runtime
# snippets such as `"~/memorum/.memoryd/memoryd.sock"`.
tilde_shell="$(rg -n -- '--(socket|repo|runtime)(=|[[:space:]]+)~/' "${paths[@]}" 2>/dev/null || true)"
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

# Current operator/API docs may mention limitations, but placeholder/deferred
# language must be classified as historical or explicitly unsupported. This
# catches stale "will return 501 later" docs after a surface becomes alpha-owned.
if stale_status_language="$(
  rg -n 'deferred|coming soon|not implemented|placeholder|stub|not yet|future|v1\.1\+' "${current_operator_docs[@]}" 2>/dev/null \
    | rg -vi 'historical|explicitly unsupported|unsupported alpha|unsupported mode|not implemented / disabled surface|placeholder socket path|replace the placeholder socket path|future-proofing|older CLIs|deferred to v2|model/semantic privacy classification remains unsupported|model privacy filter remains unsupported|browser-rendered capture is unsupported|pairing is unsupported|not full business ROI|alpha release-set|fixture/deferred data|unclassified deferral|deferred required|deferred entries|zero deferred|required deferred|required catalog entry is deferred|always includes `deferred`|must not return a placeholder|feature_deferred|\"deferred\": false' || true
)"; then
  if [ -n "$stale_status_language" ]; then
    printf '%s\n' "$stale_status_language" >&2
    echo "current docs contain unclassified placeholder/deferred language; classify it as historical, unsupported alpha scope, or update the stale claim" >&2
    failed=1
  fi
fi

if missing_source_modes="$(
  for needle in 'http_static' 'local_artifact' 'browser-rendered capture is unsupported' 'model privacy filter remains unsupported'; do
    if ! rg -q "$needle" docs/api/web-source-grounding-api.md; then
      printf 'docs/api/web-source-grounding-api.md missing %s\n' "$needle"
    fi
  done
  for needle in 'source' 'mode' 'local_artifact' 'typed unsupported' 'allow-reveal'; do
    if ! rg -q "$needle" docs/api/stream-b-daemon-mcp-api.md; then
      printf 'docs/api/stream-b-daemon-mcp-api.md missing %s\n' "$needle"
    fi
  done
)"; then
  if [ -n "$missing_source_modes" ]; then
    printf '%s\n' "$missing_source_modes" >&2
    echo "web source grounding docs must list supported alpha modes and explicit unsupported modes" >&2
    failed=1
  fi
fi

if stale_tui_command="$(rg -n '`memory ui`|memory ui' README.md docs/getting-started.md docs/mcp-wiring.md docs/api docs/specs/system-v0.2.md 2>/dev/null || true)"; then
  if [ -n "$stale_tui_command" ]; then
    printf '%s\n' "$stale_tui_command" >&2
    echo "docs contain stale TUI command; use memoryd ui" >&2
    failed=1
  fi
fi

# `memoryd init` shipped (agent-driven onboarding); it replaced `memoryd serve
# --init` as the documented bootstrap entrypoint. The old guard here forbade
# uncaveated `memoryd init` references; the invariant is now the inverse — the
# one-install-story reconciliation (2026-06 docs pass) must not regress.
if ! rg -q 'memoryd init' docs/getting-started.md; then
  echo "docs/getting-started.md must document memoryd init as the bootstrap entrypoint" >&2
  failed=1
fi
if ! rg -q 'agent-onboarding\.md' docs/getting-started.md; then
  echo "docs/getting-started.md must point AI agents at docs/agent-onboarding.md" >&2
  failed=1
fi

if uncaveated_lazy_start="$(
  rg -n 'lazy-start|lazy start|lazy-initialized|lazy initialized' README.md docs/getting-started.md docs/mcp-wiring.md docs/api docs/specs/system-v0.2.md 2>/dev/null \
    | rg -vi 'release-target|unless implemented|future|unsupported|not current alpha' || true
)"; then
  if [ -n "$uncaveated_lazy_start" ]; then
    printf '%s\n' "$uncaveated_lazy_start" >&2
    echo "docs contain uncaveated lazy-start MCP promises; alpha requires starting memoryd serve first" >&2
    failed=1
  fi
fi

if ! rg -q 'not full business ROI' docs/api/stream-g-observability-api.md docs/runbooks/dogfooding-day-one.md; then
  echo "dashboard docs must classify ROI as alpha operational metrics, not full business ROI" >&2
  failed=1
fi

if forbidden_capture_promises="$(
  rg -n 'browser-rendered|authenticated browser|cookie|screenshot|OCR|pairing|model privacy filter|semantic privacy' README.md docs/getting-started.md docs/mcp-wiring.md docs/api docs/runbooks/dogfooding-day-one.md 2>/dev/null \
    | rg -vi 'unsupported|out of scope|not implemented|disabled|explicitly|does not support|No cookies|client-supplied|key_path|raw key material|privacy classification' || true
)"; then
  if [ -n "$forbidden_capture_promises" ]; then
    printf '%s\n' "$forbidden_capture_promises" >&2
    echo "docs promise unsupported alpha behavior; mark it unsupported or remove the promise" >&2
    failed=1
  fi
fi

exit "$failed"
