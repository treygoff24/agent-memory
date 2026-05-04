#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install-launchd.sh --repo PATH --runtime PATH [--dry-run]

Installs the Memorum scheduled dream launchd agent for the current macOS user.
Use --dry-run to print the rendered plist without writing or loading it.
USAGE
}

repo=""
runtime=""
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

if [ -z "$repo" ] || [ -z "$runtime" ]; then
  usage >&2
  exit 2
fi

repo_path=$(cd "$repo" 2>/dev/null && pwd || printf '%s' "$repo")
mkdir -p "$runtime"
runtime_path=$(cd "$runtime" && pwd)
template="$(dirname "$0")/templates/com.memorum.dream-scheduled.plist.template"
label="com.memorum.dream-scheduled"
launch_agents="$HOME/Library/LaunchAgents"
target="$launch_agents/$label.plist"

rendered=$(sed \
  -e "s#{{REPO_PATH}}#$repo_path#g" \
  -e "s#{{RUNTIME_PATH}}#$runtime_path#g" \
  "$template")

if [ "$dry_run" -eq 1 ]; then
  printf '%s\n' "$rendered"
  exit 0
fi

mkdir -p "$launch_agents"
printf '%s\n' "$rendered" > "$target"
launchctl unload "$target" >/dev/null 2>&1 || true
launchctl load "$target"
echo "installed and loaded $target"
