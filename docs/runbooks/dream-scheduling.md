# Dream scheduling runbook

Memorum dogfood scheduling is macOS launchd-only for this release. The agent runs `memoryd dream scheduled --scope me` once per day at 03:00 local time.

## Install

```bash
scripts/install-launchd.sh --repo ~/memorum --runtime ~/memorum/.memoryd
```

Preview without writing:

```bash
scripts/install-launchd.sh --dry-run --repo /tmp/foo --runtime /tmp/bar
```

The installer renders `scripts/templates/com.memorum.dream-scheduled.plist.template`, writes it to `~/Library/LaunchAgents/com.memorum.dream-scheduled.plist`, unloads any previous copy, then loads the new one.

## Inspect

```bash
launchctl list | grep com.memorum.dream-scheduled
launchctl print gui/$(id -u)/com.memorum.dream-scheduled
cat ~/memorum/.memoryd/dream-scheduled.out.log
cat ~/memorum/.memoryd/dream-scheduled.err.log
```

## Uninstall

```bash
launchctl unload ~/Library/LaunchAgents/com.memorum.dream-scheduled.plist
rm ~/Library/LaunchAgents/com.memorum.dream-scheduled.plist
```

Dreams still require at least one authenticated harness CLI (`claude` or `codex`) visible to the launchd job's environment.
