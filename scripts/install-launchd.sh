#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install-launchd.sh --repo PATH --runtime PATH [--dry-run]

Installs Memorum launchd agents for the current macOS user.
By default installs both the daemon auto-restart agent and scheduled dream job.
Use --daemon or --dream-scheduler to install only one.
Use --dry-run to print the rendered plist without writing or loading it.
USAGE
}

repo=""
runtime=""
dry_run=0
install_daemon=0
install_dream=0

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
    --dry-run)
      dry_run=1
      shift
      ;;
    --daemon)
      install_daemon=1
      shift
      ;;
    --dream-scheduler)
      install_dream=1
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

if [ -z "$repo" ] || [ -z "$runtime" ]; then
  usage >&2
  exit 2
fi
if [ "$install_daemon" -eq 0 ] && [ "$install_dream" -eq 0 ]; then
  install_daemon=1
  install_dream=1
fi

repo_path=$(cd "$repo" 2>/dev/null && pwd || printf '%s' "$repo")
mkdir -p "$runtime"
runtime_path=$(cd "$runtime" && pwd)
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
launch_agents="$HOME/Library/LaunchAgents"

render_template() {
  local template="$1"
  sed \
    -e "s#{{REPO_PATH}}#$repo_path#g" \
    -e "s#{{RUNTIME_PATH}}#$runtime_path#g" \
    -e "s#{{HOME}}#$HOME#g" \
    "$template"
}

if [ "$dry_run" -eq 1 ]; then
  if [ "$install_daemon" -eq 1 ]; then
    render_template "$script_dir/templates/com.memorum.daemon.plist.template"
  fi
  if [ "$install_daemon" -eq 1 ] && [ "$install_dream" -eq 1 ]; then
    printf '\n'
  fi
  if [ "$install_dream" -eq 1 ]; then
    render_template "$script_dir/templates/com.memorum.dream-scheduled.plist.template"
  fi
  exit 0
fi

mkdir -p "$launch_agents"

install_agent() {
  local label="$1"
  local template="$2"
  local target="$launch_agents/$label.plist"
  render_template "$template" >"$target"
  launchctl bootout "gui/$(id -u)" "$target" >/dev/null 2>&1 || true
  launchctl bootstrap "gui/$(id -u)" "$target"
  echo "installed and bootstrapped $target"
}

if [ "$install_daemon" -eq 1 ]; then
  install_agent "com.memorum.daemon" "$script_dir/templates/com.memorum.daemon.plist.template"
fi
if [ "$install_dream" -eq 1 ]; then
  install_agent "com.memorum.dream-scheduled" "$script_dir/templates/com.memorum.dream-scheduled.plist.template"
fi
