# Stream G Architecture

Stream G is an observability/UI layer over the shipped substrate, daemon, governance, privacy, recall, and dreaming streams. The design keeps one mutation boundary: UI and web code never write repo files directly.

## Crate split

- `crates/memoryd-tui/`: `ratatui`/`crossterm` TUI, panel state, widgets, keymap tests, socket client wrapper.
- `crates/memoryd-web/`: embedded `axum` localhost dashboard, CSRF middleware, static assets, route contract tests.
- `crates/memoryd/`: daemon protocol, Reality Check scheduler/session handler, notification dispatcher, trust artifact assembly.
- `crates/memory-substrate/`: canonical files, derived SQLite index, `events_log`, `memory_supersession`, event append APIs.

The split keeps terminal and HTTP dependencies disjoint. Both UI crates depend on the daemon protocol/client surface, not on `memory-substrate` internals.

## Data flow

```text
TUI / web / CLI
    -> owner-only Unix socket
    -> memoryd protocol handlers
    -> Stream C/D mutation paths when an action mutates memory
    -> Stream A substrate + events JSONL + derived SQLite index
    -> response DTOs back to UI
```

Reads such as status, review queue, timeline, audit, and Reality Check list all enter through the daemon. Mutations such as confirm/correct/forget/not-relevant/skip are serialized by daemon handlers. This preserves Stream A as the only canonical substrate/index and prevents UI crates from bypassing governance or privacy checks.

## Reality Check scoring

Scoring is computed from index-visible fields plus event aggregates. It does not hydrate every memory body.

Formula:

```text
score(m) = 0.35 * days_since_observed_norm(m)
         + 0.20 * (1 - recall_frequency_norm(m))
         + 0.20 * (1 - cross_source_corroboration(m))
         + 0.15 * confidence_decay(m)
         + 0.10 * sensitivity_weight(m)
```

Pipeline:

1. Scan active/pinned, passive-recall-enabled rows from the recall/index projection.
2. Aggregate recent `RecallHit` counts from `events_log` with the covering index on `(kind, memory_id, ts)`.
3. Compute cross-source corroboration from `memory_supersession` plus `memories.source_harness`, using the depth-bounded recursive CTE.
4. Combine component scores, sort descending, keep configured top N.
5. Store a cache in `state/reality-check-pending.json`; recompute when stale or reset.

`drift.risk` is a UI/reporting concept backed by this score formula. `recall_count_30d`, `last_recalled_at`, and trust artifact recall fields come from `events_log`; there is no drift or recall counter column on `memories`.

## Events and covering index

Canonical events remain JSONL under the repo/runtime event log. Stream A mirrors them into rebuildable SQLite `events_log` for SQL consumers. The covering index supports:

- `RecallHit` count and max timestamp lookups for scoring and trust artifact rendering.
- timeline filtering by kind/memory/time.
- ROI windows and notification/report metrics.

Mirror health is observable through daemon doctor. If JSONL has events missing from SQLite, doctor reports `events_log_mirror_lag` and repair is `memoryd doctor --reindex`.

## Notification dispatch

`memoryd` owns a `tokio::sync::broadcast::Sender<NotificationEvent>`. Producers publish security, merge, review, dream, Reality Check, and summary events. The dispatcher consumes one receiver and routes each event:

1. append to passive in-memory queue, capped FIFO;
2. send OS notification only if enabled and the trigger matches;
3. send Slack/email only if configured and the trigger matches.

External payloads intentionally carry no memory content. Passive notifications are read by `memoryd status` and Stream E pending-attention assembly.

## State files and crash recovery

State is device-local under `<runtime_root>/state/` and excluded from git sync:

- `state.json`: `last_completed_at` and `snooze_until`.
- `reality-check-pending.json`: cached scored `RealityCheckItem` list.
- `reality-check-session.json`: in-flight session progress.

All writes use tempfile-then-rename. Missing or corrupt `state.json` falls back to no prior completion/snooze. Missing or stale pending cache triggers recomputation. Corrupt session files are renamed aside for forensics and treated as no active session. Session files older than seven days are discarded on daemon startup.

`memoryd reality-check reset` is the operator escape hatch: it clears pending/session state and lets the next scheduled or manual run recompute from canonical substrate and events.

## v1.1+ deferrals

Deferred v1.1+ work includes the web policy editor, web sync dashboard, remote dashboard auth, durable external notification dead-letter storage, theme/mouse support for TUI, and richer cross-device claim-lock UI once Stream I ships. Stream G v1 ships human observability and Reality Check without overclaiming Stream H/I behavior.
