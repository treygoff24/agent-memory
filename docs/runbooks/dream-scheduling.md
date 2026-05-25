# Dream scheduling runbook

Memorum dogfood scheduling is macOS launchd-only for this release. The agent runs `memoryd dream scheduled --scope me` once per day at 03:00 local time.

## Install

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"

scripts/install-launchd.sh --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
```

Preview without writing:

```bash
scripts/install-launchd.sh --dry-run --repo /tmp/foo --runtime /tmp/bar
```

The installer renders `scripts/templates/com.memorum.dream-scheduled.plist.template`, writes it to `$HOME/Library/LaunchAgents/com.memorum.dream-scheduled.plist`, unloads any previous copy, then loads the new one.

## Inspect

```bash
launchctl list | grep com.memorum.dream-scheduled
launchctl print gui/$(id -u)/com.memorum.dream-scheduled
cat "$MEMORUM_RUNTIME/dream-scheduled.out.log"
cat "$MEMORUM_RUNTIME/dream-scheduled.err.log"
```

## Uninstall

```bash
launchctl unload "$HOME/Library/LaunchAgents/com.memorum.dream-scheduled.plist"
rm "$HOME/Library/LaunchAgents/com.memorum.dream-scheduled.plist"
```

Dreams still require at least one authenticated harness CLI (`claude` or `codex`) visible to the launchd job's environment.
