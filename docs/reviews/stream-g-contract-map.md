# Stream G Contract Map

Task 1 only. This file maps `docs/specs/stream-g-observability-v0.1.md` §10 acceptance bullets and §1.3 cross-stream additions to the implementation plan in `docs/plans/2026-05-01-stream-g-observability.md`.

## TDD slice evidence

RED check, before creating this file:

```bash
$ test -f docs/reviews/stream-g-contract-map.md && rg -n "^# Stream G Contract Map" docs/reviews/stream-g-contract-map.md
# exit 1: file did not exist yet
```

GREEN check is recorded after file creation in the final verification section.

## Worktree baseline evidence

Captured before writing this contract map.

```bash
$ git status --short
 M CLAUDE.md
?? docs/plans/2026-05-01-stream-g-observability.md
?? docs/plans/2026-05-01-stream-h-eval-harness.md
?? docs/plans/2026-05-01-stream-i-cross-session.md
?? docs/reviews/stream-g-spec-review.md
?? docs/reviews/stream-ghi-combined-plan-review-pass-2.md
?? docs/reviews/stream-ghi-combined-plan-review.md
?? docs/reviews/stream-h-spec-review.md
?? docs/reviews/stream-i-plan-review.md
?? docs/reviews/stream-i-spec-review.md
?? docs/reviews/system-v0.2-spec-review.md
?? docs/specs/stream-g-observability-v0.1.md
?? docs/specs/stream-h-eval-harness-v0.1.md
?? docs/specs/stream-i-cross-session-v0.1.md
?? docs/specs/system-v0.2.md
?? thoughts/shared/handoffs/dd477f42/
```

```bash
$ git log --oneline -5
cb790ce fix stream f review blockers
4f96dfb Ship Stream F dreaming
e8c07bb Harden recall project binding
247c168 Fix Stream E recall review findings
85c0783 Implement Stream E passive recall
```

## Spec key-term evidence

```bash
$ rg -n "RecallHit|NotificationEvent|RealityCheckRequest|RealityCheckResponse|state\.json|reality-check-pending|reality-check-session|covering index|memoryd-tui|memoryd-web|drift.*score|score.*formula|CSRF|broadcast" docs/specs/stream-g-observability-v0.1.md
18:- New crate `crates/memoryd-tui/` — TUI binary and rendering engine.
19:- New crate `crates/memoryd-web/` — embedded HTTP server for the localhost dashboard.
46:   - `RecallHit { id: MemoryId, recalled_at: DateTime<Utc> }` — emitted by Stream E's recall path (see #5 below); source of `recall_count_30d` for drift scoring.
69:   **Mirror staleness must be observable.** The dual-write fail-soft mode (JSONL succeeds, SQLite write fails, WARN logged) leaves the SQLite mirror behind silently — drift scores computed against a stale mirror are wrong without warning. Stream A exposes `Substrate::events_log_mirror_health() -> EventsLogMirrorHealth { jsonl_max_seq: u64, sqlite_max_seq: u64, lag: u64 }`. The daemon's `doctor_response` calls it and emits a `DoctorFinding { code: "events_log_mirror_lag", repair: Some("memoryd doctor --reindex") }` whenever `lag > 0`, setting `healthy = false`. Without this surfacing, dual-write divergence is undetectable until a user notices wrong scores.
71:3. **`memory_supersession` SQLite derived projection** — supersession relationships in shipped Stream A live only in `Frontmatter.supersedes: Vec<MemoryId>` (and `superseded_by`); the substrate's index does not project them into a queryable table (the project's `sync_auxiliary_tables` doc-comment lists `memory_supersession` as deferred). Stream G's drift-score `cross_source_corroboration` formula needs to walk supersession chains in SQL, so v0.2 promotes this from deferred to shipped. Schema (added in migration v4):
89:5. **`RecallIndexRow` struct field surfacing** (`crates/memory-substrate/src/model.rs`): `indexed_at: DateTime<Utc>` (already a NOT NULL column on `memories`) and `source_device: Option<String>` (already a TEXT NULL column). Pure struct/hydration surface change, no new columns. Stream G's drift-score data path uses `indexed_at` for ordering; Stream I uses both for cross-device peer-update filtering.
91:6. **Daemon state files in the runtime layout** (`stream-a-core-substrate-v1.1.md` §5.2, additive entries): `<runtime_root>/state/state.json`, `<runtime_root>/state/reality-check-pending.json`, `<runtime_root>/state/reality-check-session.json`. All three are per-device, not synced (excluded from git via `.gitignore` patterns under `state/`). Crash-recovery semantics are specified in §5.8 below.
99:9. **`NotificationEvent` broadcast channel.** Stream G defines exactly **seven** variants on a `tokio::sync::broadcast` channel internal to `memoryd` (not persisted, not MCP-exposed, not crossing process boundaries):
102:pub enum NotificationEvent {
117:10. **Recall response builder emits `EventKind::RecallHit` for each memory included in a rendered startup or delta block.** One event per included memory per response, deduplicated within a single response (a memory cited twice in one block produces one event). This is the emission point for the events-log data Stream G's drift score consumes. Owned by Stream E's recall module (`crates/memoryd/src/recall/`); Stream G must not write the emission code itself — it consumes the event stream.
146:  memoryd-tui/
173:  memoryd-web/
191:      auth.rs              # CSRF token; see §4.4
195:      csrf.rs              # CSRF enforcement tests
200:`memoryd` workspace `Cargo.toml` gains two optional bin features: `memoryd-tui` and `memoryd-web`, compiled into the main `memoryd` binary when their respective `[features]` are enabled (which they are by default). The TUI launches as a subprocess spawned by `memoryd ui`; the web server runs as a Tokio task inside the daemon process when enabled.
611:**Decision: Preact + HTM + vanilla CSS, bundled to a single `app.js` + `style.css` at build time via `esbuild`.** The dashboard assets are embedded into the `memoryd-web` binary via `include_bytes!` / `rust-embed`. No runtime asset serving from disk; no CDN; no external network requests.
680:**JSON shapes** (abbreviated; full types in `crates/memoryd-web/src/routes/*.rs`):
759:**CSRF protection:** required because a malicious page loaded in the browser could POST to `localhost:7137/api/review/action` via fetch or form submission. Mitigation:
761:1. On server start, generate a random 32-byte CSRF token and store it in memory.
763:3. All mutating POST routes require the header `X-Memorum-CSRF: <token>`. Requests missing or with incorrect token return `403 Forbidden`.
864:**Derived at score time via SQL against the `events_log` SQLite table** (the v0.2 mirror added in §1.3 #2; canonical store remains per-device JSONL). The covering index `events_log(kind, memory_id, ts)` keeps the per-memory query sub-millisecond. `max_recall_30d_active` is the maximum value across all currently `active` memories in scope, computed once per scoring run via a single GROUP BY query. Bounded in `[0, 1]`.
866:If the events log has no `RecallHit` rows for `m` in the last 30 days, `recall_count_30d(m) = 0` and the term `(1 - recall_frequency_norm(m))` contributes the full 0.20 weight (highest possible drift risk from this component) — consistent with "this memory has not been recalled recently, treat as drifting."
912:**Encrypted memories:** scored using index-visible fields only (namespace, timestamps, sensitivity, recall_count from the safe index projection). No body, no title. Shown in the list as `[encrypted — title not available]` with score breakdown. The score is valid; it uses the same formula. The user can `forget` or `skip` them; `confirm` and `correct` require running `memoryd reveal` first.
921:2. "Due" = the configured schedule time has passed since the last completed session (`reality_check.last_completed_at` stored in daemon state file `~/.memoryd/state.json`), and no snooze is active for the current week.
923:   a. Compute drift scores for all scored memories.
924:   b. Store the scored list in `~/.memoryd/reality-check-pending.json` (daemon-local, not in the git tree).
925:   c. Fire `NotificationEvent::RealityCheckDue`.
935:2. Daemon fetches the pre-computed `reality-check-pending.json` (or computes fresh if not cached or >30 minutes old).
936:3. Items are served one by one. State is held in memory; partial sessions are resumable within the same daemon session (stored in `~/.memoryd/reality-check-session.json`).
940:2. Daemon writes `last_completed_at = now` to `~/.memoryd/state.json`.
941:3. Session state file `reality-check-session.json` is deleted.
944:**Abandoned sessions:** if a session is started but not completed (daemon restart, user closes TUI mid-run), `reality-check-session.json` persists. On next daemon start or next `memoryd reality-check run`, the interrupted session is offered for resumption: "Resume previous session (5 of 12 remaining)? [Y/n]". If declined, the session is discarded and a fresh run starts.
975:- Tracked in `reality-check-session.json` as `deferred_this_week: [mem_id, …]`. Deferred items are excluded from the count toward "session complete" for this week.
984:- `NotificationEvent::RealityCheckOverdue` is fired, which adds a higher-priority `<pending-attention>` line and (if configured) sends a Slack/email.
1003:pub enum RealityCheckRequest {
1057:pub enum RealityCheckResponse {
1101:    pub score: f64,                             // 0.0..=1.0, the final drift score
1135:**`<runtime_root>/state/state.json`** — daemon-wide, persists across restarts.
1149:- **Write:** atomic via `tempfile-then-rename` (write to `state.json.tmp`, fsync, rename). Standard pattern from Stream A.
1152:**`<runtime_root>/state/reality-check-pending.json`** — pre-computed top-N pending list.
1163:- **Load on `RealityCheckRequest::Run`:** if `computed_at` is within the last 30 minutes, reuse. Otherwise recompute and overwrite. If parse fails, treat as missing and recompute.
1165:- **Cleanup:** deleted on session completion (alongside the session file). Reset by `RealityCheckRequest::Reset`.
1167:**`<runtime_root>/state/reality-check-session.json`** — in-flight session state.
1184:- **Partial-write recovery:** every state mutation writes via `tempfile-then-rename`. A crash mid-write leaves either the prior version or the new version, never a corrupt file. If the file is somehow corrupt (e.g., disk full mid-fsync, manual edit), parse failure is treated as no-session-in-progress and the file is renamed to `reality-check-session.json.corrupt-<timestamp>` for forensics; the user starts fresh.
1233:The dispatcher is a Tokio task spawned inside `memoryd` at startup. It subscribes to the `tokio::sync::broadcast::Receiver<NotificationEvent>` channel (§1.3).
1237:    mut events: broadcast::Receiver<NotificationEvent>,
1243:            Err(broadcast::error::RecvError::Lagged(n)) => {
1247:            Err(broadcast::error::RecvError::Closed) => break,
1321:| Recall count (total, 30d) | Derived from events log: `SELECT COUNT(*) FROM events_log WHERE kind='recall_hit' AND memory_id=?` (total) and `... AND ts > now-30d`. Uses the covering index added in §1.3 #2. | Stream A events log + covering index |
1322:| Last recalled timestamp | Derived from events log: `SELECT MAX(ts) FROM events_log WHERE kind='recall_hit' AND memory_id=?`. Returns NULL if never recalled. | Stream A events log + covering index |
1671:- `test_reality_check_panel_renders_score_breakdown`: mock scored item with known weights; assert breakdown math matches formula output.
1706:- `test_post_without_csrf_header_returns_403`: send POST to `/api/review/action` without `X-Memorum-CSRF` header; assert 403.
1719:- `test_score_formula_staleness_only`: memory with staleness 90 days, perfect recall/corroboration/confidence/sensitivity=0; assert score ≈ 0.35.
1720:- `test_score_formula_all_components`: known input for each component; assert final score matches formula to 4 decimal places.
1748:- `test_passive_queue_receives_all_events`: fire each `NotificationEvent` variant; assert passive queue has one entry per event.
1755:- `test_lagged_dispatcher_logs_warning_and_continues`: fill broadcast channel beyond capacity; assert WARN log emitted, dispatcher continues.
1839:| Session resume from persisted state | ≤100 ms | Measures deserialization of `reality-check-session.json` |
1841:**Implementation note:** the scoring loop must avoid calling `Substrate::read_memory` per item. All scoring inputs (`observed_at`, `confidence`, `sensitivity`) come from `memories` index columns; `recall_count_30d` and `distinct_sources` come from the events_log table via the covering index added in §1.3 #2 (`events_log(kind, memory_id, ts)`). For 10,000 memories, this is 1 GROUP-BY-and-FILTER scan over the events log per metric (2 scans total) plus one row-level scan over `memories` for the static fields — well within the 500 ms budget. **Pre-aggregation is acceptable but not required**: a per-day rollup table keyed by `memory_id` could further speed scoring if dogfood reveals the events-log scan dominates, but is not part of v1's spec contract.
1843:No new column is added to the `memories` table. (Earlier draft proposed `source_count INTEGER NOT NULL DEFAULT 1` — dropped per system-v0.2 §19's authorization table; the events log + covering index supplies the same data without duplicating it.)
```

## Current code choke-point evidence

Exact command:

```bash
$ rg -n "RequestPayload|ResponsePayload|StatusResponse|EventKind|recall_hit|RecallStatusCounters|NotificationEvent|RealityCheck" crates
```

The command produced 398 matching lines. Representative choke points that future implementation tasks will touch or review:

```text
crates/memoryd/src/mcp.rs:11:    default_observe_cwd, default_observe_harness, default_observe_session_id, ObserveKind, RequestPayload,
crates/memoryd/src/main.rs:17:use memoryd::protocol::{DreamRunReport, PassStatus, RequestPayload, ResponsePayload, ResponseResult};
crates/memoryd/src/handlers.rs:32:    RequestPayload, ResponseEnvelope, ResponsePayload, RevealResponse, ReviewDecisionResponse, ReviewQueueItemResponse,
crates/memoryd/src/handlers.rs:88:    request: RequestPayload,
crates/memoryd/src/handlers.rs:91:        RequestPayload::Status => Ok(ResponsePayload::Status(status_response(state))),
crates/memoryd/src/handlers.rs:127:fn status_response(state: &HandlerState) -> StatusResponse {
crates/memoryd/src/protocol.rs:8:use crate::recall::{DeltaRequest, DeltaResponse, RecallStatusCounters, StartupRequest, StartupResponse};
crates/memoryd/src/protocol.rs:39:pub enum RequestPayload {
crates/memoryd/src/protocol.rs:157:pub enum ResponsePayload {
crates/memoryd/src/protocol.rs:178:pub struct StatusResponse {
crates/memoryd/src/recall/mod.rs:21:pub use counters::{RecallStatusCounters, SharedRecallCounters};
crates/memory-substrate/src/events/log.rs:56:pub enum EventKind {
crates/memory-substrate/src/api.rs:20:    sync_event_sequence_state, Event, EventKind,
crates/memory-substrate/tests/event_kind_schema.rs:1:use memory_substrate::events::{Event, EventKind};
crates/memory-substrate/tests/memory_query_extension.rs:100:    let recall_hits = substrate
```

## `source_count` cleanliness evidence

```bash
$ rg -n "source_count" docs/specs/stream-g-observability-v0.1.md && echo "FOUND — spec still references dropped column" || echo "clean"
134:- No new columns on the `memories` table beyond the additive nullable `original_confidence REAL` listed in #4. (Earlier draft referenced a `source_count` column — dropped. Cross-source corroboration is derived from the `memory_supersession` join table added in #3 plus the existing `memories.source_harness` column.)
1843:No new column is added to the `memories` table. (Earlier draft proposed `source_count INTEGER NOT NULL DEFAULT 1` — dropped per system-v0.2 §19's authorization table; the events log + covering index supplies the same data without duplicating it.)
FOUND — spec still references dropped column
```

Interpretation: the exact Task 1 command returns `FOUND` because the spec contains two historical references explaining that the column was dropped. Both references state that no `source_count` column is added. This is a plan/spec note to preserve for the orchestrator; it does not require editing the plan in Task 1.

## §1.3 cross-stream additions map

| §1.3 addition                                                                                                                                                                                                                               |                                                       Plan task | Owned implementation files                                                                                                                                                                                                                                                    | Narrow gate(s)                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------: | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `EventKind` variants: `RecallHit`, `RealityCheckConfirmed`, `RealityCheckForgotten`, `RealityCheckNotRelevant`; plus plan-coordinated `ClaimLockContention` in the same file                                                                |                                                          Task 2 | `crates/memory-substrate/src/events/log.rs`; `crates/memory-substrate/tests/event_kind_new_variants.rs`; docs in `docs/api/stream-a-public-api.md`                                                                                                                            | `cargo test -p memory-substrate --test event_kind_new_variants`; secondary `cargo test -p memory-substrate --test event_kind_schema`                                                     |
| `events_log` SQLite mirror, JSONL canonical, covering index `(kind, memory_id, ts)`, mirror-health helper                                                                                                                                   |                                 Task 2; doctor surfacing Task 4 | `crates/memory-substrate/src/index/{migrations.rs,schema.rs,query.rs}`; `crates/memory-substrate/src/api.rs`; `crates/memory-substrate/tests/{events_log_mirror.rs,migration_v4.rs}`; Task 4 `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/doctor_mirror_health.rs` | `cargo test -p memory-substrate --test events_log_mirror`; `cargo test -p memory-substrate --test migration_v4`; `cargo test -p memoryd --test doctor_mirror_health`                     |
| `memory_supersession(memory_id, supersedes_id)` derived projection and recursive-chain primitive                                                                                                                                            |                                      Task 2; consumed by Task 6 | `crates/memory-substrate/src/index/{migrations.rs,schema.rs,query.rs}`; `crates/memory-substrate/tests/memory_supersession_projection.rs`; `crates/memoryd/src/reality_check/scoring.rs`; `crates/memoryd/tests/scoring.rs`                                                   | `cargo test -p memory-substrate --test memory_supersession_projection`; `cargo test -p memoryd --test scoring`                                                                           |
| `Frontmatter::original_confidence: Option<f64>` and `memories.original_confidence REAL`                                                                                                                                                     |                                      Task 2; consumed by Task 6 | `crates/memory-substrate/src/model.rs`; `crates/memory-substrate/src/index/{migrations.rs,query.rs,schema.rs}`; `crates/memory-substrate/tests/frontmatter_original_confidence.rs`; `crates/memoryd/tests/scoring.rs`                                                         | `cargo test -p memory-substrate --test frontmatter_original_confidence`; `cargo test -p memory-substrate --test migration_v4`; `cargo test -p memoryd --test scoring`                    |
| `RecallIndexRow` surface adds `indexed_at` and `source_device`                                                                                                                                                                              |                        Task 2; consumed by Tasks 6 and Stream I | `crates/memory-substrate/src/model.rs`; query hydration in `crates/memory-substrate/src/index/query.rs`; docs in `docs/api/stream-a-public-api.md`                                                                                                                            | Task 2 substrate test set; Task 6 `cargo test -p memoryd --test scoring`                                                                                                                 |
| Daemon state files: `state.json`, `reality-check-pending.json`, `reality-check-session.json`; runtime-local, crash-recoverable                                                                                                              |                          Task 4; consumed by Task 7 and Task 16 | `crates/memoryd/src/state.rs`; `crates/memoryd/src/main.rs`; `crates/memoryd/tests/daemon_state_files.rs`; later `crates/memoryd/src/reality_check/{session.rs,scheduling.rs}`                                                                                                | `cargo test -p memoryd --test daemon_state_files`; `cargo test -p memoryd --test scheduling`; `cargo test -p memoryd --test responses`                                                   |
| Reality Check daemon protocol: `RealityCheckRequest`, `RealityCheckResponse`, actions/items/component scores                                                                                                                                | Task 5; handler semantics Task 7; CLI/web consumers Tasks 14/16 | `crates/memoryd/src/protocol.rs`; `crates/memoryd/src/client.rs`; `crates/memoryd/tests/protocol_contract.rs`; `crates/memoryd/src/handlers.rs`; web route files; CLI files                                                                                                   | `cargo test -p memoryd --test protocol_contract`; `cargo test -p memoryd --test responses`; `cargo test -p memoryd-web --test api_contract`; `cargo test -p memoryd --test cli_contract` |
| `MethodNotAllowedOnMcp` protocol error for admin/UI variants on MCP forwarder                                                                                                                                                               |                                                          Task 5 | `crates/memoryd/src/protocol.rs`; `crates/memoryd/src/mcp.rs`; `crates/memoryd/tests/notification_channel.rs`; `crates/memoryd/tests/protocol_contract.rs`                                                                                                                    | `cargo test -p memoryd --test notification_channel`; `cargo test -p memoryd --test protocol_contract`; secondary `cargo test -p memoryd --test mcp_manifest`                             |
| `NotificationEvent` broadcast channel with exactly seven variants: `LeakedSecretDetected`, `BlockingMergeConflict`, `ReviewQueueOverThreshold`, `DreamRunCompleted`, `RealityCheckDue`, `RealityCheckOverdue`, `DailySynthesisSummaryReady` |                                       Task 5; dispatcher Task 8 | `crates/memoryd/src/protocol.rs`; `crates/memoryd/tests/notification_channel.rs`; `crates/memoryd/src/notifications/*`; `crates/memoryd/tests/dispatcher.rs`                                                                                                                  | `cargo test -p memoryd --test notification_channel`; `cargo test -p memoryd --test dispatcher`                                                                                           |
| Stream E recall response builder emits `EventKind::RecallHit` once per included memory per startup/delta response                                                                                                                           |                                                          Task 3 | `crates/memoryd/src/recall/startup.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/recall_hit_emission.rs`                                                                                                                                                  | `cargo test -p memoryd --test recall_hit_emission`; secondary `cargo test -p memoryd --test startup_recall_mcp`; `cargo test -p memoryd --test startup_recall_determinism`               |
| Stream E `<pending-attention>` item `kind="reality_check_due"`, fixed text, weekly cap/snooze semantics                                                                                                                                     |                                                          Task 9 | `crates/memoryd/src/recall/startup.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/reality_check_pending_attention.rs`; docs in Task 18                                                                                                                     | `cargo test -p memoryd --test reality_check_pending_attention`; secondary `cargo test -p memoryd --test startup_recall_determinism --test startup_recall_privacy`                        |

## §10 acceptance bullet map

### §10.1 TUI tests

| Acceptance bullet                                     | Plan task | Owned files                                                                           | Narrow gate                                           |
| ----------------------------------------------------- | --------: | ------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `test_overview_panel_renders_daemon_status`           |   Task 10 | `crates/memoryd-tui/src/app.rs`, `panels/overview.rs`, `tests/panel_render.rs`        | `cargo test -p memoryd-tui --test panel_render`       |
| `test_review_queue_renders_candidate_items`           |   Task 10 | `panels/review_queue.rs`, `tests/panel_render.rs`                                     | `cargo test -p memoryd-tui --test panel_render`       |
| `test_review_queue_renders_dream_low_confidence`      |   Task 10 | `panels/review_queue.rs`, `tests/panel_render.rs`                                     | `cargo test -p memoryd-tui --test panel_render`       |
| `test_conflicts_panel_renders_side_by_side`           |   Task 10 | `panels/conflicts.rs`, `tests/panel_render.rs`                                        | `cargo test -p memoryd-tui --test panel_render`       |
| `test_entities_panel_search_renders_results`          |   Task 10 | `panels/entities.rs`, `tests/panel_render.rs`                                         | `cargo test -p memoryd-tui --test panel_render`       |
| `test_timeline_panel_renders_events_by_kind`          |   Task 10 | `panels/timeline.rs`, `tests/panel_render.rs`; depends on Task 2 `EventKind` variants | `cargo test -p memoryd-tui --test panel_render`       |
| `test_namespace_tree_renders_hierarchy`               |   Task 10 | `panels/namespace.rs`, `tests/panel_render.rs`                                        | `cargo test -p memoryd-tui --test panel_render`       |
| `test_policy_panel_renders_active_policies`           |   Task 10 | `panels/policy.rs`, `tests/panel_render.rs`                                           | `cargo test -p memoryd-tui --test panel_render`       |
| `test_reality_check_panel_renders_score_breakdown`    |   Task 10 | `panels/reality_check.rs`, `tests/panel_render.rs`; consumes Task 6 score DTOs        | `cargo test -p memoryd-tui --test panel_render`       |
| `test_all_panels_handle_panel_switch_keys`            |   Task 11 | `crates/memoryd-tui/src/app.rs`, `tests/keymap.rs`                                    | `cargo test -p memoryd-tui --test keymap`             |
| `test_quit_with_pending_actions_prompts_confirmation` |   Task 11 | `app.rs`, `panels/review_queue.rs`, `tests/keymap.rs`                                 | `cargo test -p memoryd-tui --test keymap`             |
| `test_escape_closes_modal`                            |   Task 11 | `app.rs`, `tests/keymap.rs`                                                           | `cargo test -p memoryd-tui --test keymap`             |
| `test_undo_window_fires_before_daemon_call`           |   Task 11 | `app.rs`, `panels/review_queue.rs`, `tests/keymap.rs`                                 | `cargo test -p memoryd-tui --test keymap`             |
| `test_undo_window_expires_and_fires_daemon_call`      |   Task 11 | `app.rs`, `panels/review_queue.rs`, `tests/keymap.rs`                                 | `cargo test -p memoryd-tui --test keymap`             |
| `test_tui_shows_unreachable_state_on_socket_failure`  |   Task 10 | `app.rs`, `client.rs`, `tests/socket_unreachable.rs`                                  | `cargo test -p memoryd-tui --test socket_unreachable` |
| `test_tui_recovers_on_reconnection`                   |   Task 10 | `app.rs`, `client.rs`, `tests/socket_unreachable.rs`                                  | `cargo test -p memoryd-tui --test socket_unreachable` |
| `test_below_minimum_shows_warning_banner`             |   Task 10 | `app.rs`, `tests/resize.rs`                                                           | `cargo test -p memoryd-tui --test resize`             |
| `test_resize_above_minimum_resumes`                   |   Task 10 | `app.rs`, `tests/resize.rs`                                                           | `cargo test -p memoryd-tui --test resize`             |

### §10.2 Web dashboard tests

| Acceptance bullet                                     |                        Plan task | Owned files                                                                        | Narrow gate                                                                                    |
| ----------------------------------------------------- | -------------------------------: | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `test_get_status_returns_correct_shape`               |                          Task 14 | `crates/memoryd-web/src/routes/status.rs`, `tests/api_contract.rs`                 | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_get_entity_graph_returns_nodes_and_edges`       |                          Task 14 | `routes/entity_graph.rs`, `tests/api_contract.rs`                                  | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_post_review_action_approve_calls_daemon`        |                          Task 14 | `routes/review.rs`, `tests/api_contract.rs`                                        | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_post_review_action_returns_409_on_wrong_state`  |                          Task 14 | `routes/review.rs`, `tests/api_contract.rs`                                        | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_get_audit_returns_full_trust_artifact`          |       Task 14; trust DTO Task 12 | `routes/audit.rs`, `crates/memoryd/src/trust_artifact.rs`, `tests/api_contract.rs` | `cargo test -p memoryd-web --test api_contract`; `cargo test -p memoryd --test trust_artifact` |
| `test_get_audit_temporal_returns_historical_state`    |                          Task 14 | `routes/audit.rs`, `tests/api_contract.rs`                                         | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_get_roi_30d_returns_correct_window`             |                          Task 14 | `routes/roi.rs`, `tests/api_contract.rs`                                           | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_get_roi_365d_returns_correct_window`            |                          Task 14 | `routes/roi.rs`, `tests/api_contract.rs`                                           | `cargo test -p memoryd-web --test api_contract`                                                |
| `test_post_without_csrf_header_returns_403`           |                          Task 13 | `crates/memoryd-web/src/auth.rs`, `server.rs`, `tests/csrf.rs`                     | `cargo test -p memoryd-web --test csrf`                                                        |
| `test_post_with_wrong_csrf_token_returns_403`         |                          Task 13 | `auth.rs`, `tests/csrf.rs`                                                         | `cargo test -p memoryd-web --test csrf`                                                        |
| `test_post_with_correct_csrf_token_succeeds`          |                          Task 13 | `auth.rs`, `routes/mod.rs`, `tests/csrf.rs`                                        | `cargo test -p memoryd-web --test csrf`                                                        |
| `test_csrf_token_in_initial_html`                     |                          Task 13 | `auth.rs`, `static/index.html`, `tests/csrf.rs`                                    | `cargo test -p memoryd-web --test csrf`                                                        |
| `test_concurrent_post_same_memory_second_returns_409` | Task 13; route semantics Task 14 | `server.rs`, `routes/review.rs`, `tests/concurrent_access.rs`                      | `cargo test -p memoryd-web --test concurrent_access`                                           |

### §10.3 Reality Check tests

| Acceptance bullet                                         | Plan task | Owned files                                                                                                               | Narrow gate                               |
| --------------------------------------------------------- | --------: | ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `test_score_formula_staleness_only`                       |    Task 6 | `crates/memoryd/src/reality_check/scoring.rs`, `types.rs`, `tests/scoring.rs`                                             | `cargo test -p memoryd --test scoring`    |
| `test_score_formula_all_components`                       |    Task 6 | `scoring.rs`, `tests/scoring.rs`                                                                                          | `cargo test -p memoryd --test scoring`    |
| `test_score_saturation_at_90_days`                        |    Task 6 | `scoring.rs`, `tests/scoring.rs`                                                                                          | `cargo test -p memoryd --test scoring`    |
| `test_corroboration_requires_two_distinct_sources`        |    Task 6 | `scoring.rs`; depends on Task 2 `memory_supersession`                                                                     | `cargo test -p memoryd --test scoring`    |
| `test_sensitivity_weights_map_correctly`                  |    Task 6 | `types.rs`, `scoring.rs`, `tests/scoring.rs`                                                                              | `cargo test -p memoryd --test scoring`    |
| `test_encrypted_memory_scored_from_index_only`            |    Task 6 | `scoring.rs`, `tests/scoring.rs`                                                                                          | `cargo test -p memoryd --test scoring`    |
| `test_top_n_selection_respects_cap`                       |    Task 6 | `scoring.rs`, `tests/scoring.rs`                                                                                          | `cargo test -p memoryd --test scoring`    |
| `test_pinned_memories_always_included`                    |    Task 6 | `scoring.rs`, `tests/scoring.rs`                                                                                          | `cargo test -p memoryd --test scoring`    |
| `test_due_after_7_days`                                   |    Task 7 | `crates/memoryd/src/reality_check/scheduling.rs`, `tests/scheduling.rs`                                                   | `cargo test -p memoryd --test scheduling` |
| `test_not_due_within_7_days`                              |    Task 7 | `scheduling.rs`, `tests/scheduling.rs`                                                                                    | `cargo test -p memoryd --test scheduling` |
| `test_snoozed_not_due`                                    |    Task 7 | `scheduling.rs`, `tests/scheduling.rs`; state from Task 4                                                                 | `cargo test -p memoryd --test scheduling` |
| `test_overdue_after_21_days`                              |    Task 7 | `scheduling.rs`, `tests/scheduling.rs`                                                                                    | `cargo test -p memoryd --test scheduling` |
| `test_confirm_updates_observed_at_and_bumps_confidence`   |    Task 7 | `crates/memoryd/src/reality_check/session.rs`, `crates/memoryd/src/handlers.rs`, `tests/responses.rs`; events from Task 2 | `cargo test -p memoryd --test responses`  |
| `test_not_relevant_sets_passive_recall_false`             |    Task 7 | `session.rs`, `handlers.rs`, `tests/responses.rs`                                                                         | `cargo test -p memoryd --test responses`  |
| `test_not_relevant_does_not_tombstone`                    |    Task 7 | `session.rs`, `handlers.rs`, `tests/responses.rs`                                                                         | `cargo test -p memoryd --test responses`  |
| `test_forget_requires_reason_minimum_length`              |    Task 7 | `session.rs`, `handlers.rs`, `tests/responses.rs`                                                                         | `cargo test -p memoryd --test responses`  |
| `test_correct_issues_supersession`                        |    Task 7 | `session.rs`, `handlers.rs`, `tests/responses.rs`                                                                         | `cargo test -p memoryd --test responses`  |
| `test_skip_this_week_defers_without_frontmatter_mutation` |    Task 7 | `session.rs`, Task 4 state store, `tests/responses.rs`                                                                    | `cargo test -p memoryd --test responses`  |

### §10.4 Notification tests

| Acceptance bullet                                             | Plan task | Owned files                                                                                                         | Narrow gate                               |
| ------------------------------------------------------------- | --------: | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `test_passive_queue_receives_all_events`                      |    Task 8 | `crates/memoryd/src/notifications/{mod.rs,dispatcher.rs,passive.rs}`, `tests/dispatcher.rs`; event enum from Task 5 | `cargo test -p memoryd --test dispatcher` |
| `test_passive_queue_drops_oldest_when_full`                   |    Task 8 | `notifications/passive.rs`, `tests/dispatcher.rs`                                                                   | `cargo test -p memoryd --test dispatcher` |
| `test_os_notification_not_fired_when_disabled`                |    Task 8 | `notifications/{config.rs,os.rs,dispatcher.rs}`, `tests/dispatcher.rs`                                              | `cargo test -p memoryd --test dispatcher` |
| `test_os_notification_fires_when_enabled_and_trigger_matches` |    Task 8 | `notifications/os.rs`, `dispatcher.rs`, `tests/dispatcher.rs`                                                       | `cargo test -p memoryd --test dispatcher` |
| `test_slack_webhook_retried_on_failure`                       |    Task 8 | `notifications/external.rs`, `tests/dispatcher.rs`                                                                  | `cargo test -p memoryd --test dispatcher` |
| `test_slack_webhook_falls_back_to_passive_on_final_failure`   |    Task 8 | `notifications/{external.rs,passive.rs}`, `tests/dispatcher.rs`                                                     | `cargo test -p memoryd --test dispatcher` |
| `test_slack_payload_contains_no_memory_content`               |    Task 8 | `notifications/external.rs`, `tests/dispatcher.rs`                                                                  | `cargo test -p memoryd --test dispatcher` |
| `test_lagged_dispatcher_logs_warning_and_continues`           |    Task 8 | `notifications/dispatcher.rs`, `tests/dispatcher.rs`                                                                | `cargo test -p memoryd --test dispatcher` |

### §10.5 Trust artifact tests

| Acceptance bullet                                | Plan task | Owned files                                                                                                                          | Narrow gate                                   |
| ------------------------------------------------ | --------: | ------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------- |
| `test_all_sections_present_for_plaintext_memory` |   Task 12 | `crates/memoryd/src/trust_artifact.rs`; `crates/memoryd-tui/src/widgets/trust_artifact.rs`; `crates/memoryd/tests/trust_artifact.rs` | `cargo test -p memoryd --test trust_artifact` |
| `test_encrypted_memory_shows_content_redacted`   |   Task 12 | `trust_artifact.rs`, TUI widget files, `tests/trust_artifact.rs`                                                                     | `cargo test -p memoryd --test trust_artifact` |
| `test_provenance_chain_correctly_ordered`        |   Task 12 | `trust_artifact.rs`, `tests/trust_artifact.rs`; events from Task 2                                                                   | `cargo test -p memoryd --test trust_artifact` |
| `test_policy_decision_expands_all_fields`        |   Task 12 | `trust_artifact.rs`, `tests/trust_artifact.rs`; Stream C governance DTOs                                                             | `cargo test -p memoryd --test trust_artifact` |

## Acceptance coverage by implementation phase

- Cross-stream substrate and recall prerequisites: Tasks 2-3, Review Gate A.
- Daemon state, protocol, scoring, and session lifecycle: Tasks 4-7, Review Gate B.
- Notifications, pending-attention, TUI, and trust artifact rendering: Tasks 8-12, Review Gate C.
- Web, slash command, and CLI surfaces: Tasks 13-16, Review Gate D.
- Performance, docs, and final release evidence: Tasks 17-19, Final Review Gate E.

## Plan/spec notes for orchestrator

- The plan source-contract note says the system spec row previously described `NotificationEvent` as six variants; Stream G §1.3 requires seven. Task 5 and Task 8 must preserve seven variants exactly.
- Task 1's `source_count` command returns `FOUND` because §1.3 and §12.3 contain historical dropped-column notes. The implementation invariant remains: do not add a `source_count` column; derive cross-source corroboration from `memory_supersession` plus `memories.source_harness`.
- Stream G does not add MCP tools. Reality Check is admin/UI protocol only and must be rejected through MCP with `MethodNotAllowedOnMcp`.

## Final verification section

To run after writing this file:

````bash
test -f docs/reviews/stream-g-contract-map.md && rg -n "^# Stream G Contract Map" docs/reviews/stream-g-contract-map.md```

Expected: the heading check returns the top-level heading. The secondary blocker-word scan was run separately and returned no matches.

## Unresolved blockers

None.
````
