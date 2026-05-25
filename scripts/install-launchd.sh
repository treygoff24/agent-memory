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

require_option_value() {
  local flag="$1"
  local value="${2:-}"
  if [ -z "$value" ] || [[ "$value" == --* ]]; then
    echo "error: $flag requires a non-empty value" >&2
    usage >&2
    exit 2
  fi
}

reject_literal_tilde() {
  case "$1" in
    '~'|'~/'*|'~'?*)
      echo "error: literal ~ is not expanded here; pass \$HOME/... or an absolute path" >&2
      exit 2
      ;;
  esac
}

absolute_path() {
  if [ -z "$1" ]; then
    echo "error: path must not be empty" >&2
    exit 2
  fi
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$(pwd -P)" "$1" ;;
  esac
}

canonical_path() {
  local path="$1"
  if [ -d "$path" ]; then
    (cd "$path" && pwd -P)
  else
    absolute_path "$path"
  fi
}

xml_escape() {
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

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      require_option_value "$1" "${2:-}"
      repo="${2:-}"
      shift 2
      ;;
    --runtime)
      require_option_value "$1" "${2:-}"
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
reject_literal_tilde "$repo"
reject_literal_tilde "$runtime"
if [ "$install_daemon" -eq 0 ] && [ "$install_dream" -eq 0 ]; then
  install_daemon=1
  install_dream=1
fi

repo_path="$(canonical_path "$repo")"
if [ "$dry_run" -eq 0 ]; then
  mkdir -p "$runtime"
fi
runtime_path="$(canonical_path "$runtime")"
repo_path_xml="$(xml_escape "$repo_path")"
runtime_path_xml="$(xml_escape "$runtime_path")"
home_xml="$(xml_escape "$HOME")"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
launch_agents="$HOME/Library/LaunchAgents"

render_template() {
  local template="$1"
  awk \
    -v repo="$repo_path_xml" \
    -v runtime="$runtime_path_xml" \
    -v home="$home_xml" '
    function replace_all(s, needle, replacement,    out, pos) {
      out = ""
      while ((pos = index(s, needle)) > 0) {
        out = out substr(s, 1, pos - 1) replacement
        s = substr(s, pos + length(needle))
      }
      return out s
    }
    {
      line = replace_all($0, "{{REPO_PATH}}", repo)
      line = replace_all(line, "{{RUNTIME_PATH}}", runtime)
      line = replace_all(line, "{{HOME}}", home)
      print line
    }
  ' "$template"
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
