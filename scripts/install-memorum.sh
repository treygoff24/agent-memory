#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/install-memorum.sh [--repo PATH] [--runtime PATH] [--socket PATH] [--with-scheduler] [--dry-run] [--force-reinstall]

Builds/installs memoryd, initializes a local repo/runtime, starts the daemon,
prints an MCP client snippet, and optionally installs the launchd scheduler.
USAGE
}

repo="$HOME/memorum"
runtime=""
socket="/tmp/memoryd.sock"
with_scheduler=0
dry_run=0
force_reinstall=0

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
    --force-reinstall)
      force_reinstall=1
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

pid_file="$runtime/memoryd.pid"
log_file="$runtime/memoryd.log"

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

memoryd_expected_version() {
  awk -F\" '/^version[[:space:]]*=/ { print $2; exit }' crates/memoryd/Cargo.toml
}

installed_memoryd_version() {
  local version_output
  if ! version_output="$(memoryd --version 2>/dev/null)"; then
    return 0
  fi
  awk '{ print $2; exit }' <<<"$version_output"
}

chmod_runtime_dir() {
  if [ "$dry_run" -eq 1 ]; then
    echo "+ chmod 700 $runtime"
  else
    chmod 700 "$runtime"
  fi
}

chmod_private_file() {
  local path="$1"
  if [ "$dry_run" -eq 1 ]; then
    echo "+ chmod 600 $path"
  else
    chmod 600 "$path"
  fi
}

pid_is_numeric() {
  case "$1" in
    ''|*[!0-9]*)
      return 1
      ;;
    *)
      return 0
      ;;
  esac
}

pid_matches_expected_daemon() {
  local pid="$1"
  local command_line
  command_line="$(ps -p "$pid" -o command= 2>/dev/null || true)"
  [ -n "$command_line" ] \
    && [[ "$command_line" == *"memoryd serve"* ]] \
    && [[ "$command_line" == *"--repo $repo"* ]] \
    && [[ "$command_line" == *"--runtime $runtime"* ]] \
    && [[ "$command_line" == *"--socket $socket"* ]]
}

install_memoryd_if_needed() {
  local expected installed
  expected="$(memoryd_expected_version)"
  installed=""
  if command -v memoryd >/dev/null 2>&1; then
    installed="$(installed_memoryd_version)"
  fi

  if [ "$force_reinstall" -eq 0 ] && [ -n "$expected" ] && [ "$installed" = "$expected" ]; then
    echo "memoryd v$expected already installed; skipping cargo install"
    return
  fi

  if [ "$dry_run" -eq 0 ]; then
    cargo install --path crates/memoryd --locked
  else
    echo "+ cargo install --path crates/memoryd --locked"
  fi
}

stop_existing_daemon() {
  if [ ! -f "$pid_file" ]; then
    return
  fi

  local existing_pid
  existing_pid="$(cat "$pid_file" 2>/dev/null || true)"
  if ! pid_is_numeric "$existing_pid"; then
    echo "warning: ignoring malformed PID file $pid_file" >&2
    if [ "$dry_run" -eq 1 ]; then
      echo "+ rm -f $pid_file"
      return
    fi
    rm -f "$pid_file"
    return
  fi

  if [ -n "$existing_pid" ] && kill -0 "$existing_pid" >/dev/null 2>&1; then
    if ! pid_matches_expected_daemon "$existing_pid"; then
      echo "warning: $pid_file points to PID $existing_pid, but it is not the expected memoryd serve process; leaving process untouched" >&2
      if [ "$dry_run" -eq 1 ]; then
        echo "+ rm -f $pid_file"
        return
      fi
      rm -f "$pid_file"
      return
    fi
    if [ "$dry_run" -eq 1 ]; then
      echo "+ kill $existing_pid"
      echo "+ wait for $existing_pid to exit"
      return
    fi

    kill "$existing_pid" >/dev/null 2>&1 || true
    for _i in 1 2 3 4 5; do
      if ! kill -0 "$existing_pid" >/dev/null 2>&1; then
        break
      fi
      sleep 1
    done
  fi
  if [ "$dry_run" -eq 1 ]; then
    echo "+ rm -f $pid_file"
    return
  fi
  rm -f "$pid_file"
}

install_memoryd_if_needed
run mkdir -p "$repo" "$runtime"
chmod_runtime_dir
stop_existing_daemon

if [ "$dry_run" -eq 0 ]; then
  : >"$log_file"
  chmod_private_file "$log_file"
  nohup memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" </dev/null >>"$log_file" 2>&1 &
  daemon_pid=$!
  disown "$daemon_pid" >/dev/null 2>&1 || true
  ready=0
  for _i in 1 2 3 4 5; do
    if memoryd status --socket "$socket" >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 1
  done
  if [ "$ready" -ne 1 ]; then
    kill "$daemon_pid" >/dev/null 2>&1 || true
    rm -f "$pid_file"
    echo "memoryd daemon did not become ready within 5s; see $log_file" >&2
    exit 1
  fi
  if ! kill -0 "$daemon_pid" >/dev/null 2>&1; then
    rm -f "$pid_file"
    echo "memoryd daemon exited after readiness check; see $log_file" >&2
    exit 1
  fi
  printf '%s\n' "$daemon_pid" >"$pid_file"
  chmod_private_file "$pid_file"
else
  echo "+ : > $log_file"
  chmod_private_file "$log_file"
  echo "+ nohup memoryd serve --init --repo $repo --runtime $runtime --socket $socket </dev/null >>$log_file 2>&1 &"
  echo "+ echo <daemon-pid> > $pid_file"
  chmod_private_file "$pid_file"
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

if [ "$dry_run" -eq 0 ]; then
  cat <<LIFECYCLE
memoryd is running (PID: $daemon_pid, log: $log_file).
To stop:    kill \$(cat "$pid_file")
To restart: bash scripts/install-memorum.sh --repo "$repo" --runtime "$runtime" --socket "$socket"
To install as a launchd agent (auto-restart on login): rerun with --with-scheduler.
LIFECYCLE
else
  cat <<LIFECYCLE
memoryd lifecycle paths:
PID file: $pid_file
Log file: $log_file
To stop:    kill \$(cat "$pid_file")
To restart: bash scripts/install-memorum.sh --repo "$repo" --runtime "$runtime" --socket "$socket"
To install as a launchd agent (auto-restart on login): rerun with --with-scheduler.
LIFECYCLE
fi

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
