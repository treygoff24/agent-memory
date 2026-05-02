# Stream G Observability Spec v0.1

**Status:** implementation contract for Stream G observability surfaces.
**Date:** 2026-05-01.
**Sources:** `docs/specs/system-v0.2.md` §16 (observability), §14.3 (admin CLI), §10 (harness tiers), §22 (product name); `docs/specs/stream-f-dreaming-v0.2.md` §1–§3 (conventions and cross-stream surface model); `docs/specs/stream-e-passive-recall-v0.5.md` §1–§5 (`<pending-attention>` XML shape, recall block format); `docs/api/stream-c-governance-api.md` (review queue and policy surfaces consumed by Stream G UI).
**Non-source:** older drafts, `docs/handoff-2026-04-23.md`, and the handbook are background; not normative.

## Revision goal

v0.1: Initial Stream G contract for v1 release. Defines all observability-facing user surfaces: TUI (8-panel `ratatui`), localhost web dashboard (4 sections), Reality Check ritual, notification dispatcher, trust artifact rendering, and CLI command additions. Stream G is a consumer of shipped Streams A–F; it does not mutate canonical memory state, does not add MCP tools, and does not modify governance or privacy logic.

v0.1 (micro-clarifications, 2026-05-02): Four implementation-accuracy fixes from the post-12-hour-run fresh-eyes review — no contract changes, only documentation alignment and deferred-list updates:
1. §4.4 bind_address clarification: explicitly documents `::1` as a valid value alongside `127.0.0.1`; the §8 config block already listed both but §4.4 prose was underspecified.
2. §11.8 (new): `GET /api/audit/:id/walk` (audit_walk) is formally moved to the v1.1+ deferred list. The route was shipped as a 501 stub by the Codex 12-hour run; the 501 is correct behavior — the route is now explicitly deferred rather than silently stubbed.
3. §12.1 (annotation added): The TUI benchmark entries `tui_panel_switch` and `tui_detail_modal_open` in the canonical baseline use synthetic 144-byte frames, not a full ratatui render path. Real-load TUI bench (full terminal-emulator integration) is deferred to v1.1+. The synthetic measurements remain valid as smoke-test regression detectors for the in-process render code path.
4. §5.5 / §5.2 clarification: `is_overdue` must treat `None` (never-completed) as overdue, mirroring `is_due`'s `is_none_or` semantics. Aligns implementation with spec intent.

---

## 1. Scope and dependency boundaries

### 1.1 Stream G owns

- New crate `crates/memoryd-tui/` — TUI binary and rendering engine.
- New crate `crates/memoryd-web/` — embedded HTTP server for the localhost dashboard.
- `memoryd ui` CLI command — launches TUI.
- `memoryd web {enable,disable,status}` CLI commands — controls dashboard lifecycle.
- `memoryd reality-check {run,skip,snooze}` CLI commands — ritual control.
- `/memory-reality-check` slash command for Tier 1 harnesses.
- Reality Check scheduling, drift-risk scoring, and response processing (confirm / correct / forget / not-relevant / skip-this-week).
- Notification dispatcher: passive channel (always-on), OS channel (opt-in), external Slack/email channel.
- Trust artifact rendering — provenance chain, confidence, recall history, policy decisions, privacy scan, supersession history, sync state — in both TUI and web surfaces.
- `config.yaml` additions for TUI, web, reality-check, and notification configuration.

### 1.2 Stream G does not own

- Agent-facing MCP tools. The nine tools are frozen for v1.
- Canonical memory mutation — no writes, supersedes, tombstones, or status changes issued directly from UI rendering code. All mutations route through daemon protocol (governance, privacy, event log).
- Governance logic, policy evaluation, privacy classification, encryption — those remain Streams C, D.
- Recall block assembly — that remains Stream E. The TUI and dashboard query the daemon's existing surfaces.
- Dream scheduling, lease management, Pass 1/2/3 logic — that remains Stream F. Stream G renders the output and review queue.
- Stream I peer-update relevance gate and presence protocol — Stream G reads peer-update state from daemon status.
- Harness hook wiring — Tier 1 hooks (`memoryd recall startup-block`, `memoryd recall delta-block`) are unchanged.

### 1.3 Cross-stream surface changes required by Stream G

All cross-stream additions below are authorized by system-v0.2 §19 ("Cross-stream surface authorizations") as additive-only. Each entry maps to that table; if anything here drifts from the system spec authorization, the system spec is the source of truth and Stream G must be brought back into alignment.

**Stream A surface additions (substrate):**

1. **`EventKind` enum gets four new variants** on `memory_substrate::EventKind` (`crates/memory-substrate/src/events/log.rs`):
   - `RecallHit { id: MemoryId, recalled_at: DateTime<Utc> }` — emitted by Stream E's recall path (see #5 below); source of `recall_count_30d` for drift scoring.
   - `RealityCheckConfirmed { id: MemoryId, session_id: String }` — emitted on user `confirm` action.
   - `RealityCheckForgotten { id: MemoryId, session_id: String, reason: String }` — emitted on user `forget` action (alongside the standard `TombstoneCommitted`).
   - `RealityCheckNotRelevant { id: MemoryId, session_id: String }` — emitted on user `not_relevant` action.

   (Stream I's `EventKind::ClaimLockContention` lands in the same file via Stream G's authorization umbrella; see "Inter-stream coordination" in this stream's plan.)

2. **`events_log` SQLite mirror table** — Stream A's shipped event log is per-device JSONL on disk (`events/<device_id>.jsonl`); v0.2 adds a SQLite mirror table as a **derived projection** so SQL queries can answer "how many recall hits for this memory in the last 30 days?" sub-millisecond. Schema (added in migration v4):

   ```sql
   CREATE TABLE IF NOT EXISTS events_log (
     seq           INTEGER PRIMARY KEY,
     kind          TEXT NOT NULL,
     memory_id     TEXT,           -- NULL for events not memory-scoped
     ts            TEXT NOT NULL,  -- ISO 8601 UTC
     payload_json  TEXT NOT NULL CHECK (json_valid(payload_json))
   );
   CREATE INDEX IF NOT EXISTS idx_events_log_kind_memory_ts
     ON events_log(kind, memory_id, ts);
   ```

   **Migration v4 also backfills** `events_log` from each device's `events/<device_id>.jsonl` file using existing JSONL reader APIs. **Going forward**, `events::log::append` writes to JSONL first (canonical) and then upserts the SQLite row in the same transaction-bracketed scope used by other index writes; if the SQLite mirror is corrupted or behind, `memoryd doctor --reindex` rebuilds it from JSONL. JSONL is the source of truth; SQLite is rebuildable. Schema-version bump: `INDEX_SUPPORTED_SCHEMA_VERSION` goes from 3 to 4.

   **Mirror staleness must be observable.** The dual-write fail-soft mode (JSONL succeeds, SQLite write fails, WARN logged) leaves the SQLite mirror behind silently — drift scores computed against a stale mirror are wrong without warning. Stream A exposes `Substrate::events_log_mirror_health() -> EventsLogMirrorHealth { jsonl_max_seq: u64, sqlite_max_seq: u64, lag: u64 }`. The daemon's `doctor_response` calls it and emits a `DoctorFinding { code: "events_log_mirror_lag", repair: Some("memoryd doctor --reindex") }` whenever `lag > 0`, setting `healthy = false`. Without this surfacing, dual-write divergence is undetectable until a user notices wrong scores.

3. **`memory_supersession` SQLite derived projection** — supersession relationships in shipped Stream A live only in `Frontmatter.supersedes: Vec<MemoryId>` (and `superseded_by`); the substrate's index does not project them into a queryable table (the project's `sync_auxiliary_tables` doc-comment lists `memory_supersession` as deferred). Stream G's drift-score `cross_source_corroboration` formula needs to walk supersession chains in SQL, so v0.2 promotes this from deferred to shipped. Schema (added in migration v4):

   ```sql
   CREATE TABLE IF NOT EXISTS memory_supersession (
     memory_id     TEXT NOT NULL,
     supersedes_id TEXT NOT NULL,
     PRIMARY KEY(memory_id, supersedes_id),
     FOREIGN KEY(memory_id)     REFERENCES memories(id) ON DELETE CASCADE,
     FOREIGN KEY(supersedes_id) REFERENCES memories(id) ON DELETE CASCADE
   );
   CREATE INDEX IF NOT EXISTS idx_memory_supersession_supersedes_id
     ON memory_supersession(supersedes_id);
   ```

   Migration v4 backfills from each `memories.frontmatter_json` row's `supersedes` array. Going forward, the existing `sync_auxiliary_tables` function (which already wholesale-replaces tags/aliases/entities/evidence per memory write) is extended to also wholesale-replace this memory's supersession edges. Frontmatter remains canonical; the table is rebuildable from frontmatter on `memoryd doctor --reindex`. **There is no `memories.supersedes_ids` column** — references to such a column in earlier drafts were a fiction; the join table replaces it.

4. **`Frontmatter` model field addition** (`crates/memory-substrate/src/model.rs`): `pub original_confidence: Option<f64>`. Set on initial promotion (Stream C governance pipeline) and never mutated thereafter. The `confidence_decay` drift component (§5.1) reads this. Pre-v0.2 memories that lack the field are read as `None`; the formula treats `None` as "decay = 0" (a conservative floor). Index column `original_confidence REAL` added to `memories` in migration v4 via `add_column_if_missing` (matches the existing migration idiom).

5. **`RecallIndexRow` struct field surfacing** (`crates/memory-substrate/src/model.rs`): `indexed_at: DateTime<Utc>` (already a NOT NULL column on `memories`) and `source_device: Option<String>` (already a TEXT NULL column). Pure struct/hydration surface change, no new columns. Stream G's drift-score data path uses `indexed_at` for ordering; Stream I uses both for cross-device peer-update filtering.

6. **Daemon state files in the runtime layout** (`stream-a-core-substrate-v1.1.md` §5.2, additive entries): `<runtime_root>/state/state.json`, `<runtime_root>/state/reality-check-pending.json`, `<runtime_root>/state/reality-check-session.json`. All three are per-device, not synced (excluded from git via `.gitignore` patterns under `state/`). Crash-recovery semantics are specified in §5.8 below.

**Stream B / daemon protocol additions (newline-delimited JSON over Unix socket):**

7. **`RequestPayload` / `ResponsePayload` Reality Check variants.** Wire shapes specified in §5.7. Forward-compatible: existing variants unchanged. MCP forwarder rejects these with `MethodNotAllowedOnMcp` (see #8 below) — Reality Check is admin/UI surface, not agent-facing (per system-v0.2 §14.3).

8. **`MethodNotAllowedOnMcp` error variant** on the daemon's protocol error enum (`crates/memoryd/src/protocol.rs`). Returned by the MCP forwarder when an admin/UI variant is invoked through the MCP tool path. Today the MCP forwarder uses `UnknownToolName` for unrecognized tools but has no rejection path for known-but-admin variants because admin commands are CLI-only by construction. Stream G adds this variant and wires its return for: `RealityCheckRun`/`RealityCheckList`/`RealityCheckRespond`/`RealityCheckSnooze`/`RealityCheckReset`, Stream I's `PeerPresenceHeartbeat`/`PeerClaimAcquire`/`PeerClaimRelease`, and Stream H's `TestInjectEvent` (test-utils-gated). The variant is reused — not per-stream rejection text.

9. **`NotificationEvent` broadcast channel.** Stream G defines exactly **seven** variants on a `tokio::sync::broadcast` channel internal to `memoryd` (not persisted, not MCP-exposed, not crossing process boundaries):

```rust
pub enum NotificationEvent {
    LeakedSecretDetected { memory_id: MemoryId },
    BlockingMergeConflict { path: String },
    ReviewQueueOverThreshold { count: usize, threshold: usize },
    DreamRunCompleted { scope: String, promoted: usize, queued: usize, dropped: usize },
    RealityCheckDue { due_at: DateTime<Utc> },
    RealityCheckOverdue { last_completed_at: Option<DateTime<Utc>>, weeks_skipped: u32 },
    DailySynthesisSummaryReady { scope: String },
}
```

`RealityCheckOverdue` fires once per missed-week threshold crossing (3 weeks, then 6 weeks, then 12 weeks); see §5.5. The dispatcher (§6) subscribes to this channel.

**Stream E surface additions (recall):**

10. **Recall response builder emits `EventKind::RecallHit` for each memory included in a rendered startup or delta block.** One event per included memory per response, deduplicated within a single response (a memory cited twice in one block produces one event). This is the emission point for the events-log data Stream G's drift score consumes. Owned by Stream E's recall module (`crates/memoryd/src/recall/`); Stream G must not write the emission code itself — it consumes the event stream.
11. **`<pending-attention>` notification line** (already documented in v0.1 of this spec):

```
<pending-attention>
  ...existing items...
  <item kind="reality_check_due" count="1">Weekly Reality Check is ready — run `memoryd reality-check run` or open TUI panel 8.</item>
</pending-attention>
```

- Emitted at most once per 7-day window regardless of how many sessions start.
- Suppressed if user has snoozed via `memoryd reality-check snooze` within the current week.
- The item text is a fixed string; it must not contain any memory title or body content (no privacy risk from pending-attention context).
- Stream E's item count caps (2/scope, 6 total per system-v0.2 §12; tightened from v0.1) still apply. The `reality_check_due` item counts against the 6-total cap. If 6 slots are already filled with higher-priority items, the reality-check item is dropped silently (counted in `omitted_count`).

**Surfaces explicitly NOT touched by Stream G:**

- No new columns on the `memories` table beyond the additive nullable `original_confidence REAL` listed in #4. (Earlier draft referenced a `source_count` column — dropped. Cross-source corroboration is derived from the `memory_supersession` join table added in #3 plus the existing `memories.source_harness` column.)
- No changes to `MemoryFrontmatter`, `WriteOptions`, `ClassificationOutcome`, or any agent-facing MCP tool. The MCP surface stays at the nine tools frozen by system-v0.2 §14.1.
- No changes to Stream C governance, Stream D privacy classification/encryption, or Stream F dream pipelines. Stream G reads their outputs (review queue, audit metadata, dream cleanup logs) but writes nothing through them.

---

## 2. Crate layout

Two crates, not one. The TUI and web server have different dependency trees — the TUI pulls in `ratatui`, `crossterm`, and terminal-event machinery; the web server pulls in `axum` and static-asset embedding. Merging them into one crate bloats both binaries and forces conditional compilation gymnastics that hurt readability. The split is cleaner.

```
crates/
  memoryd-tui/
    Cargo.toml
    src/
      main.rs              # binary entry point: `memoryd ui`
      app.rs               # App state, panel enum, event loop
      panels/
        mod.rs
        overview.rs        # Panel 1
        review_queue.rs    # Panel 2
        conflicts.rs       # Panel 3
        entities.rs        # Panel 4
        timeline.rs        # Panel 5
        namespace.rs       # Panel 6
        policy.rs          # Panel 7
        reality_check.rs   # Panel 8
      widgets/
        mod.rs
        trust_artifact.rs  # shared trust artifact renderer
        memory_detail.rs   # full memory view modal
        diff_view.rs       # side-by-side diff for Panel 3
        search_bar.rs      # typeahead / search input
      client.rs            # thin wrapper over memoryd socket client
      config.rs            # reads [ui] section from config.yaml
    tests/
      panel_render.rs      # snapshot tests against sample daemon responses
      keymap.rs            # keymap exhaustiveness checks

  memoryd-web/
    Cargo.toml
    src/
      main.rs              # library entry; started by memoryd daemon on `web enable`
      server.rs            # axum router, port binding, shutdown
      routes/
        mod.rs
        status.rs          # GET /api/status
        entity_graph.rs    # GET /api/entity-graph
        roi.rs             # GET /api/roi
        reality_check.rs   # GET/POST /api/reality-check
        audit.rs           # GET /api/audit/:id, GET /api/audit/:id/walk
        review.rs          # GET/POST /api/review  (queue read + mutating actions)
      static/
        index.html         # single-page shell
        app.js             # bundled; see §4.2 for stack choice
        style.css
        fonts/             # self-hosted, no CDN
      auth.rs              # CSRF token; see §4.4
      config.rs            # reads [web] section from config.yaml
    tests/
      api_contract.rs      # route shape tests
      csrf.rs              # CSRF enforcement tests
```

Both crates depend on `crates/memoryd/` as a library (the daemon protocol client). Neither crate has direct Substrate access — all reads go through the daemon socket. This preserves the single-writer model and ensures every query runs through governance projections.

`memoryd` workspace `Cargo.toml` gains two optional bin features: `memoryd-tui` and `memoryd-web`, compiled into the main `memoryd` binary when their respective `[features]` are enabled (which they are by default). The TUI launches as a subprocess spawned by `memoryd ui`; the web server runs as a Tokio task inside the daemon process when enabled.

---

## 3. TUI architecture

### 3.1 Technology choices

**`ratatui` with `crossterm` backend.** `ratatui` is the maintained successor to `tui-rs`, widely used in the Rust terminal UI space (lazygit, k9s, gitui). `crossterm` provides cross-platform raw-mode terminal I/O. Together they deliver keyboard-first, zero-mouse rendering on any terminal that supports ANSI escape codes. No alternative considered.

**Immediate-mode rendering.** State lives in `App`. Each tick the full frame is redrawn from state. This is standard for `ratatui` and makes the refresh model trivial: any state change triggers a redraw; no diffing or retained-widget graph to maintain.

**Tick rate: 16 ms (≈60 fps) for input processing; 250 ms for daemon poll.** Keyboard events are processed every tick. Daemon state (review queue, timeline, status) is polled every 250 ms via the Unix socket. This decouples rendering from network latency. The 60 fps target is for perceived responsiveness on navigation; actual screen refresh only happens when the rendered frame differs from the last (ratatui's buffer diff eliminates no-op writes).

### 3.2 Panel layout

The TUI has 8 panels, each the full terminal frame. Only one panel is active at a time. Toggle between panels with number keys `1`–`8`. The active panel number is always visible in a persistent header line.

```
┌──────────────────────────────────────────────────────────────────────┐
│ Memorum  [1]Overview [2]Review [3]Conflicts [4]Entities [5]Timeline  │
│          [6]Namespaces [7]Policy [8]Reality Check    ?:help  q:quit  │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│                        <active panel content>                        │
│                                                                      │
│                                                                      │
└─────────────────────────────────── memoryd v1.0.0  socket:ok ───────┘
```

**Header** (1 line): panel tabs with active panel highlighted. Always rendered.
**Footer** (1 line): daemon version, socket status (`socket:ok` / `socket:UNREACHABLE`), and a context-sensitive hints line. Always rendered.
**Content area**: remaining terminal height.

#### Panel 1 — Overview

Displays daemon health and activity summary. No navigation; read-only snapshot, refreshed every 250 ms.

```
Daemon       running  (PID 12345)        Uptime: 3d 14h
Socket       /run/user/1000/memoryd.sock ok
Index        ~1,204 active memories      last reindex: 2026-05-01 08:12
Sync         ahead 2 / behind 0          last push: 2026-05-01 11:30
             remote: git@github.com:trey/memory.git

Pending review      7   (3 candidate, 2 quarantined, 2 dream low-confidence)
Conflicts           1
Active sessions     2   (claude-code, codex-cli)
Dreaming            scheduled  next: 2026-05-02 03:00
                    last run: 2026-05-01 03:04  promoted:3 queued:1 dropped:0

Recall (session totals)  startup:42  delta:119  peer-updates:8
```

All numeric fields are live from `RequestPayload::Status` → `StatusResponse`. Sync state from `Substrate::git::status()`. Dreaming from last `dreams/cleanup/<device>/<date>.json`.

#### Panel 2 — Review Queue

Displays all memories in `candidate`, `quarantined`, and `dream_low_confidence` states. Paginated list with j/k navigation and action keys.

Layout:

```
Review Queue  7 items  (3 candidate / 2 quarantined / 2 dream)   filter:[all ▾]

  ► [candidate]  mem_20260501_abc_000001  "Prefer CITEXT for email columns"
    Namespace: project:atlasos  Confidence: 0.72  Added: 3h ago
    Policy: project-standard@v2  Next: requires_user_confirmation

    [quarantined] mem_20260430_def_000004  "SSH key rotation every 90d"
    Namespace: me  Confidence: 0.50  Reason: grounding_rehydration_failed
    ...

a: approve   r: reject   f: forget   q: quarantine   e: edit   /: filter
Enter: expand detail   tab: toggle namespace filter
```

**Item states displayed:**
- `candidate` — promoted to candidate; awaiting review confirmation.
- `quarantined` — held pending user decision; shows quarantine reason.
- `dream_low_confidence` — dream Pass 2 candidate with confidence 0.65–0.85.
- `conflict` — appears here if a memory has a pending merge conflict.

**Detail expansion (Enter on a selected item):** opens memory-detail modal (§3.6) showing the full trust artifact.

**Filter bar (`/`):** opens inline filter input. Supports: `ns:<namespace>`, `reason:<code>`, `since:<duration>`, free-text title/body match (FTS via daemon `memory_search`).

**Actions:**
- `a` (approve): sends `memoryd review approve <id>` via daemon protocol. Confirmed with a brief status flash ("Approved → promoted").
- `r` (reject): sends `memoryd review reject <id>`. Prompts for optional reason.
- `f` (forget): sends `memoryd review forget <id>`. Prompts for reason (required). Asks for confirmation: "Forget and tombstone this memory? [y/N]".
- `q` (quarantine): moves a candidate to quarantined (sends `memoryd review quarantine <id>`). For already-quarantined items, no-op with explanation.
- `e` (edit): opens the memory body in `$EDITOR` (falls back to `$VISUAL`; falls back to `nano`). On editor exit with non-zero or unchanged file, no write is issued. On changed file, issues `memory_supersede` with `source_kind: "user"`.

All mutating actions are confirmed in the footer line before execution, with a 1-second undo window: "Approved mem_… — press u to undo" before the daemon call fires.

#### Panel 3 — Conflicts

Displays memories with active merge conflicts (three-way merge failures that landed in quarantine with `reason: merge_conflict`). Side-by-side diff resolver.

Layout:

```
Conflicts  1 item

[conflict] mem_20260501_ghi_000002  "Database connection pool size"
Namespace: project:atlasos

  LOCAL                                   REMOTE
  ─────────────────────────────────────   ─────────────────────────────────────
  Pool size: 20                           Pool size: 30
  Set 2026-04-29 by codex                 Set 2026-04-30 by claude-code
  Confidence: 0.90                        Confidence: 0.88

  COMMON ANCESTOR
  Pool size: 10  (set 2026-04-28 by codex)

l: accept local   r: accept remote   m: merge (open editor)   q: quarantine
```

**Field-level conflicts** (when frontmatter fields individually conflict): each differing field pair is rendered as a sub-row. `l`/`r` accept one side per field.

**`q` (quarantine):** sends the memory back to quarantined with `reason: user_deferred_conflict`. Removes it from this panel; it appears in Panel 2 for future review.

**`m` (merge in editor):** writes a temp file with conflict markers, opens in `$EDITOR`. On save, performs diff validation (rejects saves that still contain `<<<<<<<` markers) then issues `memory_supersede`.

Navigation: `j/k` move between conflict items. `Enter` expands to field-level diff.

#### Panel 4 — Entities

Entity search and relationship explorer.

Layout:

```
Entities   /entity-search: [atlasos_________]

  Entity: atlasos (project:proj_a3f2)
  ─────────────────────────────────────────────
  Memories: 42 active  (12 canonical, 18 candidate, 12 from supersession chains)
  Last written: 2026-05-01 10:45 by codex
  Last recalled: 2026-05-01 11:02 (delta-block)
  Recall count (30d): 28

  Supersession chains:
    mem_…001 → mem_…004 → mem_…009 (latest)
    mem_…002 → mem_…006 (latest)

  Top memories:
  ► mem_…009  "Deploy target is production ECS"  conf:0.95
    mem_…006  "DB pool size 30"  conf:0.88
    mem_…014  "Prefer CITEXT for emails"  conf:0.72  [candidate]

/: search   Enter: open memory detail   s: show supersession chain   r: recall history
t: show trust artifact for selected   tab: switch between entity list and memory list
```

**Search** (`/`): typeahead search over entity names from frontmatter `entities` fields. Matching is case-insensitive prefix + FTS against entity aliases. Results update as-you-type with a 100 ms debounce.

**Memory list for entity**: all memories where the entity appears in `frontmatter.entities[]`. Sorted by confidence descending, with status badges (`[candidate]`, `[quarantined]`, `[pinned]`).

**Supersession chain view** (`s`): renders the chain as an ASCII tree. Timestamps and devices shown inline.

**Trust artifact view** (`t`): opens the full trust artifact modal (§3.6) for the selected memory.

#### Panel 5 — Timeline

Scrollable event feed from the daemon's event log (`events/<device_id>.jsonl`). Displays the 500 most recent events; older events require filter to retrieve.

Layout:

```
Timeline   last 500 events   filter:[all ▾]   ↑↓ scroll   /: filter

  2026-05-01 11:32:04  write        mem_…022  promoted    namespace:project
  2026-05-01 11:31:58  dream_pass   scope:project  pass:2  promoted:1 queued:0
  2026-05-01 11:30:02  sync         push  2 commits  remote:github
  2026-05-01 11:28:44  recall       session:claude-code  startup  42 items
  2026-05-01 11:15:17  privacy      mem_…019  encrypted_at_rest  label:email
  2026-05-01 11:02:11  recall       session:codex  delta  3 items  peer_updates:1
  2026-05-01 10:45:33  write        mem_…018  candidate   reason:grounding
  2026-05-01 10:20:01  dream_lease  acquired  device:macbook  scope:project
```

**Event types displayed:** `write`, `supersede`, `forget`/`tombstone`, `recall`, `dream_pass`, `dream_lease`, `sync`, `privacy`, `governance_refusal`, `review_action`, `conflict`, `notification`, `reality_check`.

**Filter (`/`):** `kind:<type>`, `ns:<namespace>`, `id:<memory_id>`, `since:<duration>`, `session:<harness>`. Multiple filters are AND-combined.

**Scroll:** `j/k` or arrow keys. `G` jumps to newest. `gg` jumps to oldest loaded. `Enter` on an event with a memory ID opens the memory-detail modal.

**Color coding:** writes in green, refusals/errors in red, privacy events in yellow, dream events in blue, sync events in cyan.

#### Panel 6 — Namespace Explorer

Tree view of the memory namespace hierarchy with inline memory inspection.

Layout:

```
Namespace Explorer                          /: jump to memory by id or search

  ▼ me/                                     ► mem_…_identity_role
    ▼ identity/                               Title: Senior engineer, Rust+TS stack
        role.md          [active] conf:0.95   Namespace: me/identity
        principles.md    [active] conf:0.90   Updated: 2026-04-28
    ▶ relationship/                           Recall (30d): 41
    ▶ knowledge/                              Sensitivity: internal
    ▶ episodic/                               Tags: [identity, role]
  ▶ projects/
    ▶ atlasos/
    ▶ agent-memory/
  ▶ agent/

h/l: collapse/expand   j/k: navigate tree   Enter: open detail   t: trust artifact
/: search by path or title   tab: switch tree ↔ detail pane
```

**Left pane** (60% width): tree view. Directories expand/collapse with `h`/`l` or `Enter`. Files show status badges and confidence.

**Right pane** (40% width): inline preview of selected memory (title, namespace, updated, recall count, sensitivity, tags). Full detail via `Enter` or `t`.

**Quick-jump** (`/`): searches across all canonical paths and memory titles simultaneously. Jumping to a result selects it in the tree and populates the right pane.

**Deferred/encrypted items:** encrypted memories show as `[encrypted]` with no title preview. Reveal requires explicit `memory_reveal` action (not available from this panel — operator runs `memoryd reveal <id>` separately).

#### Panel 7 — Policy Inspector

Active governance policies, recent decisions, and refusal log.

Layout:

```
Policy Inspector                                e: open policy in $EDITOR

  Active policies:
    me-strict@v1          source: disk   (policies/me-strict.yaml)
    project-standard@v2   source: disk   (policies/project-standard.yaml)
    agent-strict@v3       source: built_in_fallback
    dreaming-strict@v1    source: disk   (policies/dreaming-strict.yaml)

  Recent decisions (last 50):
  ► 2026-05-01 11:32  PROMOTED   mem_…022  policy:project-standard@v2  conf:0.95
    2026-05-01 10:45  CANDIDATE  mem_…018  policy:project-standard@v2  grounding:fail
    2026-05-01 09:12  REFUSED    (no-id)   policy:me-strict@v1  reason:tombstone
    2026-05-01 08:55  QUARANTINE mem_…015  reason:review_required

  Refusal reasons (all-time):
    tombstone           12
    grounding            7
    contradiction        3
    policy               2
    review_required      1

j/k: navigate decisions   Enter: expand decision detail   e: edit selected policy
/: filter decisions   r: reload policies from disk
```

**Policy detail:** selecting a policy and pressing `Enter` shows the policy contents inline (YAML rendered with syntax highlighting via `syntect`). `e` drops the cursor into the file in `$EDITOR`.

**`r` (reload):** sends `memoryd policy reload` — triggers live policy re-read from disk without daemon restart. Useful after editing.

**Decision detail expansion:** shows the full governance decision: `policy_applied`, `policy_source`, `confidence_floor_pass`, `grounding_check`, `tombstone_enforced`, `contradiction_result`, `sensitivity_gate_result`.

#### Panel 8 — Reality Check

Drift-risk review ritual surface.

Layout (when not running):

```
Reality Check

  Status: DUE  (last completed: 2026-04-20, 11 days ago)
  Schedule: Sunday 09:00  |  Next: 2026-05-04 09:00
  Notifications: Slack webhook (configured)   OS: disabled

  Top drift-risk memories (12 of 1,204):

  #1  [score:0.82]  "My preferred stack is TypeScript + Rust"
      Namespace: me/identity   Confidence: 0.88   Last observed: 62 days ago
      Recall (30d): 0   Corroboration: single-source
      Score breakdown: staleness:0.35 recall:0.16 corroboration:0.20 decay:0.08 sensitivity:0.03

  #2  [score:0.71]  "atlasos uses Postgres 15 with CITEXT extensions"
      ...

  r: run reality check   s: snooze this week   h: history   /: filter by namespace
```

Layout (when running):

```
Reality Check — ACTIVE   8 of 12 items reviewed

  ► "My preferred stack is TypeScript + Rust"
    me/identity  |  conf:0.88  |  last observed: 62 days ago  |  score: 0.82

    Score breakdown:
      Staleness (0.35×):        62/90 days = 0.69 → contributes 0.24
      Inverse recall (0.20×):   0 recalls in 30d → contributes 0.20
      Cross-source (0.20×):     single-source → contributes 0.20
      Confidence decay (0.15×): original 0.90 → current 0.88 → contributes 0.03
      Sensitivity (0.10×):      internal → contributes 0.03

    c: confirm   k: correct   f: forget   n: not relevant   space: skip this week
```

**Score breakdown is always shown in the active run.** Users must see the data that generated the score. No opaque sorting.

**Actions during an active run:**
- `c` (confirm): marks observed; slight confidence bump; marks as reviewed for this week.
- `k` (correct): opens `$EDITOR` with current body; on save, issues `memory_supersede`.
- `f` (forget): tombstone; prompts for reason.
- `n` (not relevant): lowers passive-recall weight; excluded from future reality checks (not tombstoned; see §5.4 for exact behavior).
- `space` (skip this week): defers this item to next Sunday; all other items in the session continue normally.

### 3.3 Full keymap

Global keys (active in all panels):

| Key       | Action                                              |
|-----------|-----------------------------------------------------|
| `1`–`8`   | Switch to panel N                                   |
| `?`       | Open help overlay (full keymap reference)           |
| `q`       | Quit (with confirmation if unsaved review actions are pending) |
| `Ctrl-c`  | Quit immediately (skips confirmation)               |
| `Ctrl-r`  | Force full state refresh (re-polls daemon)          |
| `:`       | Command prompt (`:q` quit, `:reload`, `:help <topic>`) |

Panel-local keys are defined per panel in §3.2. Keys that appear in multiple panels:

| Key      | Standard meaning (panel-local docs may override)    |
|----------|-----------------------------------------------------|
| `j`/`↓`  | Move down in list/tree                              |
| `k`/`↑`  | Move up in list/tree                                |
| `h`/`←`  | Collapse / back / left pane                         |
| `l`/`→`  | Expand / forward / right pane                       |
| `g g`    | Jump to first item                                  |
| `G`      | Jump to last item                                   |
| `Enter`  | Expand / open detail / confirm selection            |
| `Esc`    | Close modal / cancel search / back to previous panel |
| `/`      | Open search/filter input                            |
| `tab`    | Cycle focus between panes (where applicable)        |
| `u`      | Undo last action (within 1-second window, before daemon call fires) |

**Help overlay** (`?`): rendered as a modal over the current panel. Lists all global and current-panel keys. `?` or `Esc` closes it.

### 3.4 Rendering and refresh strategy

**State model:** `App` struct owns all panel state. Each panel has its own sub-state struct (list cursor positions, filter state, active memory detail, etc.). State updates are driven by two event sources:
1. **Keyboard events** from crossterm's event stream — processed every 16 ms tick.
2. **Daemon poll** — every 250 ms, the app sends a lightweight `RequestPayload::Status` call and a panel-specific query (review queue, timeline entries, entity list, etc.) via the Unix socket client.

**Incremental refresh:** panels only re-render their visible portion. Terminal width/height are tracked; layout recomputes on resize.

**Buffer diff:** `ratatui`'s built-in buffer diffing means only changed cells are written. On idle frames where no state changed, the screen write is a no-op.

**Performance contract:** normal navigation (cursor moves, panel switches, filter keystrokes) must complete within one 16 ms tick at worst. Daemon polls are async and non-blocking; if a poll hasn't returned by the next render tick, the previous data is displayed with a staleness indicator (`[stale]`) next to the affected section. This prevents daemon latency from blocking the UI.

**Modal management:** modals (memory detail, help overlay, confirmation prompts) are rendered as overlapping widgets over the base panel. Modal-local key bindings take priority over panel bindings. At most one modal is open at a time; `Esc` closes the top modal.

### 3.5 Terminal compatibility

**Minimum:** any terminal supporting ANSI escape codes, 80×24. Below 80×24, a warning banner replaces the content area: "Terminal too small (current: WxH, minimum: 80x24)."

**Tested targets:** iTerm2, Terminal.app (macOS), Alacritty, kitty, GNOME Terminal, xterm-256color, tmux (latest). `crossterm` handles the platform differences; no per-terminal special-casing in Stream G code.

**Colors:** uses `ratatui` default color palette (16-color ANSI base, with true-color extended where the terminal declares `COLORTERM=truecolor`). No hardcoded RGB values. Stream G does not ship a theme system; color profile is the terminal's own setting.

**Mouse:** not supported. Zero mouse dependency. Mouse events from crossterm are ignored.

### 3.6 Terminal resize behavior

On resize event (SIGWINCH or crossterm resize):
1. Recompute layout from new `(width, height)`.
2. If below minimum (80×24), show the "Terminal too small" banner and suspend normal rendering.
3. On resize back above minimum, resume normal rendering from current state.
4. No state is lost on resize. Cursor positions are clamped to the new list bounds.
5. Active modals are closed on resize (simpler than re-laying-out a modal whose relative dimensions may now overflow). Footer status remains visible.

### 3.7 Daemon socket unreachable

If the daemon socket is unavailable (connection refused, file not found, timeout):
1. All panel content areas replace with a status box:
   ```
   ┌─ Daemon unreachable ──────────────────────────────────┐
   │ Socket: /run/user/1000/memoryd.sock                   │
   │ Error:  Connection refused                            │
   │                                                       │
   │ Run `memoryd start` to start the daemon.              │
   │ Ctrl-r to retry.  q to quit.                          │
   └───────────────────────────────────────────────────────┘
   ```
2. The footer shows `socket:UNREACHABLE` in red.
3. The TUI enters a retry loop: reconnection attempt every 2 seconds. On reconnection, normal rendering resumes.
4. No stale cached data is displayed as live data. The error box is unambiguous.
5. Keymap in this state: only `Ctrl-r` (immediate retry), `q` (quit), and `?` (help) are active.

---

## 4. Web dashboard architecture

### 4.1 Stack choice: vanilla JS + minimal framework (Preact)

The dashboard runs on localhost. Bundle size matters — a 500 ms paint budget on localhost means the assets must be fast even on a cold cache. Framework options considered:

- **Full React/Next.js/Vue/Svelte SPA:** ruled out. Initial bundle 100–400 KB gzipped, requires a build pipeline inside the daemon, increases binary size significantly. Overkill for a 4-section dashboard.
- **Vanilla HTML+JS+CSS, no framework:** workable for status panels but becomes unmaintainable for the entity graph and audit explorer which need reactive state. Ruled out.
- **Preact (3 KB gzipped) + HTM:** correct choice. Preact is a signal-compatible React-API-equivalent in 3 KB. HTM lets us write JSX-style templates without a build step — templates are tagged template literals compiled at runtime with negligible overhead. Combined with hand-written CSS (no Tailwind, no CSS-in-JS), the full asset bundle stays under 50 KB gzipped.
- **`maud` / `askama` server-side templates:** good for static pages but the entity graph and audit explorer are interactive (force-directed graph, time-scrub); SSR-then-hydrate adds more complexity than a client-side reactive approach for this payload size.

**Decision: Preact + HTM + vanilla CSS, bundled to a single `app.js` + `style.css` at build time via `esbuild`.** The dashboard assets are embedded into the `memoryd-web` binary via `include_bytes!` / `rust-embed`. No runtime asset serving from disk; no CDN; no external network requests.

### 4.2 HTTP server: `axum`

`axum` is already a likely dependency in the daemon ecosystem; it is well-maintained, composable, and integrates cleanly with Tokio. The web server runs as a Tokio task inside `memoryd` on `web enable`, and the `axum` router is spawned on that task's runtime. The web server does not own a separate Tokio runtime.

### 4.3 Routes

All routes are under `localhost:7137`. The port is configurable; see §8.

**Static assets:**

```
GET /                     → index.html (SPA shell)
GET /assets/app.js        → bundled JS (Preact+HTM+app code)
GET /assets/style.css     → stylesheet
GET /assets/fonts/*       → self-hosted fonts (Inter, JetBrains Mono)
```

**API routes (all return `application/json`):**

```
GET  /api/status
     → DaemonStatusResponse (same shape as memoryd status JSON output)

GET  /api/entity-graph
     ?namespace=<ns>&depth=<int>&focus=<entity_id>
     → EntityGraphResponse

GET  /api/entity-graph/:entity_id
     → EntityDetailResponse (memories, supersession chain, recall history)

GET  /api/roi
     ?window=30|90|365
     → RoiResponse

GET  /api/reality-check
     → RealityCheckStatusResponse

POST /api/reality-check/respond
     Content-Type: application/json
     Body: { "memory_id": "mem_…", "action": "confirm"|"correct"|"forget"|"not_relevant"|"skip_this_week", "correction": "…" }
     → RealityCheckActionResponse

GET  /api/reality-check/history
     ?limit=<int>
     → RealityCheckHistoryResponse

GET  /api/audit/:id
     → AuditMemoryResponse (full trust artifact)

GET  /api/audit/:id/walk
     ?direction=up|down&depth=<int>
     → ProvenanceWalkResponse (provenance graph from this memory)

GET  /api/audit/:id/temporal
     ?at=<iso_timestamp>
     → TemporalStateResponse (memory state at a given point in time)

GET  /api/review
     ?status=candidate|quarantined|dream_low_confidence&namespace=<ns>&limit=<int>&offset=<int>
     → ReviewQueueResponse

POST /api/review/action
     Content-Type: application/json
     Body: { "id": "mem_…", "action": "approve"|"reject"|"forget"|"quarantine", "reason": "…" }
     → ReviewActionResponse
```

**JSON shapes** (abbreviated; full types in `crates/memoryd-web/src/routes/*.rs`):

```json5
// GET /api/status
{
  "daemon": { "version": "1.0.0", "pid": 12345, "uptime_seconds": 302440 },
  "socket": "ok",
  "index": { "active_memories": 1204, "last_reindex": "2026-05-01T08:12:00Z" },
  "sync": { "ahead": 2, "behind": 0, "last_push": "2026-05-01T11:30:00Z", "remote": "git@github.com:trey/memory.git" },
  "review": { "candidate": 3, "quarantined": 2, "dream_low_confidence": 2 },
  "conflicts": 1,
  "active_sessions": [{ "harness": "claude-code", "session_id": "…" }, { "harness": "codex-cli", "session_id": "…" }],
  "dreaming": { "status": "scheduled", "next_run": "2026-05-02T03:00:00Z", "last_run": { "at": "2026-05-01T03:04:00Z", "promoted": 3, "queued": 1, "dropped": 0 } },
  "recall": { "startup_total": 42, "delta_total": 119, "peer_update_total": 8 }
}

// GET /api/entity-graph
{
  "nodes": [
    { "id": "ent_atlasos", "label": "atlasos", "namespace": "project:proj_a3f2", "memory_count": 42 }
  ],
  "edges": [
    { "source": "ent_atlasos", "target": "ent_postgres", "kind": "co_mentioned", "weight": 0.72 },
    { "source": "mem_…009", "target": "mem_…004", "kind": "supersedes", "temporal_from": "2026-04-30", "temporal_to": null }
  ]
}

// GET /api/roi?window=30
{
  "window_days": 30,
  "promotion_rate": 0.68,
  "promotion_precision": 0.91,
  "refusal_breakdown": {
    "grounding": 7, "policy": 2, "tombstone": 3, "contradiction": 1, "review_required": 0
  },
  "dreaming": {
    "candidates_generated": 18, "promoted_silent": 9, "entered_review_queue": 5, "dropped": 4,
    "review_queue_approval_rate": 0.80
  }
}

// GET /api/audit/:id
{
  "memory_id": "mem_20260501_abc_000009",
  "title": "Deploy target is production ECS",
  "body": "…",
  "status": "active",
  "namespace": "project:proj_a3f2",
  "confidence": 0.95,
  "confidence_reason": "promoted from candidate; user confirmed; high corroboration",
  "recall_count_total": 28,
  "recall_count_30d": 12,
  "last_recalled": "2026-05-01T11:02:00Z",
  "provenance_chain": [
    { "step": 0, "event": "written_by_agent", "harness": "codex-cli", "session_id": "…", "at": "2026-04-30T14:22:00Z" },
    { "step": 1, "event": "governance_promoted", "policy": "project-standard@v2", "at": "2026-04-30T14:22:01Z" },
    { "step": 2, "event": "user_confirmed_reality_check", "at": "2026-05-01T09:05:00Z" }
  ],
  "policy_decisions": [
    { "policy": "project-standard@v2", "outcome": "promoted", "confidence_floor_pass": true, "grounding_satisfied": true }
  ],
  "privacy_scan": {
    "labels_detected": [],
    "storage_action": "plaintext"
  },
  "supersession_history": [
    { "superseded_id": "mem_…004", "reason": "updated ECS target", "at": "2026-04-30T14:22:00Z" }
  ],
  "sync_state": {
    "on_devices": ["macbook", "desktop"],
    "merge_status": "clean"
  }
}
```

### 4.4 Authentication and concurrent access

**Authentication in v1:** none in the TLS/password sense — the dashboard binds to `localhost` only (no `0.0.0.0`). OS-level process isolation makes the localhost binding owner-only in practice. Remote access requires an SSH tunnel — the daemon never sets up port forwarding.

**CSRF protection:** required because a malicious page loaded in the browser could POST to `localhost:7137/api/review/action` via fetch or form submission. Mitigation:

1. On server start, generate a random 32-byte CSRF token and store it in memory.
2. Serve the token in the initial page HTML as a `<meta name="csrf-token" content="…">` tag (not a cookie; not accessible to cross-origin JS unless the page is same-origin).
3. All mutating POST routes require the header `X-Memorum-CSRF: <token>`. Requests missing or with incorrect token return `403 Forbidden`.
4. Token rotates on server restart (i.e., on `memoryd web restart`). Open browser tabs that cached the old token get a `403` and must refresh.

This is lightweight and correct for a single-user localhost scenario. Full OAuth or cookie-based session auth is v2+ if remote access ever ships.

**Concurrent read access:** read-only GET routes are safe for concurrent browser sessions. Multiple tabs polling `/api/status` every 5 seconds is fine.

**Concurrent mutating access:** mutating actions (review, reality-check respond) are serialized through the daemon protocol. The web server sends each action to the daemon via the Unix socket; the daemon's single-writer model serializes them. If two browser tabs attempt to `POST /api/review/action` for the same memory simultaneously, the first succeeds and the second receives a `409 Conflict` with body `{"error": "memory_not_in_review_state", "current_status": "active"}`.

### 4.5 Browser support

**Target:** Chromium-based browsers (Chrome 112+, Edge 112+) and Firefox 115+ and Safari 16.4+. These cover 95%+ of developer browser usage as of 2026.

**Not supported:** IE, any mobile browser (the dashboard is a desktop-only developer tool), `links`, `lynx`.

**No build-time polyfills needed.** Target browsers all support `async/await`, `fetch`, `CSS Grid`, CSS custom properties, `<dialog>` element, and Preact's signal-based reactivity primitives. `esbuild` targets `es2022`.

### 4.6 Web dashboard sections

#### Section 1 — Entity Graph

Force-directed graph rendered with the D3 force simulation API. Nodes are entities; edges are co-mention relationships (weighted by co-mention frequency) and supersession chains (temporal edges, rendered with a dashed stroke and a timestamp annotation).

**Interaction:**
- Click node: focus on that entity. Pane on right shows entity detail (memory list, recall history).
- Click edge: shows co-mention context or supersession chain detail.
- Scroll: zoom in/out. Drag: pan. Double-click node: navigate into entity detail view (replaces graph with entity-focused list view; breadcrumb to return).
- Namespace filter (dropdown): restrict graph to `me`, `project:<ns>`, `agent`, or all.
- Depth slider (1–3): controls how many hops from the focus node to render. Default 2.

**Rendering limits:** the graph must render 5,000 nodes without choppy interaction (see §12 performance budgets). D3's force simulation is paused after initial stabilization to avoid continuous CPU usage.

**Supersession chains as temporal edges:** edges from `mem_A` → `mem_B` where `mem_B` supersedes `mem_A` are rendered with a dashed line, labeled with the supersede date, and colored distinctly from co-mention edges.

#### Section 2 — Synthesis ROI Dashboard

Three time-window tabs: 30d / 90d / 365d. Each shows:

- **Promotion rate:** `promoted_writes / total_writes`. Trend sparkline.
- **Promotion precision:** `(memories_still_active_at_t + memories_that_generated_follow_up) / total_promoted_in_window`. Proxy for "did promoting this memory have lasting value?"
- **Refusal breakdown:** pie chart of refusal reasons. Hover shows count and percentage.
- **Dream value:** bar chart: `silent_promotions`, `review_queue_entries`, `approved_from_queue`, `dropped`. Tracks dreaming's contribution over time.
- **Reality Check adherence:** weeks completed vs. skipped.

All data comes from `GET /api/roi?window=<n>`, computed by the daemon from the event log and index.

#### Section 3 — Reality Check UI

More ergonomic than TUI Panel 8. Memory items rendered as cards:

- Card header: title, namespace, score badge.
- Card body: score breakdown bar chart (same five components as TUI Panel 8, rendered as colored bars).
- Card footer: action buttons — Confirm / Correct / Forget / Not Relevant / Skip This Week.

"Correct" action opens an inline editor (textarea, pre-filled with current body) with Save/Cancel. On Save, issues `POST /api/reality-check/respond` with `action: "correct"` and the new body.

Session progress indicator at top: "5 of 12 reviewed this week."

**History tab:** completed sessions with dates, counts of each action type, and trend over weeks.

#### Section 4 — Audit Explorer

Provenance graph walk and time-scrub temporal validity.

**Default view:** memory detail card (same trust artifact fields as TUI; see §7).

**Provenance walk:** "Walk provenance" button triggers `GET /api/audit/:id/walk?direction=up&depth=3`. Renders the walk as an interactive DAG: nodes are memories/events; edges are provenance relationships. Click node to navigate to that memory's audit view. Breadcrumb trail for walk history.

**Time scrub:** slider at bottom of audit view. Dragging the slider to a past timestamp calls `GET /api/audit/:id/temporal?at=<ts>` and re-renders the trust artifact card as it was at that time. Red "viewing historical state" banner when slider is not at present. This is read-only; no time-travel writes.

---

## 5. Reality Check

### 5.1 Algorithm — drift-risk scoring

Weights locked by system-v0.2 §16.4:

```
score(m) = 0.35 * days_since_observed_norm(m)
         + 0.20 * (1 - recall_frequency_norm(m))
         + 0.20 * (1 - cross_source_corroboration(m))
         + 0.15 * confidence_decay(m)
         + 0.10 * sensitivity_weight(m)
```

**Normalization functions (exact):**

```
days_since_observed_norm(m)  = min(1.0, (now - m.observed_at).days / 90.0)
```
`observed_at` is set at initial write and updated on each `confirm` action. Saturates at 90 days. Source: `memories.observed_at` (already in shipped index).

```
recall_frequency_norm(m)     = recall_count_30d(m) / max(max_recall_30d_active, 1)

recall_count_30d(m) = SELECT COUNT(*) FROM events_log
                     WHERE kind = 'recall_hit'
                       AND memory_id = m.id
                       AND ts > (now - 30d)
```
**Derived at score time via SQL against the `events_log` SQLite table** (the v0.2 mirror added in §1.3 #2; canonical store remains per-device JSONL). The covering index `events_log(kind, memory_id, ts)` keeps the per-memory query sub-millisecond. `max_recall_30d_active` is the maximum value across all currently `active` memories in scope, computed once per scoring run via a single GROUP BY query. Bounded in `[0, 1]`.

If the events log has no `RecallHit` rows for `m` in the last 30 days, `recall_count_30d(m) = 0` and the term `(1 - recall_frequency_norm(m))` contributes the full 0.20 weight (highest possible drift risk from this component) — consistent with "this memory has not been recalled recently, treat as drifting."

```
cross_source_corroboration(m) = 1  if distinct_sources(m) >= 2
                               = 0  otherwise

distinct_sources(m) =
    WITH RECURSIVE chain(memory_id, depth) AS (
      SELECT m.id, 0
      UNION ALL
      SELECT ms.supersedes_id, c.depth + 1
        FROM memory_supersession ms
        JOIN chain c ON ms.memory_id = c.memory_id
       WHERE c.depth < 8
    )
    SELECT COUNT(DISTINCT mem.source_harness)
      FROM chain
      JOIN memories mem ON chain.memory_id = mem.id
```
**Derived from the `memories` index plus the `memory_supersession` join table** (added in §1.3 #3). `source_harness TEXT` is already a column on `memories`, populated on every write through the shipped Stream A index path; `memory_supersession(memory_id, supersedes_id)` is the v0.2 derived projection populated from `Frontmatter.supersedes`. The CTE walks the supersession chain depth-bounded at 8 levels — adequate for any plausible chain, and **the `WHERE c.depth < 8` predicate also serves as the cycle guard** so a malformed supersession ring cannot infinite-loop. Two writes from the same harness, even in different sessions, count as 1; a memory written by `claude-code` and superseded by `codex` counts as 2. There is no `memories.supersedes_ids` column; references to one in earlier drafts were a fiction. The earlier-drafted formula sourced harness identity from `WriteCommitted` event payloads, but those payloads do not carry session_id and pulling harness from `memories` is simpler.

**NULL `source_harness` handling.** `Source.harness` in the shipped model is `Option<String>` (`crates/memory-substrate/src/model.rs`) and the `memories.source_harness` column is nullable — for example, a `memory_note` written without harness attribution writes `NULL`. `COUNT(DISTINCT source_harness)` excludes NULL by SQL convention; that exclusion is intentional under this spec. NULL means "unknown harness," and an unknown harness is not corroborating evidence. Two writes — one with `source_harness = NULL`, one with `source_harness = 'codex'` — yield `distinct_sources(m) = 1` and `cross_source_corroboration(m) = 0`. The conservative floor matches the rest of the formula's "no signal → no credit" behavior (cf. `confidence_decay` for missing `original_confidence`). Implementations must include explicit test coverage for the NULL case.

```
confidence_decay(m)          = match m.original_confidence {
    Some(c0) => max(0.0, c0 - m.current_confidence),
    None     => 0.0,   // pre-v0.2 memories: no baseline, no decay
}
```
`original_confidence` is a v0.2-added `Option<f64>` field on `Frontmatter` (Stream A surface, authorized in system-v0.2 §19) set at initial promotion and never mutated. If confidence was manually raised (user confirms, corroboration added), the raw difference is negative and clamps to 0.0. Pre-v0.2 memories that lack the field score 0.0 on this component (a conservative floor — without a baseline, drift cannot be measured).

```
sensitivity_weight(m)        = match m.sensitivity {
    None | "public"           => 0.0,
    "internal"                => 0.3,
    "confidential"            => 0.6,
    "personal"                => 1.0,
}
```

Final `score(m)` is bounded in `[0.0, 1.0]` (component weights sum to 1.0).

**Top N selection:** default N=12. Sort all `active` and `pinned` memories by `score(m)` descending. Take top 12. `pinned` memories appear in the list regardless of score (they may have drifted; the user pinned them for a reason). The 12-item cap is configurable (`reality_check.top_n`; see §8).

**Excluded from scoring:** `candidate`, `quarantined`, `tombstoned`, `archived`, `superseded`, and memories with `retrieval_policy.passive_recall: false` are not scored.

**Encrypted memories:** scored using index-visible fields only (namespace, timestamps, sensitivity, recall_count from the safe index projection). No body, no title. Shown in the list as `[encrypted — title not available]` with score breakdown. The score is valid; it uses the same formula. The user can `forget` or `skip` them; `confirm` and `correct` require running `memoryd reveal` first.

### 5.2 Scheduling

**Default schedule:** Sunday, 09:00 local time, weekly. Configurable as `reality_check.schedule` in `config.yaml` (cron expression string). Minimum interval: 7 days. Maximum interval: 90 days. Invalid cron expressions at config load → fail-closed: daemon logs a warning and uses the default Sunday 09:00.

**How the daemon fires the schedule:**

1. On daemon startup and once per hour, check whether a reality-check is due.
2. "Due" = the configured schedule time has passed since the last completed session (`reality_check.last_completed_at` stored in daemon state file `~/.memoryd/state.json`), and no snooze is active for the current week.
3. When due:
   a. Compute drift scores for all scored memories.
   b. Store the scored list in `~/.memoryd/reality-check-pending.json` (daemon-local, not in the git tree).
   c. Fire `NotificationEvent::RealityCheckDue`.
   d. Add `<pending-attention kind="reality_check_due">` to the next session's recall block (§1.3).
   e. If `notifications.external.triggers` includes `reality_check_due`, dispatch the Slack/email notification (§6).

**On-demand trigger:** `memoryd reality-check run` computes scores and enters the review UI immediately, regardless of schedule. Does not change `last_completed_at` unless the user completes the session.

### 5.3 Data flow — session lifecycle

**Starting a session:**
1. User runs `memoryd reality-check run` (CLI), types `r` in TUI Panel 8, clicks "Start" in web dashboard Section 3, or receives `/memory-reality-check` slash command.
2. Daemon fetches the pre-computed `reality-check-pending.json` (or computes fresh if not cached or >30 minutes old).
3. Items are served one by one. State is held in memory; partial sessions are resumable within the same daemon session (stored in `~/.memoryd/reality-check-session.json`).

**Completing a session:**
1. User has responded to all N items (or items remaining are all `skip_this_week`).
2. Daemon writes `last_completed_at = now` to `~/.memoryd/state.json`.
3. Session state file `reality-check-session.json` is deleted.
4. Pending-attention `reality_check_due` item is cleared from the next recall block.

**Abandoned sessions:** if a session is started but not completed (daemon restart, user closes TUI mid-run), `reality-check-session.json` persists. On next daemon start or next `memoryd reality-check run`, the interrupted session is offered for resumption: "Resume previous session (5 of 12 remaining)? [Y/n]". If declined, the session is discarded and a fresh run starts.

**Slack/email notification sent at session start (external channel):** "Your weekly Memorum Reality Check is ready. Run `memoryd reality-check run` or open the dashboard." Contains no memory content.

### 5.4 User response actions

**`confirm`:**
- Sends `POST /api/reality-check/respond` or daemon protocol `RequestPayload::RealityCheckRespond { action: Confirm }`.
- Daemon: sets `memory.observed_at = now`, bumps `confidence` by 0.02 (clamped to 1.0), appends event `EventKind::RealityCheckConfirmed`.
- No governance gate needed — confirm is a metadata-only update, not a content change.

**`correct`:**
- User provides new body text via `$EDITOR` (TUI) or inline textarea (web).
- Daemon: issues `memory_supersede` internally with `source_kind: "user"`, `explicit_user_context: true`. Goes through full governance pipeline.
- If governance refuses the correction (e.g., tombstone match), the correction is rejected with the refusal reason displayed; the reality-check item is not marked as reviewed.
- On successful supersession: marks item reviewed, old memory tombstoned, new memory created.

**`forget`:**
- User provides reason (required; minimum 3 characters; otherwise "reason too short" error displayed inline).
- Daemon issues `memory_forget` via governance path. Reason stored in tombstone record.
- Item removed from session. `EventKind::RealityCheckForgotten` appended.

**`not_relevant`:**
- Sets `memory.retrieval_policy.passive_recall = false` and appends `tags: ["reality_check_not_relevant"]`.
- Memory is NOT tombstoned. It remains in the index and can be retrieved via explicit search.
- Excluded from future reality-check scoring (`passive_recall: false` filter matches in scoring query).
- `EventKind::RealityCheckNotRelevant` appended.
- Reversible: `memoryd pin <id>` re-enables passive recall for a memory.

**`skip_this_week`:**
- Item is deferred to the next scheduled reality-check session. No frontmatter mutation.
- Tracked in `reality-check-session.json` as `deferred_this_week: [mem_id, …]`. Deferred items are excluded from the count toward "session complete" for this week.

### 5.5 Stale sessions (skipped 3 weeks in a row)

If `last_completed_at` is more than 21 days ago (3× weekly cadence):
- TUI Panel 8 shows a warning banner: "Reality Check overdue — 3 sessions skipped."
- Web dashboard Section 3 shows a warning card.
- `memoryd reality-check run` prepends a notice before starting the session: "You've skipped the last 3 sessions. Items that have drifted the most are surfaced first."
- The pending list is re-sorted with `skip_this_week` items from prior sessions interspersed — they are now surfaced as normal items, not deferred.
- `NotificationEvent::RealityCheckOverdue` is fired, which adds a higher-priority `<pending-attention>` line and (if configured) sends a Slack/email.

### 5.6 Score evolution over time

Scores are recomputed on each scheduled run, not persisted between runs. A memory's score changes organically:
- Confirms lower staleness and raise recall frequency → lower score.
- Memories that are never recalled accumulate higher staleness → higher score.
- New corroborating sources lower corroboration contribution → lower score.
- Confidence bumps (user confirmation, corroboration) reduce decay contribution → lower score.

The scoring run for the next session sees the current state of the index, not a snapshot from when the last session ran.

### 5.7 Daemon protocol — Reality Check wire shapes

Authorized in system-v0.2 §19's `RequestPayload`/`ResponsePayload` row. All variants are admin-surface and **must be rejected from the MCP forwarder** (per system-v0.2 §14.3); they are reachable only via the Unix socket protocol, the CLI, and the localhost web dashboard.

**Request variants** (all carry the standard envelope `{ "version": "0", "id": <u32>, "payload": { ... } }` — same as Stream B/C/D/E/F variants):

```rust
pub enum RealityCheckRequest {
    /// Compute scores and return the top-N pending list, but do NOT start a session
    /// or mark anything as reviewed. Used by TUI/dashboard panel 8 to render
    /// the queue without committing the user to a session.
    List {
        /// Optional namespace filter; None = all namespaces.
        namespace: Option<String>,
        /// Optional cap override. Defaults to `reality_check.top_n` from config (12).
        limit: Option<usize>,
    },

    /// Start (or resume) a reality-check session. If a session file exists,
    /// returns the existing session; otherwise computes fresh scores and
    /// creates a new session. Idempotent within a single session id.
    Run {
        /// Optional session id. If None, daemon mints a new one.
        /// If Some and a matching session file exists, resume it.
        session_id: Option<String>,
        /// Optional namespace filter (forwarded to scoring).
        namespace: Option<String>,
    },

    /// Respond to a single item in the active session.
    Respond {
        /// Session id returned by `Run`.
        session_id: String,
        /// Memory id being responded to.
        memory_id: MemoryId,
        /// One of: Confirm | Correct { new_body: String } | Forget { reason: String }
        ///       | NotRelevant | SkipThisWeek
        action: RealityCheckAction,
    },

    /// Snooze the current week's reminder. Suppresses the `<pending-attention>`
    /// emission and `RealityCheckDue` notification for the rest of the week.
    Snooze,

    /// Clear all pending state, abandon any session in progress, and recompute
    /// on the next scheduled trigger. Admin-only.
    Reset,
}

pub enum RealityCheckAction {
    Confirm,
    Correct { new_body: String },
    Forget { reason: String },
    NotRelevant,
    SkipThisWeek,
}
```

**Response variants:**

```rust
pub enum RealityCheckResponse {
    /// Returned for List and the initial Run reply.
    Pending {
        session_id: Option<String>,    // present for Run, absent for List
        items: Vec<RealityCheckItem>,
        /// Total memories scored (may exceed items.len() if limit applied).
        total_scored: usize,
        /// Last completed session timestamp; None if never completed.
        last_completed_at: Option<DateTime<Utc>>,
    },

    /// Per-action response.
    RespondAccepted {
        session_id: String,
        memory_id: MemoryId,
        next_item: Option<RealityCheckItem>,    // None when session complete
        completion: RealityCheckCompletion,     // Progress | Complete
    },

    /// Action was rejected — typically because governance refused the underlying
    /// supersede/forget. The session is NOT advanced; the user can retry.
    RespondRefused {
        session_id: String,
        memory_id: MemoryId,
        reason: String,                         // refusal text from governance
        kind: RespondRefusalKind,               // GovernanceRefused | TombstoneMatch | InvalidAction | SessionExpired
    },

    Snoozed {
        snooze_until: DateTime<Utc>,
    },

    Reset {
        cleared_pending: usize,
        cleared_session: bool,
    },
}

pub struct RealityCheckItem {
    pub memory_id: MemoryId,
    pub title: String,                          // empty string for encrypted memories
    pub namespace: String,
    pub status: MemoryStatus,
    pub sensitivity: Option<Sensitivity>,
    pub score: f64,                             // 0.0..=1.0, the final drift score
    pub component_scores: ComponentScores,      // see below — required for trust artifact rendering
    pub encrypted: bool,                        // true → title is empty, body cannot be shown
    pub last_observed_at: DateTime<Utc>,
    pub recall_count_30d: u32,
    pub last_recalled_at: Option<DateTime<Utc>>,
}

pub struct ComponentScores {
    pub days_since_observed_norm: f64,
    pub recall_frequency_norm: f64,             // already normalized; (1 - this) is what enters the formula
    pub cross_source_corroboration: f64,        // 0.0 or 1.0
    pub confidence_decay: f64,
    pub sensitivity_weight: f64,
}

pub enum RealityCheckCompletion {
    Progress { remaining: usize, deferred: usize },
    Complete { reviewed: usize, deferred: usize, completed_at: DateTime<Utc> },
}
```

**Authorization:**

- All variants are admin/UI surface. The MCP forwarder rejects them with the same `MethodNotAllowedOnMcp` error used for `privacy`/`device`/`review` admin commands (system-v0.2 §14.3).
- `Run`, `Respond`, `Snooze`, `Reset` mutate daemon state and require the standard daemon-socket access (owner-only chmod, established by Stream B); no additional auth beyond socket ownership.
- `List` is read-only.

**Field definitions for `ComponentScores`:** the wire shape is the JSON serialization of the names above (snake_case). Stream H test #16 asserts on these field names — they are the contract for component-score introspection.

### 5.8 State files — crash recovery semantics

The three daemon state files declared in §1.3 #3 each have explicit load/recover/discard rules. None of them are critical-path: a corrupt or missing state file means at worst "the next Reality Check is computed from scratch and any in-flight session is lost," never "Memorum cannot start."

**`<runtime_root>/state/state.json`** — daemon-wide, persists across restarts.

Schema:
```json
{
  "version": 1,
  "reality_check": {
    "last_completed_at": "2026-04-26T15:32:11Z",
    "snooze_until": null
  }
}
```

- **Load on daemon startup:** read, parse, validate `version == 1`. If missing, parse error, or `version` mismatch → log a warning and treat as if `last_completed_at = null` and `snooze_until = null`. The daemon does NOT refuse to start.
- **Write:** atomic via `tempfile-then-rename` (write to `state.json.tmp`, fsync, rename). Standard pattern from Stream A.
- **Forward compatibility:** unknown fields tolerated (deserialize with `#[serde(default)]` for any new fields v1.1+ adds; v1 only writes the fields above).

**`<runtime_root>/state/reality-check-pending.json`** — pre-computed top-N pending list.

Schema:
```json
{
  "version": 1,
  "computed_at": "2026-05-01T09:00:03Z",
  "items": [ /* RealityCheckItem array */ ]
}
```

- **Load on `RealityCheckRequest::Run`:** if `computed_at` is within the last 30 minutes, reuse. Otherwise recompute and overwrite. If parse fails, treat as missing and recompute.
- **Stale tolerance:** an aged file is fine — recomputation is the normal path on schedule trigger; the file is a cache, not a source of truth.
- **Cleanup:** deleted on session completion (alongside the session file). Reset by `RealityCheckRequest::Reset`.

**`<runtime_root>/state/reality-check-session.json`** — in-flight session state.

Schema:
```json
{
  "version": 1,
  "session_id": "rcs_01HXYZ...",
  "started_at": "2026-05-01T09:01:14Z",
  "items_total": 12,
  "items_reviewed": ["mem_...", "mem_..."],
  "items_deferred": [],
  "items_remaining": ["mem_...", "mem_...", "mem_..."],
  "current_index": 5
}
```

- **Load on daemon startup or fresh `Run`:** if file exists and is parseable, daemon offers session resumption (per §5.3 "Abandoned sessions" UX). User can resume, discard, or ignore.
- **Partial-write recovery:** every state mutation writes via `tempfile-then-rename`. A crash mid-write leaves either the prior version or the new version, never a corrupt file. If the file is somehow corrupt (e.g., disk full mid-fsync, manual edit), parse failure is treated as no-session-in-progress and the file is renamed to `reality-check-session.json.corrupt-<timestamp>` for forensics; the user starts fresh.
- **TTL:** session files older than 7 days are auto-discarded on daemon startup (a session that has gone stale beyond a full week of inactivity has no useful recovery point).
- **Cleanup:** deleted on session completion (`RealityCheckCompletion::Complete`) or on `Reset`.

**Concurrent write protection:** the daemon is single-process; only one writer touches these files. The TUI, web dashboard, and CLI all go through the daemon socket — they never read/write state files directly. If a future deployment puts a second `memoryd` instance on the same runtime root (e.g., a misconfigured launchd unit), Stream A's existing socket-bind exclusion (only one daemon owns the socket) prevents two daemons from coexisting on the same root, which inherits to these state files.

---

## 6. Notifications

### 6.1 Channels

**Passive (always on):**
- Surfaces in `memoryd status` as a list of pending notifications.
- Added to the next session's `<pending-attention>` block (Stream E §1.3 addition from this spec).
- Zero configuration required; cannot be disabled.
- Does not interrupt any active session or workflow.

**OS notification (opt-in, disabled by default):**
- macOS: `osascript` calling `display notification`. Linux: `notify-send`.
- Only fired for high-urgency triggers (see §6.2).
- Enabled via `notifications.os.enabled: true` in `config.yaml`.
- If `osascript` / `notify-send` are unavailable, falls back to passive silently (logs a warning at daemon startup: "OS notifications configured but tool not found; falling back to passive").

**External (Slack webhook or email):**
- Slack: HTTP POST to the configured `webhook_url` with a structured JSON payload (see §6.3 for payload shape).
- Email: SMTP send to configured `to` address.
- Not both simultaneously unless both are configured under separate `channel` entries.
- Used for scheduled triggers (reality check due, daily synthesis summary).
- Configuration in `config.yaml` `[notifications.external]` block (see §8).

### 6.2 Trigger definitions

| Trigger name | Default channels | Description |
|---|---|---|
| `leaked_secret_detected` | passive + OS (if enabled) | Stream D detects a `Refuse`-tier write; the write was blocked but an attempt happened |
| `blocking_merge_conflict` | passive + OS (if enabled) | git merge produced a conflict that blocked sync push |
| `review_queue_over:<N>` | passive | review queue (candidate+quarantined+dream) exceeds N items; default threshold 50 |
| `reality_check_due` | passive + external (if configured) | scheduled Reality Check is ready |
| `reality_check_overdue` | passive + external (if configured) | 3+ weekly sessions skipped |
| `dream_run_completed` | passive | dream pass completed; surfaced only if `promoted > 0 OR queued > 0` |
| `daily_synthesis_summary` | external (if configured) | daily summary of dream + governance activity |

**`leaked_secret_detected` triggers the OS channel by default when enabled because a secret write attempt is a security event; it warrants active interruption.**

**`review_queue_over:N`:** N defaults to 50. Configurable as `notifications.os.triggers: ["review_queue_over:25"]` etc.

### 6.3 Dispatcher architecture

The dispatcher is a Tokio task spawned inside `memoryd` at startup. It subscribes to the `tokio::sync::broadcast::Receiver<NotificationEvent>` channel (§1.3).

```rust
async fn notification_dispatcher(
    mut events: broadcast::Receiver<NotificationEvent>,
    config: NotificationConfig,
) {
    loop {
        match events.recv().await {
            Ok(event) => dispatch_event(event, &config).await,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("notification dispatcher lagged {} events", n);
                // continue; lagged events are dropped — passive channel catches them
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}
```

**Channel routing per event:**
1. Always append to the in-memory passive queue (capped at 100 entries, FIFO drop when full).
2. If `os.enabled` and event is in `os.triggers`: fire OS notification.
3. If event is in `external.triggers` and external is configured: fire Slack/email.

The passive queue is drained by `memoryd status` and by the recall assembly (Stream E pending-attention hook). Items older than 7 days are dropped from the queue at drain time.

### 6.4 Slack webhook payload shape

```json
{
  "text": "Memorum: Weekly Reality Check is ready.",
  "blocks": [
    {
      "type": "section",
      "text": {
        "type": "mrkdwn",
        "text": "*Memorum Weekly Reality Check*\n12 memories to review. Run `memoryd reality-check run` or open <http://localhost:7137|the dashboard>."
      }
    },
    {
      "type": "context",
      "elements": [{ "type": "mrkdwn", "text": "Device: macbook  |  2026-05-04 09:00" }]
    }
  ]
}
```

The payload contains **no memory content** — no titles, no bodies, no entity names. Reason: the webhook URL may be shared with a Slack channel that others can see; memory content privacy must not be compromised by the notification channel. Only counts, timestamps, and invocation instructions.

### 6.5 Retry policy

**Slack webhook failures:**
- Retry with exponential backoff: 30s, 120s, 600s, then give up.
- Maximum 3 retries per notification event.
- On final failure: log to daemon log at WARN level; add to passive queue as `"External notification failed: <reason>"`.
- No dead-letter queue on disk. Dead notifications do not accumulate; the underlying condition (reality check due) remains visible via passive channel.

**Email (SMTP) failures:**
- Same retry policy as Slack.
- SMTP connection failure (auth error, unreachable) logged once at ERROR level; retries paused for 1 hour before trying again.

**OS notification failures:**
- No retry. OS notifications are best-effort. If `osascript` fails (e.g., user not logged into GUI session), log at DEBUG and continue.

### 6.6 Configuration

See §8 for the full `config.yaml` block. Key defaults:

- `notifications.passive`: always active; no config needed.
- `notifications.os.enabled`: `false`.
- `notifications.os.triggers`: `["leaked_secret_detected", "blocking_merge_conflict", "review_queue_over:50"]`.
- `notifications.external.channel`: not set (external notifications disabled by default).
- `notifications.external.triggers`: `["reality_check_due", "daily_synthesis_summary"]` (takes effect only when channel is configured).

---

## 7. Trust artifact data sources and rendering

Every memory's detail view — in TUI Panel 4 (entity detail), Panel 2 (review queue expansion), Panel 6 (namespace explorer), and the web dashboard's audit explorer — shows the full trust artifact. "No black boxes" is the design constraint.

### 7.1 Data sources per field

| Field | Source | Query |
|---|---|---|
| Title, body, namespace, status | Stream A `Substrate::read_memory(id)` | Direct file read + frontmatter parse |
| Confidence (current) | Frontmatter `confidence` field | Same read |
| Confidence reason | `frontmatter.confidence_reason` (optional annotation field, set by governance on promotion and by user confirms) | Same read |
| Recall count (total, 30d) | Derived from events log: `SELECT COUNT(*) FROM events_log WHERE kind='recall_hit' AND memory_id=?` (total) and `... AND ts > now-30d` (30d). Uses the covering index added in §1.3 #2. | Stream A events log + covering index |
| Last recalled timestamp | Derived from events log: `SELECT MAX(ts) FROM events_log WHERE kind='recall_hit' AND memory_id=?`. Returns NULL if never recalled. | Stream A events log + covering index |
| Provenance chain | Stream A event log: filter `events/<device_id>.jsonl` for `memory_id == <id>`, emit events: `WrittenByAgent`, `GovernanceDecision`, `Superseded`, `RealityCheckConfirmed`, `Revealed`, `DreamPromotion` | Event log scan — cached per memory detail open |
| Policy decisions | From provenance chain events of kind `GovernanceDecision`; each carries `policy_applied`, `policy_source`, `confidence_floor_pass`, `grounding_satisfied`, `tombstone_enforced`, `contradiction_result`, `sensitivity_gate_result` | Event log (same scan) |
| Privacy scan results | `frontmatter.privacy_scan` if present (set by Stream D on write); or real-time `DeterministicPrivacyClassifier::classify(body)` for memories written before Stream D | Frontmatter + optional re-scan |
| Supersession history | Stream A index: `SELECT supersedes_id FROM memory_supersession WHERE memory_id = ?` (forward edges) and `SELECT memory_id FROM memory_supersession WHERE supersedes_id = ?` (reverse edges, served by `idx_memory_supersession_supersedes_id`); recursive walk via the bounded CTE shape from §5.1 | `memory_supersession` derived projection (added in §1.3 #3) — frontmatter remains canonical |
| Sync state (devices) | Stream A event log: distinct `device_id` values in events for this memory; plus `git log --all --oneline -- <memory_path>` output | Event log + git |
| Merge status | Stream A `Substrate::git::status()` for the memory's path specifically | Git status for path |
| Claim-lock status (Stream I) | Daemon in-memory presence state (if Stream I is active): `GET /api/peer/claim-lock?id=<id>` | Daemon state |

### 7.2 Rendering format — TUI memory detail modal

The modal overlays the full terminal frame minus 2-column/1-row padding. Scrollable with `j/k`. `Esc` closes.

```
┌─ Memory Detail ────────────────────────────────────────────────────────── ✕ ┐
│ mem_20260501_abc_000009                                                      │
│ "Deploy target is production ECS"                                            │
│ namespace: project:atlasos  status: active  sensitivity: internal            │
│                                                                              │
│ Body:                                                                        │
│   The ECS cluster in us-east-1 is the production deployment target.          │
│   All deploy scripts should target this cluster.                             │
│                                                                              │
│ ─── Confidence ──────────────────────────────────────────────────────────── │
│ Current: 0.95  Original: 0.90                                               │
│ Reason:  promoted from candidate; user confirmed 2026-05-01; corroborated   │
│          by 2 sources (codex-cli, claude-code)                              │
│                                                                              │
│ ─── Recall ─────────────────────────────────────────────────────────────── │
│ Total: 28  (30d: 12)  Last: 2026-05-01 11:02 via delta-block               │
│                                                                              │
│ ─── Provenance ─────────────────────────────────────────────────────────── │
│  1. 2026-04-30 14:22  written by codex-cli (sess_abc123)                   │
│  2. 2026-04-30 14:22  governance: promoted  policy:project-standard@v2      │
│                        conf_floor:pass  grounding:satisfied                  │
│  3. 2026-05-01 09:05  user confirmed (reality check)                        │
│  4. 2026-05-01 11:02  recalled in delta-block (session:claude-code)         │
│                                                                              │
│ ─── Policy Decisions ───────────────────────────────────────────────────── │
│  project-standard@v2 (disk)                                                 │
│    conf_floor: 0.80 → pass (0.90)                                           │
│    grounding: 2 source refs resolved                                        │
│    contradiction: none detected                                              │
│    sensitivity_gate: pass (internal)                                        │
│                                                                              │
│ ─── Privacy Scan ───────────────────────────────────────────────────────── │
│  Labels detected: none                                                       │
│  Storage action: plaintext                                                   │
│                                                                              │
│ ─── Supersession ───────────────────────────────────────────────────────── │
│  Supersedes: mem_…004 (2026-04-28)  "Deploy target ECS (initial)"           │
│              mem_…001 (2026-04-27)  "Deploy target TBD"                     │
│  Superseded by: (none — this is the latest)                                 │
│                                                                              │
│ ─── Sync State ─────────────────────────────────────────────────────────── │
│  Devices: macbook (written here), desktop (synced 2026-05-01 06:00)        │
│  Merge status: clean                                                        │
│                                                                              │
│ j/k: scroll   e: edit   f: forget   p: pin   Esc: close                    │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Encrypted memories:** body and title sections show `[encrypted — use memoryd reveal <id> to decrypt]`. All other fields (provenance chain, policy decisions, privacy scan summary using safe-index-only fields) render normally. The trust artifact is complete even for encrypted memories; only the content is withheld.

### 7.3 Rendering format — web audit card

The web audit card (Section 4 — Audit Explorer) shows the same fields in HTML layout:

- Horizontal pill badges for status, sensitivity, privacy storage action.
- Timeline component for provenance chain (vertical with connecting lines, expandable per event).
- Confidence bar (current vs. original, color-coded: green if stable/growing, amber if decayed).
- Recall histogram (30-day bar chart).
- Supersession chain as a mini-force-directed DAG (using the same D3 code as Section 1, but scoped to this memory's chain).
- Policy decisions in an expandable accordion per policy.
- Sync state as device badges with "last seen" timestamps.

---

## 8. Configuration schema additions to `config.yaml`

All keys below are under `config.yaml` at `~/.memory/config.yaml`. They are additive to the existing Stream A–F keys; this spec does not redefine existing keys.

```yaml
# ─── Stream G additions ────────────────────────────────────────────────────

ui:
  # TUI configuration
  enabled: true                  # if false, `memoryd ui` exits immediately with a message
  tick_rate_ms: 16               # render/input tick; range: 10–100; default: 16
  poll_rate_ms: 250              # daemon poll interval; range: 100–5000; default: 250
  editor: ""                     # override $EDITOR for `e` key; empty = use $EDITOR env
  confirm_timeout_ms: 1000       # undo window after mutating action; range: 0–5000; default: 1000
  # Behavior on invalid: unknown fields rejected at config load → daemon logs warning + uses defaults

web:
  enabled: false                 # default off; `memoryd web enable` sets this to true
  port: 7137                     # range: 1024–65535; default: 7137; conflicts with existing ports: fail at bind
  bind_address: "127.0.0.1"      # MUST be 127.0.0.1 or ::1; "0.0.0.0" rejected at config load with ERROR
  # On invalid bind_address: daemon refuses to start web server, logs error, continues without web
  cors_allowed_origins: []       # empty = strict same-origin only; adding an origin widens CORS
  static_assets_cache_secs: 3600 # browser cache TTL for JS/CSS; range: 0–86400

reality_check:
  enabled: true
  schedule: "0 9 * * SUN"        # cron expression (5-field); default: Sunday 09:00
  # Behavior on invalid cron: daemon logs warning, uses default "0 9 * * SUN"
  top_n: 12                      # memories per session; range: 5–50; default: 12
  score_weights:
    staleness: 0.35              # weight of staleness component; must sum to 1.0 across all five
    inverse_recall: 0.20
    corroboration: 0.20
    confidence_decay: 0.15
    sensitivity: 0.10
  # Behavior on invalid weights (don't sum to 1.0, negative values):
  # daemon logs warning "reality_check.score_weights do not sum to 1.0; using v0.2 defaults"
  # and falls back to the locked defaults above
  staleness_saturation_days: 90  # range: 30–365; default: 90
  overdue_threshold_days: 21     # range: 7–90; if last_completed_at older than this, show overdue; default: 21

notifications:
  passive: always                # fixed value; no other valid value

  os:
    enabled: false
    triggers:
      - leaked_secret_detected
      - blocking_merge_conflict
      - "review_queue_over:50"   # threshold configurable; format "review_queue_over:<int>"
    # Behavior on unknown trigger names: log warning, skip that trigger; do not fail
    # Behavior on invalid review_queue_over threshold: log warning, use 50

  external:
    # Leave unconfigured to disable external notifications entirely
    channel: slack               # "slack" or "email"; required when block is present
    # Slack-specific:
    webhook_url: ""              # required when channel = slack; validated as HTTPS URL
    # Email-specific:
    smtp_host: ""
    smtp_port: 587
    smtp_user: ""
    smtp_password_env: "MEMORUM_SMTP_PASSWORD"  # name of env var; NOT stored in config.yaml
    to: ""
    from: "memorum@localhost"
    triggers:
      - reality_check_due
      - daily_synthesis_summary
      - reality_check_overdue
    retry_max: 3                 # range: 0–10; default: 3
    retry_backoff_seconds: [30, 120, 600]  # must have retry_max entries or be padded with last value
    # Behavior on invalid: log error at daemon startup; disable external channel; continue
```

**Notes:**
- `smtp_password_env` stores the name of an environment variable, not the password value, to avoid writing credentials to the config file. If the env var is not set, SMTP delivery logs `ERROR: SMTP password env var <name> not set` and disables email delivery.
- `config.yaml` is owned by Stream A's config loader. Stream G keys are registered as known extensions; unrecognized top-level keys cause a config-load warning (not an error) per Stream A's `unknown fields preserved on round-trip` policy.

---

## 9. CLI surface additions

### 9.1 `memoryd ui`

**Synopsis:**

```
memoryd ui [--panel <1-8>]
```

**Description:** Launches the `ratatui`-based TUI. Replaces the current terminal session until `q` or `Ctrl-c`. Requires a terminal with ANSI support; rejects non-TTY stdin with a message: "memoryd ui requires an interactive terminal."

**Options:**
- `--panel <N>`: start with panel N active (default: 1). Range 1–8. Out-of-range: error exit with message.

**Exit codes:**
- `0`: clean exit (`q` key or `Ctrl-c`)
- `1`: failed to connect to daemon socket (printed: "Cannot connect to memoryd. Run `memoryd start` first.")
- `2`: terminal too small or not a TTY
- `3`: config error

**Examples:**

```
$ memoryd ui
$ memoryd ui --panel 2   # start on Review Queue
$ memoryd ui --panel 8   # start on Reality Check
```

### 9.2 `memoryd web enable`

**Synopsis:**

```
memoryd web enable [--port <port>]
```

**Description:** Starts the localhost web dashboard. Sets `web.enabled: true` in `config.yaml` and starts the axum server. Idempotent — if already running, prints port and URL.

**Options:**
- `--port <port>`: override port (default: 7137). Range: 1024–65535.

**Exit codes:**
- `0`: server started (or already running)
- `1`: port in use (`Address already in use` → prints: "Port 7137 is in use. Try `memoryd web enable --port 7138` or check what's listening.")
- `2`: bind_address restricted (attempt to set non-localhost address)
- `3`: daemon not running

**Output:**
```
Web dashboard enabled at http://localhost:7137
```

### 9.3 `memoryd web disable`

**Synopsis:**

```
memoryd web disable
```

**Description:** Stops the web dashboard server. Sets `web.enabled: false` in `config.yaml`. In-flight requests are drained (up to 5 seconds) before the server stops.

**Exit codes:**
- `0`: stopped (or already stopped)
- `1`: daemon not running

### 9.4 `memoryd web status`

**Synopsis:**

```
memoryd web status [--json]
```

**Output (text):**
```
Web dashboard: running
URL: http://localhost:7137
Port: 7137
Uptime: 2h 14m
Active connections: 2
```

**Output (json):**
```json
{ "running": true, "url": "http://localhost:7137", "port": 7137, "uptime_seconds": 8040, "active_connections": 2 }
```

**Exit codes:**
- `0`: status printed
- `1`: daemon not running

### 9.5 `memoryd reality-check run`

**Synopsis:**

```
memoryd reality-check run [--top-n <N>] [--namespace <ns>] [--tui] [--json]
```

**Description:** Triggers a Reality Check session. Default: interactive TUI-style prompt in the terminal. If the TUI is already open (Panel 8), `--tui` routes the session there. `--json` prints the scored list and exits without starting an interactive session (for scripting).

**Options:**
- `--top-n <N>`: override top-N for this run. Does not change config.
- `--namespace <ns>`: restrict scoring to `me`, `project`, or `agent`.
- `--tui`: emit a signal to an already-open TUI to switch to Panel 8 and start the session.
- `--json`: print the scored list as JSON, exit 0. No interactive prompts.

**Exit codes:**
- `0`: session completed or JSON printed
- `1`: no items to review (all scored memories have `skip_this_week` deferred or queue is empty)
- `2`: session abandoned (no actions taken; `Ctrl-c`)
- `3`: daemon not running

**Examples:**

```
$ memoryd reality-check run
$ memoryd reality-check run --namespace me
$ memoryd reality-check run --json | jq '.items[0]'
```

### 9.6 `memoryd reality-check skip`

**Synopsis:**

```
memoryd reality-check skip
```

**Description:** Marks the current week's Reality Check as skipped. Does not change `last_completed_at`. Prevents `reality_check_due` notifications for this week's window.

**Exit codes:**
- `0`: skipped
- `1`: no pending reality check for this week
- `2`: daemon not running

### 9.7 `memoryd reality-check snooze`

**Synopsis:**

```
memoryd reality-check snooze [--until <iso_date>]
```

**Description:** Snoozes the Reality Check. Default snooze: until next scheduled run (next Sunday). `--until <iso_date>` snoozes until a specific date.

**Exit codes:**
- `0`: snoozed (prints: "Reality Check snoozed until 2026-05-11 09:00")
- `1`: invalid date
- `2`: daemon not running

### 9.8 `/memory-reality-check` slash command (Tier 1)

Available in Claude Code and Codex CLI (Tier 1 harnesses only, §10 of system-v0.2).

**Behavior:** calls `memoryd reality-check run --json` and formats the scored list as a user-readable summary in the harness's chat interface. The agent does not see the individual memory bodies (it's an admin-surface command, not an MCP tool). Output is formatted for human reading, not for agent consumption.

**Example output in harness:**

```
## Reality Check — 12 memories to review

1. "My preferred stack is TypeScript + Rust" (me/identity, score: 0.82) — last observed 62 days ago
2. "atlasos uses Postgres 15 with CITEXT" (project:atlasos, score: 0.71) — 0 recalls in 30d
...

Run `memoryd reality-check run` or open TUI panel 8 to complete the review.
```

**If no items due:** "No Reality Check items pending. Next session: Sunday 2026-05-11."

---

## 10. Stream G acceptance tests

These are the tests that Stream G's deliverable must pass. They are unit and integration tests of Stream G's surfaces, not Stream H eval harness tests.

### 10.1 TUI tests

**`tests/panel_render.rs` — snapshot tests:**

- `test_overview_panel_renders_daemon_status`: mock daemon response with known values; assert rendered frame matches snapshot.
- `test_review_queue_renders_candidate_items`: mock review queue with 3 candidate items; assert correct item count, namespaces, confidence values in frame.
- `test_review_queue_renders_dream_low_confidence`: mock queue containing a `dream_low_confidence` item; assert `[dream]` badge rendered.
- `test_conflicts_panel_renders_side_by_side`: mock conflict with local/remote/ancestor; assert three-column diff layout.
- `test_entities_panel_search_renders_results`: mock entity search response; assert entity name and memory count visible.
- `test_timeline_panel_renders_events_by_kind`: mock event log with one of each `EventKind`; assert each renders with correct color code.
- `test_namespace_tree_renders_hierarchy`: mock memory tree with `me/`, `projects/atlasos/`, `agent/`; assert tree indentation and expand/collapse.
- `test_policy_panel_renders_active_policies`: mock policy response; assert all four policy names visible.
- `test_reality_check_panel_renders_score_breakdown`: mock scored item with known weights; assert breakdown math matches formula output.

**`tests/keymap.rs` — keymap exhaustiveness:**

- `test_all_panels_handle_panel_switch_keys`: in each of 8 panels, send key events `1`–`8`; assert App state transitions to correct panel.
- `test_quit_with_pending_actions_prompts_confirmation`: stage a review action, send `q`; assert confirmation modal opens.
- `test_escape_closes_modal`: open memory detail modal, send `Esc`; assert modal closes and underlying panel state unchanged.
- `test_undo_window_fires_before_daemon_call`: stage a review `approve`, check 1-second undo window; assert that pressing `u` within window cancels daemon call.
- `test_undo_window_expires_and_fires_daemon_call`: same setup, wait >1000 ms; assert daemon call fires.

**`tests/socket_unreachable.rs`:**

- `test_tui_shows_unreachable_state_on_socket_failure`: mock daemon returning connection refused; assert error box content and footer `socket:UNREACHABLE`.
- `test_tui_recovers_on_reconnection`: simulate unreachable then reconnected socket; assert normal rendering resumes.

**`tests/resize.rs`:**

- `test_below_minimum_shows_warning_banner`: send resize event to (79, 23); assert warning banner replaces content.
- `test_resize_above_minimum_resumes`: sequence: normal → small → normal; assert state preserved.

### 10.2 Web dashboard tests

**`tests/api_contract.rs`:**

- `test_get_status_returns_correct_shape`: mock daemon status; POST to `GET /api/status`; assert JSON shape matches spec.
- `test_get_entity_graph_returns_nodes_and_edges`: mock entity data; assert response has `nodes[]` and `edges[]`.
- `test_post_review_action_approve_calls_daemon`: mock review action; assert daemon `review_approve` call fired with correct id.
- `test_post_review_action_returns_409_on_wrong_state`: mock daemon returning "not in review state"; assert HTTP 409 with correct body.
- `test_get_audit_returns_full_trust_artifact`: mock memory with all trust artifact fields; assert all sections present.
- `test_get_audit_temporal_returns_historical_state`: mock temporal query; assert `viewing_historical_state: true` in response.
- `test_get_roi_30d_returns_correct_window`: assert `window_days: 30` in response.
- `test_get_roi_365d_returns_correct_window`: assert `window_days: 365`.

**`tests/csrf.rs`:**

- `test_post_without_csrf_header_returns_403`: send POST to `/api/review/action` without `X-Memorum-CSRF` header; assert 403.
- `test_post_with_wrong_csrf_token_returns_403`: send wrong token; assert 403.
- `test_post_with_correct_csrf_token_succeeds`: send correct token; assert non-403 response.
- `test_csrf_token_in_initial_html`: fetch `/`; assert `<meta name="csrf-token">` present in response.

**`tests/concurrent_access.rs`:**

- `test_concurrent_post_same_memory_second_returns_409`: simulate two simultaneous `POST /api/review/action` for same memory id; assert first succeeds, second gets 409.

### 10.3 Reality Check tests

**`tests/scoring.rs`:**

- `test_score_formula_staleness_only`: memory with staleness 90 days, perfect recall/corroboration/confidence/sensitivity=0; assert score ≈ 0.35.
- `test_score_formula_all_components`: known input for each component; assert final score matches formula to 4 decimal places.
- `test_score_saturation_at_90_days`: memory with 120 days staleness; assert `days_since_observed_norm` = 1.0 (not 1.33).
- `test_corroboration_requires_two_distinct_sources`: one-source memory; assert `cross_source_corroboration` = 0.
- `test_sensitivity_weights_map_correctly`: four memories with `public`, `internal`, `confidential`, `personal`; assert weights 0.0, 0.3, 0.6, 1.0.
- `test_encrypted_memory_scored_from_index_only`: memory with body hidden; assert scoring completes without body access.
- `test_top_n_selection_respects_cap`: 20 scored memories; assert only 12 returned with default `top_n`.
- `test_pinned_memories_always_included`: 12 high-score memories + 1 pinned low-score memory; assert pinned memory present in list.

**`tests/scheduling.rs`:**

- `test_due_after_7_days`: mock `last_completed_at` 8 days ago; assert `is_due()` returns true.
- `test_not_due_within_7_days`: mock 5 days ago; assert `is_due()` returns false.
- `test_snoozed_not_due`: mock due but snoozed; assert `is_due()` returns false.
- `test_overdue_after_21_days`: mock 22 days ago; assert `is_overdue()` returns true.

**`tests/responses.rs`:**

- `test_confirm_updates_observed_at_and_bumps_confidence`: mock confirm action; assert `observed_at` updated, confidence += 0.02 (capped at 1.0).
- `test_not_relevant_sets_passive_recall_false`: mock not-relevant action; assert `retrieval_policy.passive_recall = false`.
- `test_not_relevant_does_not_tombstone`: assert memory status remains `active`, no tombstone event emitted.
- `test_forget_requires_reason_minimum_length`: reason < 3 chars; assert error response, no tombstone event.
- `test_correct_issues_supersession`: mock correct with new body; assert `memory_supersede` called with correct fields.
- `test_skip_this_week_defers_without_frontmatter_mutation`: skip action; assert `deferred_this_week` updated in session state, no frontmatter write.

### 10.4 Notification tests

**`tests/dispatcher.rs`:**

- `test_passive_queue_receives_all_events`: fire each `NotificationEvent` variant; assert passive queue has one entry per event.
- `test_passive_queue_drops_oldest_when_full`: fill queue to 100; fire one more; assert first item dropped.
- `test_os_notification_not_fired_when_disabled`: send `leaked_secret_detected` with `os.enabled: false`; assert no `osascript` / `notify-send` call.
- `test_os_notification_fires_when_enabled_and_trigger_matches`: `os.enabled: true`, send matching trigger; assert OS call made.
- `test_slack_webhook_retried_on_failure`: mock Slack returning 500; assert retry up to `retry_max` times.
- `test_slack_webhook_falls_back_to_passive_on_final_failure`: exhausted retries; assert passive queue contains failure note.
- `test_slack_payload_contains_no_memory_content`: fire `reality_check_due`; assert Slack payload has no memory titles or bodies.
- `test_lagged_dispatcher_logs_warning_and_continues`: fill broadcast channel beyond capacity; assert WARN log emitted, dispatcher continues.

### 10.5 Trust artifact tests

**`tests/trust_artifact.rs`:**

- `test_all_sections_present_for_plaintext_memory`: build trust artifact for a plaintext memory; assert all 8 sections present (title/body, confidence, recall, provenance, policy decisions, privacy scan, supersession, sync state).
- `test_encrypted_memory_shows_content_redacted`: build trust artifact for encrypted memory; assert body section shows redaction notice, all other sections present.
- `test_provenance_chain_correctly_ordered`: mock events in reverse insertion order; assert chain rendered chronologically ascending.
- `test_policy_decision_expands_all_fields`: assert all 5 governance decision fields rendered (conf_floor, grounding, contradiction, tombstone, sensitivity).

---

## 11. Open questions and dogfood-tunable items

### 11.1 Drift-risk weight tuning (dogfood-tunable, not open)

The weights in §5.1 are locked as defaults by system-v0.2 §16.4. They are configurable in `config.yaml` under `reality_check.score_weights` (§8). The locked defaults ship; if the 1-week dogfood reveals that one component dominates to the point of uselessness (e.g., `staleness` 0.35 ranking 90-day-old but frequently-recalled memories too high), the config knob allows tuning before 1.0.0. The weights in the spec do not change; the config defaults do if dogfood demands it.

### 11.2 Policy editor deferral (open thread for v1.x)

The web dashboard Section 5 (policy editor) is deferred per system-v0.2 §16.3. v1 users edit policy YAML files via `$EDITOR` (TUI Panel 7 `e` key) or directly on disk. The deferred section would add:
- Syntax-highlighted YAML editor in the browser.
- Live validation (dry-run via `Policy::dry_run` API).
- Side-by-side diff of "active" vs. "on-disk" policy state.

This is useful but not required for v1 correctness. The `$EDITOR` escape in TUI Panel 7 covers the write path. Web dashboard Section 5 is a known gap in v1; it should be called out in v1 release notes.

### 11.3 Sync status dashboard (open thread for v1.x)

Web dashboard Section 6 (sync status: which devices have what, lease state, commit history) is deferred per system-v0.2 §16.3. TUI Panel 1's sync line and `git log ~/.memory/` cover the 90% case. Full multi-device sync visualization with device-by-device content divergence tracking is a natural v1.1 addition once there are multi-device dogfood results to learn from.

### 11.4 OS notification tool detection (implementation decision, not open)

On macOS, the daemon uses `osascript -e 'display notification …'`. On Linux, it uses `notify-send`. Both are detected at daemon startup by checking `$PATH`. If neither is found, `notifications.os.enabled` is silently degraded to passive with a startup warning. No fallback to terminal bell or other mechanism — those would be surprising and potentially noisy in automated contexts.

### 11.5 Entity graph rendering for large corpora

The §12 performance budget requires the entity graph to handle 5,000 nodes. D3 force simulation on 5,000 nodes in a browser tab will require careful optimization: frozen simulation after initial stabilization, level-of-detail culling (edges below 0.3 weight hidden by default), and cluster-first rendering (namespace clusters layout before individual nodes). If dogfood shows that the graph is unusable above ~500 entities in practice, the v1 release note should document the effective limit and defer the large-corpus optimization to v1.1.

The server-side `GET /api/entity-graph` response is paginated when `depth=1` and node count exceeds 1,000: additional nodes are loaded lazily via `GET /api/entity-graph?cursor=<token>`. Depth 2+ returns the full graph up to 5,000 nodes; beyond 5,000 nodes, the response is truncated with a `truncated: true` flag and a count of omitted nodes.

### 11.6 Email notification authentication

SMTP password storage via env var (`smtp_password_env`) covers the basic case. For environments where env vars are also sensitive (e.g., macOS Keychain, systemd credential storage), v1.1 should add `smtp_password_keychain_item` as an alternative to `smtp_password_env`. Not blocking for v1 — the env var approach is standard and secure for daemon services.

### 11.7 Reality Check slash command agent-visible output

The `/memory-reality-check` slash command (§9.8) prints a summary for the human. Open question: should the output include memory titles (which could surface private memory content in the harness's visible context window)? Decision for v0.1: yes, titles are shown, because (a) the harness context is already trusted (Tier 1), (b) `memoryd reality-check run --json` — which backs this command — applies `safe_plaintext_fragment` on the title before outputting, and (c) omitting titles makes the summary useless. If Stream D's safe_plaintext_fragment returns `OmitEncryptedBodyHidden` for a title, the item is rendered as `[encrypted item, score: X.XX]` with no title.

### 11.8 `GET /api/audit/:id/walk` — deeper audit-walk provenance traversal (deferred v1.1+)

`GET /api/audit/:id/walk?direction=up|down&depth=<int>` (§4.3, `ProvenanceWalkResponse`) was shipped as a 501 Not Implemented stub by the Codex 12-hour implementation run. The route and its response type are defined; the backend traversal logic (walking the `events_log` mirror and `memory_supersession` join table to build a multi-hop provenance DAG) was scoped out due to the complexity of reliable recursive graph walks within the 12-hour constraint.

**v1 behavior:** The route returns `{"status": "not_implemented", "route": "audit_walk"}` with HTTP 501. The dashboard "Walk provenance" button is visible in the UI but triggers this 501; users see a "Provenance walk not yet available" message in the dashboard. No silent failure.

**v1.1+ scope:** Implement the full provenance walk using the bounded CTE shape from §5.1 (`distinct_sources`), extended to walk `EventKind` provenance events and supersession edges simultaneously. The response shape (`ProvenanceWalkResponse` with `nodes[]` and `edges[]`) is already defined and stable.

---

## 12. Performance budgets

### 12.1 TUI render performance

| Metric | Budget | Measurement |
|---|---|---|
| Input-to-render latency (key press to visible frame change) | ≤16 ms (1 frame at 60 fps) | TUI synthetic benchmark: N key events pumped; measure total wall time / N |
| Panel switch (key `1`–`8` to new panel first render) | ≤16 ms | Same benchmark; panel switch variant |
| Daemon poll round-trip (250 ms interval) | Must not block render loop | Verify via async poll: render tick fires while poll awaits; no frame skip |
| Memory detail modal open (Enter on review queue item) | ≤32 ms | Includes trust artifact data fetch from daemon |
| Entity search typeahead (keystroke to results visible) | ≤100 ms | Includes 100 ms debounce + daemon FTS query round-trip |
| Resize redraw | ≤32 ms | Resize event to stable frame |

**Failure mode on budget miss:** log a WARNING to the daemon log at `WARN` level: "TUI render budget exceeded: <op> took <ms> ms". No error state; no crash. Budget misses above 3× (48 ms) are logged at `ERROR`.

**Synthetic benchmark caveat (v1 baseline):** The canonical baseline entries `tui_panel_switch` and `tui_detail_modal_open` in `bench/stream-g-observability-results.darwin-arm64.json` measure in-process state transitions against a synthetic 144-byte mock frame, not a full ratatui render path with a real terminal backend and live daemon socket responses. Measured times (≤0.001 ms p95) reflect the state-machine cost only and will not catch regressions in the ratatui buffer-diff or crossterm write paths. The entries remain in the baseline as smoke-test regression detectors for the in-process render logic. A real-load TUI bench (terminal-emulator integration, realistic frame sizes, mock daemon socket) is deferred to v1.1+. TODO(v1.1): replace `tui_panel_switch` and `tui_detail_modal_open` with full-fidelity terminal-emulator integration bench using e.g. a headless vt100 backend or ratatui's TestBackend with realistic content sizes.

### 12.2 Web dashboard performance

| Metric | Budget | Measurement |
|---|---|---|
| Initial page load and paint (cold, localhost) | ≤500 ms | `curl` time to first byte + browser paint from Lighthouse in localhost mode |
| Total asset size (gzipped) | ≤50 KB | `wc -c` of gzipped `app.js` + `style.css` combined |
| API route p99 latency (under no load, localhost) | ≤50 ms | Integration test with mock daemon responses |
| `GET /api/entity-graph` with 5,000 nodes | ≤200 ms | Server-side serialization; measured in integration test |
| Entity graph initial force-layout stabilization | ≤3 seconds at 5,000 nodes | Browser benchmark using D3 simulation tick count |
| Entity graph interactive (pan/zoom/click after stabilization) | 60 fps | Manual verification during dogfood |

### 12.3 Reality Check scoring

| Metric | Budget | Measurement |
|---|---|---|
| Score computation for 10,000 memories | ≤500 ms | Benchmark test in `tests/scoring.rs` |
| Top-N selection (sort + take) | ≤50 ms on top of scoring | Same benchmark |
| Session resume from persisted state | ≤100 ms | Measures deserialization of `reality-check-session.json` |

**Implementation note:** the scoring loop must avoid calling `Substrate::read_memory` per item. All scoring inputs (`observed_at`, `confidence`, `sensitivity`) come from `memories` index columns; `recall_count_30d` and `distinct_sources` come from the events_log table via the covering index added in §1.3 #2 (`events_log(kind, memory_id, ts)`). For 10,000 memories, this is 1 GROUP-BY-and-FILTER scan over the events log per metric (2 scans total) plus one row-level scan over `memories` for the static fields — well within the 500 ms budget. **Pre-aggregation is acceptable but not required**: a per-day rollup table keyed by `memory_id` could further speed scoring if dogfood reveals the events-log scan dominates, but is not part of v1's spec contract.

No new column is added to the `memories` table. (Earlier draft proposed `source_count INTEGER NOT NULL DEFAULT 1` — dropped per system-v0.2 §19's authorization table; the events log + covering index supplies the same data without duplicating it.)

### 12.4 Notification dispatcher

| Metric | Budget | Measurement |
|---|---|---|
| Passive queue append latency | ≤1 ms | In-process benchmark; no I/O |
| Slack webhook dispatch (first attempt, assuming network available) | ≤2 seconds | Integration test with local HTTP mock |
| Slack retry with exponential backoff (3 retries, final failure) | ≤(30+120+600) = 750 seconds | Property test: mock always-fail webhook; measure total retry duration |

---

*End of Stream G Observability Spec v0.1.*
