# Stream G Observability API

Stream G is the human observability layer for shipped Streams A-F. It adds TUI, localhost web, Reality Check, notification, and trust artifact surfaces. It does not add agent-facing MCP tools; Reality Check protocol variants are admin-only and MCP-rejected.

## TUI

Invoke with:

```bash
memoryd ui
```

`memoryd ui` requires a TTY and talks to the running daemon over the owner-only Unix socket. The TUI has no direct substrate access.

Eight panels:

1. **Overview**: daemon, socket, index, sync, review, dreaming, recall, and notification health.
2. **Review Queue**: candidate/quarantined/dream review items with approve/reject/forget/quarantine actions routed through daemon governance.
3. **Conflicts**: merge and sync conflicts with side-by-side context.
4. **Entities**: entity-centric memory lists and trust artifact detail.
5. **Timeline**: recent event feed, including write, supersede, tombstone, recall, dream, sync, privacy, review, conflict, notification, and reality-check events.
6. **Namespace Explorer**: namespace tree, memory preview, and trust artifact access.
7. **Policy Inspector**: active governance policies, recent decisions, refusal counts, reload/edit helpers.
8. **Reality Check**: due status, drift-risk queue, score breakdown, and confirm/correct/forget/not-relevant/skip actions.

Global keymap:

| Key             | Action                                            |
| --------------- | ------------------------------------------------- |
| `1`-`8`         | Switch to panel N                                 |
| `?`             | Help overlay                                      |
| `q`             | Quit; confirms when review actions are pending    |
| `Ctrl-c`        | Quit immediately                                  |
| `Ctrl-r`        | Force daemon refresh                              |
| `:`             | Command prompt (`:q`, `:reload`, `:help <topic>`) |
| `j`/`k`, arrows | Navigate lists, trees, and panes                  |
| `h`/`l`         | Collapse/expand or move left/right                |
| `gg` / `G`      | First / last item                                 |
| `Enter`         | Open detail or confirm selection                  |
| `Esc`           | Close modal/cancel/back                           |
| `/`             | Search or filter                                  |
| `tab`           | Cycle focus                                       |
| `u`             | Undo last action before daemon call fires         |

Panel 8 local keys: `r` run, `s` snooze this week, `h` history, `c` confirm, `k` correct, `f` forget, `n` not relevant, `space` skip this week.

## Web dashboard

`memoryd web enable` starts the embedded localhost dashboard. It must bind only to `127.0.0.1`; `0.0.0.0` is rejected. Static assets are embedded and self-hosted.

Static routes:

| Route                   | Shape                        |
| ----------------------- | ---------------------------- |
| `GET /`                 | SPA shell with CSRF meta tag |
| `GET /assets/app.js`    | Embedded JS bundle           |
| `GET /assets/style.css` | Embedded stylesheet          |
| `GET /assets/fonts/*`   | Self-hosted fonts            |

API routes return `application/json`:

| Route                                               | Response / body                                                  |
| --------------------------------------------------- | ---------------------------------------------------------------- |
| `GET /api/status`                                   | daemon status JSON                                               |
| `GET /api/entity-graph?namespace=&depth=&focus=`    | `{ nodes, edges }` entity/co-mention/supersession graph          |
| `GET /api/entity-graph/:entity_id`                  | entity detail, memories, supersession chain, recall history      |
| `GET /api/roi?window=30\|90\|365`                   | promotion, refusal, dream, and Reality Check adherence metrics   |
| `GET /api/reality-check`                            | `RealityCheckResponse::Pending`-compatible status/list           |
| `POST /api/reality-check/respond`                   | body `{ memory_id, action, correction? }`; returns action result |
| `GET /api/reality-check/history?limit=`             | completed-session summaries                                      |
| `GET /api/audit/:id`                                | top-level audit/trust artifact object                            |
| `GET /api/audit/:id/walk?direction=up\|down&depth=` | provenance graph walk                                            |
| `GET /api/audit/:id/temporal?at=`                   | read-only temporal state                                         |
| `GET /api/review?status=&namespace=&limit=&offset=` | review queue page                                                |
| `POST /api/review/action`                           | body `{ id, action, reason? }`; returns review action result     |

CSRF: on server start, the dashboard generates a random 32-byte token, emits it in `<meta name="csrf-token">`, and requires `X-Memorum-CSRF` on every POST. Missing or wrong CSRF returns 403. Concurrent mutations serialize through the daemon; stale review/reality-check mutations return 409 with a typed JSON error.

Deferred v1.1+ web sections: policy editor and sync dashboard routes are not implemented in v1. They should return 501 with a JSON note until a v1.1+ stream owns them.

## Reality Check CLI

All commands route through daemon `RequestPayload::RealityCheck`; there is no direct-substrate fallback.

```bash
memoryd reality-check run [--json] [--namespace <ns>] [--top-n <n>]
memoryd reality-check skip
memoryd reality-check snooze [--until <yyyy-mm-dd>]
```

Exit codes:

| Code | Meaning                                                   |
| ---- | --------------------------------------------------------- |
| 0    | success                                                   |
| 1    | invalid request, bad args, non-TTY interactive invocation |
| 2    | daemon/socket/substrate unavailable                       |
| 3    | governance or privacy refusal for an item action          |
| 4    | not implemented / disabled surface                        |
| 5    | stuck or corrupt Reality Check state                      |

`run --json` prints the pending scored list and exits without prompts. `run --top-n <n>` forwards `limit: Some(n)` to the daemon for both JSON/list and interactive runs. Interactive `run` walks the weekly ritual item by item. `snooze --until <yyyy-mm-dd>` suppresses due notifications and `<pending-attention kind="reality_check_due">` until midnight UTC on that date; omitting `--until` lets the daemon choose the default snooze window. `skip` defers the current session/week without mutating memory frontmatter. The implemented CLI does not expose a `reset` subcommand; daemon protocol still has `Reset` for admin callers that wire a stuck-state repair path.

## Daemon protocol

Reality Check wire variants are admin-only, reachable through the Unix socket by CLI/TUI/web, and MCP-rejected with `MethodNotAllowedOnMcp`.

```rust
pub enum RealityCheckRequest {
    List { namespace: Option<String>, limit: Option<usize> },
    Run { session_id: Option<String>, namespace: Option<String>, limit: Option<usize> },
    Respond { session_id: String, memory_id: MemoryId, action: RealityCheckAction },
    Skip,
    Snooze { until: Option<DateTime<Utc>> },
    Reset,
}

pub enum RealityCheckAction {
    Confirm,
    Correct { new_body: String },
    Forget { reason: String },
    NotRelevant,
    SkipThisWeek,
}

pub enum RealityCheckResponse {
    Pending { session_id: Option<String>, items: Vec<RealityCheckItem>, total_scored: usize, last_completed_at: Option<DateTime<Utc>> },
    RespondAccepted { session_id: String, memory_id: MemoryId, next_item: Option<RealityCheckItem>, completion: RealityCheckCompletion },
    RespondRefused { session_id: String, memory_id: MemoryId, reason: String, kind: RespondRefusalKind },
    Snoozed { snooze_until: DateTime<Utc> },
    Skipped { skipped_until: DateTime<Utc> },
    Reset { cleared_pending: usize, cleared_session: bool },
}
```

`RealityCheckItem` includes `memory_id`, safe `title` (empty for encrypted items), `namespace`, `status`, `sensitivity`, final `score`, `component_scores`, `encrypted`, `last_observed_at`, `recall_count_30d`, and `last_recalled_at`. `ComponentScores` serializes as snake_case: `days_since_observed_norm`, `recall_frequency_norm`, `cross_source_corroboration`, `confidence_decay`, `sensitivity_weight`.

## NotificationEvent

`NotificationEvent` is an internal `tokio::sync::broadcast` channel in `memoryd`. It is not persisted, not exposed over MCP, and not a cross-process API.

| Variant                                                    | Trigger                                                            |
| ---------------------------------------------------------- | ------------------------------------------------------------------ |
| `LeakedSecretDetected { memory_id }`                       | Stream D refuses a secret/high-risk write before disk effects      |
| `BlockingMergeConflict { path }`                           | sync/merge creates a conflict that blocks normal push/merge flow   |
| `ReviewQueueOverThreshold { count, threshold }`            | candidate + quarantined + dream queue exceeds configured threshold |
| `DreamRunCompleted { scope, promoted, queued, dropped }`   | Stream F dream pass finishes with promoted or queued work          |
| `RealityCheckDue { due_at }`                               | weekly schedule is due and not snoozed                             |
| `RealityCheckOverdue { last_completed_at, weeks_skipped }` | missed-week threshold crossing at 3, 6, or 12 weeks                |
| `DailySynthesisSummaryReady { scope }`                     | daily dream/governance summary is ready for external notification  |

Dispatch path: broadcast event -> passive queue always -> optional OS notification -> optional Slack/email. Slack/email payloads contain counts and invocation instructions only, never memory titles, bodies, or entity names.

## Trust artifact

TUI detail modals and web `GET /api/audit/:id` render the same trust artifact. The web route returns the audit object fields at top level:

```json
{
  "memory_id": "mem_20260501_a1b2c3d4e5f60718_000010",
  "title": "Task 14 audit fixture",
  "body": "Task 14 audit-only fixture body",
  "status": "active",
  "namespace": "project:agent-memory",
  "confidence": 0.95,
  "confidence_reason": "deterministic web fallback fixture",
  "recall_count_total": 28,
  "recall_count_30d": 12,
  "last_recalled": "2026-05-01T12:00:00Z",
  "provenance_chain": [],
  "policy_decisions": [],
  "privacy_scan": { "labels_detected": [], "storage_action": "plaintext" },
  "supersession_history": [],
  "sync_state": {
    "devices": ["macbook", "desktop"],
    "merge_status": "clean",
    "claim_lock_status": "Stream I not active"
  }
}
```

`provenance_chain`, `policy_decisions`, `privacy_scan`, and `sync_state` use the shipped `memoryd::trust_artifact` DTO field names. `supersession_history` flattens `supersedes` and `superseded_by` links into entries with `direction`, `memory_id`, `at`, and safe `title`. `recall_count_30d` and `last_recalled` are derived from `events_log` using the covering index; they are not `memories` columns.

Encrypted memories show a body/title redaction notice in UI surfaces. Index-visible metadata and score components may still render; confirm/correct require explicit reveal outside the UI path.

## Slash command

Tier 1 harnesses may expose:

```text
/memory-reality-check
```

It shells to `memoryd reality-check run --json` and formats a human list:

```text
Memorum Reality Check
1. [0.82] me/identity — My preferred stack is TypeScript + Rust
2. [0.71] project:atlasos — atlasos uses Postgres 15 with CITEXT extensions
```

Encrypted items render as `[encrypted item, score: X.XX]`. If no work is pending, output is `No Reality Check items pending.` The slash command emits no raw memory bodies and adds no MCP tool.
