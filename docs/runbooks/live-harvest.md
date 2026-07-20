# Live harvest

Live harvest periodically imports Claude Code and Codex auto-memory through the running daemon's normal privacy and governance path. It is device-local and opt-in.

## Enable or disable

```bash
memoryd config harvest enable
memoryd config harvest enable --interval-minutes 60
memoryd config harvest disable
```

Use `--repo PATH --runtime PATH` for a non-default installation. The effective interval is clamped to 5–1440 minutes. Changes require no restart; the scheduler reads `local-device.yaml` on its next wake (up to five minutes while disabled).

## Diagnose

Run `memoryd doctor` and inspect `.result.success.doctor.harvest`:

- `enabled`, `interval_minutes`: effective local config.
- `never_run: true`: no valid `harvest-state.json` exists; counts are intentionally absent.
- `last_attempt_at`, `last_success_at`, `next_due`: cadence and retry history.
- `harnesses`: last completed attempt counts for `claude-code` and `codex`.
- `last_error`: latest bounded scheduler or per-source/discovery error.
- `active_embedding_lane`: embedding provider active at the last attempt.

Lock contention with a manual `memoryd import` is normal: the scheduled tick skips immediately, writes no state, and retries while overdue.

## Local embedding cadence

The API embedding lane stays warm and adds negligible resident cost per harvested memory. The local lane may unload its model after an idle window; a trickle of new memories can therefore reload the model once per harvest. Set `interval_minutes` at least as long as the local idle-unload window (15 minutes by default) to avoid unnecessary reload churn, or accept the reload cost for fresher cross-harness memory.
