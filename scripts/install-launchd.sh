#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install-launchd.sh --repo PATH --runtime PATH [--dry-run]
                                  [--claude-config-dir PATH]

Installs Memorum launchd agents for the current macOS user.
By default installs both the daemon auto-restart agent and scheduled dream job.
Use --daemon or --dream-scheduler to install only one.
Use --dry-run to print the rendered plist without writing or loading it.

--claude-config-dir pins CLAUDE_CONFIG_DIR in the daemon environment so dreaming
authenticates against a specific Claude profile (e.g. $HOME/.claude-personal).
Required when multiple Claude profiles are logged in; otherwise the daemon
auto-detects a single authenticated profile. The daemon does not inherit your
shell PATH, so the rendered plist PATH also includes wherever claude/codex
currently resolve plus $HOME/.local/bin and $HOME/.cargo/bin.
USAGE
}

repo=""
runtime=""
claude_config_dir=""
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
    --claude-config-dir)
      require_option_value "$1" "${2:-}"
      claude_config_dir="${2:-}"
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
if [ -n "$claude_config_dir" ]; then
  reject_literal_tilde "$claude_config_dir"
fi
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
# launchd resolves ProgramArguments[0] itself and ignores the plist's
# EnvironmentVariables PATH, so the executable must be an absolute path — a bare
# "memoryd" fails to spawn with EX_CONFIG. Prefer the resolved on-PATH binary;
# fall back to the standard cargo-install location.
memoryd_bin="$(command -v memoryd 2>/dev/null || true)"
case "$memoryd_bin" in
  /*) : ;;
  *) memoryd_bin="$HOME/.cargo/bin/memoryd" ;;
esac
memoryd_bin_xml="$(xml_escape "$memoryd_bin")"

# Build the daemon PATH. launchd does not inherit the user's shell PATH, so the
# dream harness cannot find `claude`/`codex` unless their directories are named
# here explicitly. Resolve them now (bash sees the real binaries on PATH, not the
# interactive shell-function wrappers) and add the standard user bin locations.
# Dedup is order-preserving and bash 3.2-safe (no associative arrays).
daemon_path=""
append_path_dir() {
  local dir="$1"
  [ -z "$dir" ] && return 0
  case ":$daemon_path:" in
    *":$dir:"*) return 0 ;;
  esac
  if [ -z "$daemon_path" ]; then
    daemon_path="$dir"
  else
    daemon_path="$daemon_path:$dir"
  fi
}
for dir in /opt/homebrew/bin /usr/local/bin /usr/bin /bin /usr/sbin /sbin "$HOME/.cargo/bin" "$HOME/.local/bin"; do
  append_path_dir "$dir"
done
for tool in claude codex; do
  resolved="$(command -v "$tool" 2>/dev/null || true)"
  case "$resolved" in
    /*) append_path_dir "$(cd "$(dirname "$resolved")" && pwd -P)" ;;
  esac
done
daemon_path_xml="$(xml_escape "$daemon_path")"

# launchd user agents do not get USER in their environment, but Claude's macOS
# keychain lookup needs it (without USER, `claude auth status` reports
# loggedIn:false even with a valid CLAUDE_CONFIG_DIR). Resolve it now so both
# plists carry it. `id -un` is robust even when $USER is unset.
user_name="$(id -un)"
user_name_xml="$(xml_escape "$user_name")"

# Optional CLAUDE_CONFIG_DIR entry, appended to the EnvironmentVariables dict as
# a single-line key/value pair (empty when --claude-config-dir was not given).
# Single-line avoids awk's literal-newline handling in -v; plist XML is
# whitespace-insensitive so the formatting is irrelevant.
claude_config_dir_entry=""
if [ -n "$claude_config_dir" ]; then
  claude_config_dir_path="$(canonical_path "$claude_config_dir")"
  claude_config_dir_xml="$(xml_escape "$claude_config_dir_path")"
  claude_config_dir_entry="<key>CLAUDE_CONFIG_DIR</key><string>${claude_config_dir_xml}</string>"
fi

embed_idle_unload_entry=""
if [ "${MEMORUM_EMBED_IDLE_UNLOAD_SECS+x}" = x ]; then
  embed_idle_unload_xml="$(xml_escape "$MEMORUM_EMBED_IDLE_UNLOAD_SECS")"
  embed_idle_unload_entry="<key>MEMORUM_EMBED_IDLE_UNLOAD_SECS</key><string>${embed_idle_unload_xml}</string>"
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
launch_agents="$HOME/Library/LaunchAgents"

render_template() {
  local template="$1"
  awk \
    -v repo="$repo_path_xml" \
    -v runtime="$runtime_path_xml" \
    -v home="$home_xml" \
    -v memoryd_bin="$memoryd_bin_xml" \
    -v daemon_path="$daemon_path_xml" \
    -v user_name="$user_name_xml" \
    -v ccd_entry="$claude_config_dir_entry" \
    -v embed_idle_entry="$embed_idle_unload_entry" '
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
      line = replace_all(line, "{{MEMORYD_BIN}}", memoryd_bin)
      line = replace_all(line, "{{PATH}}", daemon_path)
      line = replace_all(line, "{{USER}}", user_name)
      line = replace_all(line, "{{CLAUDE_CONFIG_DIR_ENTRY}}", ccd_entry)
      line = replace_all(line, "{{MEMORUM_EMBED_IDLE_UNLOAD_SECS_ENTRY}}", embed_idle_entry)
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
