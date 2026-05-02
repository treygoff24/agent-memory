# Reality Check Runbook

Reality Check is the weekly human ritual for reviewing memories most likely to have drifted. It is an admin/UI surface, not an MCP tool.

## Weekly ritual

1. Notice the due signal:
   - TUI Panel 8 shows `DUE`;
   - `memoryd status` shows a passive notification;
   - startup recall may include a `<pending-attention>` item: `<item kind="reality_check_due" count="1">Weekly Reality Check is ready — run memoryd reality-check run or open TUI panel 8.</item>`;
   - optional Slack/email says the weekly check is ready without memory content.
2. Start the session:
   - CLI: `memoryd reality-check run`;
   - TUI: `memoryd ui`, open panel 8, press `r`;
   - web: open dashboard Section 3 and start Reality Check.
3. Review each item. The score formula is shown as component scores so the drift.risk ordering is explainable.
4. Choose one action per item:
   - **confirm**: memory is still true; updates observed metadata and appends `RealityCheckConfirmed`.
   - **correct**: provide replacement body; daemon routes through normal governance supersession.
   - **forget**: provide a reason; daemon routes through governance tombstone/forget path and appends `RealityCheckForgotten`.
   - **not relevant**: disables passive recall and tags the memory; it remains searchable and appends `RealityCheckNotRelevant`.
   - **skip this week**: defer this item only; no frontmatter mutation.
5. Complete the session. When all non-deferred items are handled, daemon writes `last_completed_at`, deletes `reality-check-session.json`, and clears the due pending-attention item for future recall blocks.

For automation or inspection:

```bash
memoryd reality-check run --json --top-n 10
```

This prints the scored list and exits without an interactive ritual. Omit `--top-n` to use the daemon default.

## Abandon and resume

If the TUI closes, the daemon restarts, or the user stops mid-session, progress persists in:

```text
<runtime_root>/state/reality-check-session.json
```

Next `memoryd reality-check run` offers to resume the previous session. Resume keeps reviewed/deferred/remaining item state. Decline/discard starts a fresh run. Session files older than seven days are auto-discarded because the weekly queue is stale.

If a session file is corrupt, the daemon renames it to `reality-check-session.json.corrupt-<timestamp>` and starts fresh. No canonical memory data is lost; the queue is recomputed from Stream A index/events.

## Snooze vs skip

- **Snooze** (`memoryd reality-check snooze --until <date>` or TUI Panel 8 `s`) suppresses the whole weekly reminder until the snooze expires. It suppresses `RealityCheckDue` notifications and the `reality_check_due` pending-attention line for that week.
- **Skip this week** (`space` on an item or CLI/web equivalent) defers only the current item. It does not mark the session complete by itself and does not change memory frontmatter.

Use snooze when the user cannot do the ritual this week. Use skip when one memory should wait but the rest of the session should continue.

## Overdue behavior

If `last_completed_at` is more than 21 days old, Reality Check is overdue. The daemon fires `NotificationEvent::RealityCheckOverdue` once at threshold crossings of 3, 6, and 12 skipped weeks. TUI/web/CLI show an overdue warning.

On overdue runs, previously skipped items are reintroduced as normal items and the pending list is re-sorted by current score so the highest drift.risk memories appear first.

## Encrypted memories

Encrypted memories are scored from safe index-visible fields only: namespace, timestamps, sensitivity, recall events, and other safe projections. UI text shows `[encrypted — title not available]` or `[encrypted item, score: X.XX]`; body and title are not revealed.

Allowed without reveal: forget or skip. Confirm and correct require explicit user-directed reveal through the existing Stream D reveal path before action. The web/TUI does not silently decrypt encrypted bodies.

## Stuck state

If Reality Check appears stuck, repeatedly resumes the wrong session, reports corrupt state, or exits with stuck-state code 5, first run:

```bash
memoryd doctor --reindex
```

Use this when scoring or trust artifact recall counts look stale; doctor will surface `events_log_mirror_lag` if the SQLite mirror is behind JSONL.

If the session file itself is corrupt, the daemon should quarantine it automatically on the next run. The implemented daemon protocol includes an admin `RealityCheckRequest::Reset` variant, but the implemented CLI intentionally does not expose `memoryd reality-check reset`; do not document it as an operator command unless a CLI subcommand is implemented.

After repair, rerun:

```bash
memoryd reality-check run
```

## v1.1+ deferrals

Deferred v1.1+ operator work includes richer stuck-state diagnostics, durable external-notification retry queues, and remote dashboard auth. The v1 runbook intentionally covers local daemon/TUI/web/CLI operation only.
