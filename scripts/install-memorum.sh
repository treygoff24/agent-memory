#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install-memorum.sh [--repo PATH] [--runtime PATH] [--socket PATH] [--with-scheduler] [--dry-run]

Builds/installs memoryd, initializes a local repo/runtime, starts the daemon,
prints an MCP client snippet, and optionally installs the launchd scheduler.
USAGE
}

repo="$HOME/memorum"
runtime=""
socket="/tmp/memoryd.sock"
with_scheduler=0
dry_run=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      repo="${2:-}"
      shift 2
      ;;
    --runtime)
      runtime="${2:-}"
      shift 2
      ;;
    --socket)
      socket="${2:-}"
      shift 2
      ;;
    --with-scheduler)
      with_scheduler=1
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$runtime" ]; then
  runtime="$repo/.memoryd"
fi

run() {
  if [ "$dry_run" -eq 1 ]; then
    printf '+ %q' "$1"
    shift
    for arg in "$@"; do printf ' %q' "$arg"; done
    printf '\n'
  else
    "$@"
  fi
}

if [ "$dry_run" -eq 0 ]; then
  cargo install --path crates/memoryd --locked
else
  echo "+ cargo install --path crates/memoryd --locked"
fi
run mkdir -p "$repo" "$runtime"

if [ "$dry_run" -eq 0 ]; then
  memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" >/tmp/memoryd-install.log 2>&1 &
  daemon_pid=$!
  ready=0
  for _ in 1 2 3 4 5; do
    if memoryd status --socket "$socket" >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 1
  done
  if [ "$ready" -ne 1 ]; then
    kill "$daemon_pid" >/dev/null 2>&1 || true
    echo "memoryd daemon did not become ready within 5s; see /tmp/memoryd-install.log" >&2
    exit 1
  fi
else
  echo "+ memoryd serve --init --repo $repo --runtime $runtime --socket $socket"
  echo "+ memoryd status --socket $socket"
fi

cat <<SNIPPET
MCP client snippet:
{
  "mcpServers": {
    "memorum": {
      "command": "memoryd",
      "args": ["mcp", "--socket", "$socket"]
    }
  }
}
SNIPPET

if command -v claude >/dev/null 2>&1; then
  echo "✓ harness CLI detected: claude"
fi
if command -v codex >/dev/null 2>&1; then
  echo "✓ harness CLI detected: codex"
fi
if ! command -v claude >/dev/null 2>&1 && ! command -v codex >/dev/null 2>&1; then
  echo "warning: no supported harness CLI detected; dreams stay inactive until claude or codex is installed. See docs/runbooks/dream-scheduling.md."
fi

if [ "$with_scheduler" -eq 1 ]; then
  scripts/install-launchd.sh --repo "$repo" --runtime "$runtime"
fi
