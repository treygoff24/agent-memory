# Alpha Core Gap Closeout Implementation Plan

> **Status: 9 Done + 1 Partial (Task 10, pending live-smoke gate) as of 2026-05-26.** Audit closeout at `docs/reviews/2026-05-26-alpha-gap-audit.md` with per-task verdicts and file:line evidence. Tasks 1, 2, 3, 4, 5, 6, 7, 8, 9 are Done. Task 4's frontend partials (`Entities.tsx normalizeKind`, `Peers.tsx normalizePeer`, plus `TrustLedger.tsx` render-site nulls and `TrafficCard.tsx` event-counter passthrough) were fixed in the 2026-05-26 pre-dogfood cleanup pass; the daemon-side `dashboard/status.rs` + `dashboard/entities.rs` architecture deviation was accepted (see Architecture note in Task 4 below). Task 10 stays Partial until the live dogfood smoke is walked end-to-end on a clean machine.

**Goal:** Close the alpha-readiness gaps found in the May 25 readiness audit, excluding the semantic/model privacy-filter item, so Memorum's dogfood surfaces are either genuinely daemon-backed or explicitly unavailable.

**Architecture:** Treat `memoryd` as the source of truth. Add protocol DTOs only where the daemon has real state to expose, keep web/TUI adapters thin, and remove fixture/heuristic UI behavior from daemon mode. Use vertical TDD: one public behavior test, minimal implementation, then refactor into small Rust modules instead of growing `handlers/mod.rs`.

**Tech Stack:** Rust 2021, Tokio, Axum, serde JSON protocol, memory-substrate/event log, memory-governance policies, memory-source capture, React/TypeScript dashboard, Vitest/Playwright.

---

## Scope

### Included gaps

1. Dashboard ROI endpoint deferred in daemon mode.
2. Dashboard notifications stream returns empty heartbeat in daemon mode.
3. Visible web controls are inert or not connected.
4. Dashboard/TUI status and detail views still use synthetic or heuristic fields in daemon mode.
5. Source grounding is too narrow at the daemon/MCP/CLI boundary.
6. Eval/release confidence still has deferred/mock-only semantic coverage. This is audit item #7.
7. Policy editor is read-only or not visibly connected in default daemon-backed web mode. This is audit item #8.

### Explicitly excluded

- Audit item #6, optional semantic/model privacy filter. For alpha, deterministic-first privacy is accepted.

### Non-negotiable invariants

- MCP remains local-first: `memoryd mcp --socket <PATH>`.
- `memory_reveal` stays hidden/blocked by default on MCP stdio unless `--allow-reveal`.
- No dashboard route may silently fall back to fixture data when `WebState::daemon(...)` is active.
- Fixture mode remains test/demo-only and must be visually/programmatically distinguishable.
- Policy edits must validate against the full policy set before atomically writing.
- Source capture must never persist unsafe plaintext excerpts or raw sensitive text.
- External CLI/MCP/web source-capture requests must never accept client-controlled key paths or privacy bypass flags; the daemon derives runtime key paths internally.
- Release/eval skips must be honest: no "passed" result for an unexercised semantic path.
- Deterministic privacy checks remain in scope for alpha; model/semantic privacy classification remains out of scope by explicit decision.

### Execution discipline

Every task below lists a behavior inventory, not permission to create a broad red state. Execute each behavior as a vertical TDD slice:

1. add one failing public behavior test;
2. run the narrow focused command and confirm the intended failure;
3. implement the smallest daemon/UI/docs change that makes that test pass;
4. rerun the focused command green;
5. refactor only while green, then move to the next behavior.

If a task lists multiple assertions, do not add all of those tests before implementation. This is especially important for protocol enum work, where one broad compile break can block unrelated streams.

### Gap-to-task traceability

| Gap | Primary tasks | Acceptance signal |
| --- | --- | --- |
| ROI endpoint deferred | Tasks 1, 2, 10 | Daemon-backed `/api/roi` returns `200` with live/zero metrics, never fixture/deferred data. |
| Notifications stream empty | Tasks 1, 3, 10 | SSE includes daemon passive notifications and exposes daemon errors honestly. |
| Inert visible controls | Tasks 1, 6, 10 | Every clickable control either performs a tested action or is disabled with accessible explanatory copy. |
| Synthetic dashboard/TUI fields | Tasks 1, 4, 10 | Daemon mode derives values from daemon/substrate state or labels them unknown/unavailable. |
| Source grounding too narrow | Tasks 1, 5, 8, 10 | CLI/MCP/daemon support alpha capture modes and return typed unsupported errors for rich capture. |
| Deferred semantic/eval coverage | Tasks 9, 10 | Required alpha eval set has no deferred required cases and release CI fails on missing coverage. |
| Policy editor read-only/unmounted | Tasks 1, 7, 10 | Settings exposes policy editing; daemon-backed GET is writable/complete when allowed, POST validates, persists atomically, and refreshes UI. |

---

## Execution order

1. Task 1 adds the protocol/API contracts and failing contract tests.
2. Tasks 2-5 implement daemon-backed data surfaces.
3. Tasks 6-8 wire UI/TUI and policy/source write paths.
4. Task 9 closes eval/release confidence.
5. Task 10 updates docs/gates and performs integrated verification.

Parallelism note: after Task 1, Tasks 2, 3, 5, 7, and 9 are conceptually independent, but they touch shared daemon protocol/handler integration. If executed in parallel, give each worker a separate worktree and route all edits to `crates/memoryd/src/protocol.rs` and `crates/memoryd/src/handlers/mod.rs` through one coordinator. Do not let parallel workers independently edit those two files.

Known duplicate ownership is intentional only in these cases:
- `crates/memoryd/src/handlers/mod.rs`: coordinator-owned integration file for Tasks 2, 3, 4, 5, and 7.
- `crates/memoryd/src/dashboard/mod.rs`: Task 2 creates the module shell; Task 4 adds status/entities exports after Task 2 lands.
- Frontend API helpers: Task 1 adds shared types/hooks; Task 7 consumes/refines only policy-editor helpers after Task 1 lands.
- `crates/memoryd-web/src/routes/status.rs`: Task 3 owns notification SSE behavior; Task 4 owns dashboard status fields and is not parallel.
- `crates/memoryd-web/frontend/src/views/Peers.tsx`: Task 4 removes heuristic data; Task 6 later disables or wires the pair CTA.
- `crates/memoryd/src/cli.rs` and `crates/memoryd/src/main.rs`: Task 5 owns source-capture CLI/MCP wiring; Task 9 owns device-key/eval CLI wiring. If both run in parallel, route those two files through the coordinator or split patches by subcommand with explicit merge review.
- Docs/gates: Task 8 owns public docs reconciliation and Task 10 owns final gate-script cleanup. If Tasks 5 or 9 run in parallel and discover doc/gate changes, they should leave coordinator patch notes or a separate non-overlapping patch rather than editing shared docs/gates independently.

---

### Task 1: Protocol and contract scaffolding

**Parallel:** no  
**Blocked by:** none  
**Owned files:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd-web/frontend/src/api/types.ts`, `crates/memoryd-web/frontend/src/api/queries.ts`, `crates/memoryd-web/frontend/src/api/mutations.ts`  
**Invariants:** preserve existing serialized request/response names; additive protocol changes only unless a test proves a breaking cleanup is required.  
**Out of scope:** implementing metrics, policy persistence, source extraction, or UI behavior.

**Files:**
- Modify: `crates/memoryd/src/protocol.rs`
- Modify: `crates/memoryd/tests/protocol_contract.rs`
- Modify: `crates/memoryd-web/frontend/src/api/types.ts`
- Modify: `crates/memoryd-web/frontend/src/api/queries.ts`
- Modify: `crates/memoryd-web/frontend/src/api/mutations.ts`

**Step 1: Define protocol behavior inventory and take the first red slice**

Add these protocol-contract behaviors one at a time, following the execution discipline above:
- `RequestPayload::DashboardRoi { window_days }` -> `ResponsePayload::DashboardRoi(DashboardRoiResponse)`.
- `RequestPayload::NotificationsRecent { limit }` -> `ResponsePayload::NotificationsRecent(NotificationsRecentResponse)`.
- `RequestPayload::PolicyValidate { raw_yaml, file_name }` and `RequestPayload::PolicyWrite { raw_yaml, file_name }`.
- `RequestPayload::Search { ... }` already exists; only add a web API type and query hook for `/api/search`.
- Extend `CaptureSource` with a typed external payload struct rather than loose fields: `SourceCapturePayload { source, mode, excerpts, note, local_path }`.
- Assert source-capture protocol/MCP rejects any user-supplied `key_path`, raw key material, or bypass flag. If `memory-source` needs a key path internally, populate it only inside daemon/runtime adapter code.
- Add `DashboardSearchResponse`, `NotificationSnapshot`, `DashboardRoiResponse`, `PolicyEditorPostResponse`, and source-capture mode types to frontend API types.

Start with the ROI round-trip only, get it green, then repeat for notifications, policy, source capture, and frontend API types.

**Step 2: Run focused failing tests**

Run:

```bash
cargo test -p memoryd --test protocol_contract -- --nocapture
pnpm --dir crates/memoryd-web/frontend run typecheck
```

Expected before each slice implementation: the single new protocol/frontend behavior fails or does not compile for the intended missing variant/type. Do not proceed to the next behavior until this slice is green.

**Step 3: Add minimal protocol DTOs**

Recommended Rust shapes:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSourceMode {
    HttpStatic,
    LocalArtifact,
    PdfText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapturePayload {
    pub source: String,
    pub mode: CaptureSourceMode,
    pub excerpts: Vec<String>,
    pub note: Option<String>,
    pub local_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardRoiResponse {
    pub window_days: u16,
    pub promotion_rate: f64,
    pub promotion_precision: f64,
    pub refusal_breakdown: BTreeMap<String, u32>,
    pub dreaming: DreamingRoiSummary,
    pub reality_check_adherence: RealityCheckAdherenceSummary,
}
```

Use concrete DTOs rather than `serde_json::Value` so downstream routes cannot fake shapes.

**Step 4: Run tests green**

Run:

```bash
cargo test -p memoryd --test protocol_contract -- --nocapture
pnpm --dir crates/memoryd-web/frontend run typecheck
```

Expected: protocol round trips pass; frontend still may have unused hooks until later tasks.

**Verification plan:**
- Primary: `cargo test -p memoryd --test protocol_contract -- --nocapture`
- Secondary: `pnpm --dir crates/memoryd-web/frontend run typecheck`

---

### Task 2: Real daemon-backed ROI metrics

**Parallel:** yes after Task 1 if protocol/handler edits are coordinator-owned  
**Blocked by:** Task 1  
**Owned files:** `crates/memoryd/src/dashboard/mod.rs`, `crates/memoryd/src/dashboard/roi.rs`, `crates/memoryd/src/lib.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd-web/src/routes/roi.rs`, `crates/memoryd-web/tests/dashboard_endpoints.rs`, `crates/memoryd/tests/dashboard_roi.rs`  
**Invariants:** `/api/roi` in daemon mode must never return HTTP 501; fixture ROI may remain only under `fixture_router()`.  
**Out of scope:** adding new event kinds unless a metric cannot be derived from existing events/statuses.

**Files:**
- Create: `crates/memoryd/src/dashboard/mod.rs`
- Create: `crates/memoryd/src/dashboard/roi.rs`
- Create: `crates/memoryd/tests/dashboard_roi.rs`
- Modify: `crates/memoryd/src/lib.rs`
- Modify: `crates/memoryd/src/handlers/mod.rs`
- Modify: `crates/memoryd-web/src/routes/roi.rs`
- Modify: `crates/memoryd-web/tests/dashboard_endpoints.rs`

**Step 1: Define ROI behavior inventory and take the first red slice**

Add these seeded-test behaviors one at a time:
- promoted/active memories,
- candidate/quarantined memories,
- refused write events where available,
- dream run reports,
- Reality Check history events.

Expected assertions:
- `RequestPayload::DashboardRoi { window_days: 90 }` returns nonzero aggregates from seeded repo/event log.
- `GET /api/roi?window=90` against `WebState::daemon(socket)` returns `200`, not `501`.
- Empty repo returns zeros plus no fake fixture values.

**Step 2: Run focused failing tests**

```bash
cargo test -p memoryd --test dashboard_roi -- --nocapture
cargo test -p memoryd-web --test dashboard_endpoints test_get_roi_daemon_returns_live_metrics_not_deferred_stub -- --nocapture
```

Expected before implementation: unknown request or HTTP 501.

**Step 3: Implement daemon metrics module**

Create the `crates/memoryd/src/dashboard/` directory and `crates/memoryd/src/dashboard/mod.rs` with:

```rust
pub mod roi;
```

Add `pub mod dashboard;` to `crates/memoryd/src/lib.rs`.

Implement `crates/memoryd/src/dashboard/roi.rs` with small query functions:
- `promotion_counts(substrate, since)`.
- `refusal_breakdown(substrate, since)`.
- `dreaming_roi(repo, since)`.
- `reality_check_adherence(substrate, since)`.

Keep each function side-effect-free and testable. Put numeric downcasts behind a named helper such as `fn usize_to_u32_saturating(value: usize) -> u32 { u32::try_from(value).unwrap_or(u32::MAX) }`.

**Step 4: Wire handler and web route**

- In `handlers/mod.rs`, route `RequestPayload::DashboardRoi` to the ROI module.
- In `routes/roi.rs`, replace daemon-mode `deferred_response("roi")` with daemon request forwarding and `daemon_error(...)` on failure.

**Step 5: Run tests green**

```bash
cargo test -p memoryd --test dashboard_roi -- --nocapture
cargo test -p memoryd-web --test dashboard_endpoints -- --nocapture
```

**Verification plan:**
- Primary: focused tests above.
- Secondary: `cargo test -p memoryd-web --test api_contract -- --nocapture`.

---

### Task 3: Daemon-backed notifications stream

**Parallel:** yes after Task 1 if protocol/handler edits are coordinator-owned  
**Blocked by:** Task 1  
**Owned files:** `crates/memoryd/src/notifications/passive.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd-web/src/routes/status.rs`, `crates/memoryd-web/tests/api_contract.rs`, `crates/memoryd-web/frontend/src/api/notifications.ts`, `crates/memoryd-web/frontend/tests/unit/notifications.test.ts`  
**Invariants:** missed critical notifications must be recoverable from a bounded daemon queue; SSE heartbeat stays valid `text/event-stream`.  
**Out of scope:** guaranteed external delivery to SMTP/Slack; this task only wires local daemon/dashboard visibility.

**Files:**
- Modify: `crates/memoryd/src/notifications/passive.rs`
- Modify: `crates/memoryd/src/handlers/mod.rs`
- Modify: `crates/memoryd-web/src/routes/status.rs`
- Modify: `crates/memoryd-web/tests/api_contract.rs`
- Modify: `crates/memoryd-web/frontend/src/api/notifications.ts`
- Modify: `crates/memoryd-web/frontend/tests/unit/notifications.test.ts`

**Step 1: Define notification behavior inventory and take the first red slice**

Add these tests one behavior at a time:
- Emit `NotificationEvent::ReviewQueueOverThreshold` and assert `RequestPayload::NotificationsRecent { limit: 50 }` returns it.
- Start web router with daemon state and assert `/api/notifications/stream` includes emitted daemon notification in SSE payload.
- Frontend store receives a daemon notification and keeps prior copy on temporary reconnect without duplicating notifications.

**Step 2: Run focused failing tests**

```bash
cargo test -p memoryd --test notification_channel notifications_recent_returns_passive_queue -- --nocapture
cargo test -p memoryd-web --test api_contract test_notifications_stream_returns_daemon_notifications -- --nocapture
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/unit/notifications.test.ts
```

**Step 3: Implement recent notification protocol**

- Convert existing passive queue entries into `NotificationSnapshot` with a stable `id`; if historical entries cannot have IDs, define deterministic dedupe as `(created_at, kind, subject)`.
- Add `RequestPayload::NotificationsRecent`.
- Keep broadcast for live process-internal events, but let web stream call daemon recent queue before each heartbeat.

**Step 4: Wire web SSE**

Replace `Vec::new()` daemon path in `notifications_stream` with daemon request forwarding. If daemon request fails, return a heartbeat with `error` and empty notifications rather than silently claiming no notifications. Assert the SSE path does not re-append the same recent queue item on every heartbeat/reconnect.

**Step 5: Run tests green**

```bash
cargo test -p memoryd --test notification_channel notifications_recent_returns_passive_queue -- --nocapture
cargo test -p memoryd-web --test api_contract test_notifications_stream_returns_daemon_notifications -- --nocapture
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/unit/notifications.test.ts
```

**Verification plan:**
- Primary: notification focused Rust/frontend tests.
- Secondary: manual `curl -N http://127.0.0.1:7137/api/notifications/stream` in live smoke.

---

### Task 4: Replace synthetic dashboard/TUI status and detail fields

**Parallel:** no; touches broad status and UI data contracts  
**Blocked by:** Tasks 1, 2  
**Owned files:** `crates/memoryd/src/dashboard/mod.rs`, `crates/memoryd/src/dashboard/status.rs`, `crates/memoryd/src/dashboard/entities.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd-web/src/routes/status.rs`, `crates/memoryd-web/src/routes/entity_graph.rs`, `crates/memoryd-web/src/routes/sync_dashboard.rs`, `crates/memoryd-web/frontend/src/views/Entities.tsx`, `crates/memoryd-web/frontend/src/views/Peers.tsx`, `crates/memoryd-tui/src/client.rs`, `crates/memoryd-tui/src/app.rs`, `crates/memoryd-tui/tests/inbox_render.rs`  
**Invariants:** daemon mode must prefer "unknown/unavailable" over invented values; fixture mode may keep demo data.  
**Out of scope:** full multi-device pairing UX; this task displays real active sessions and locks only.

**Files:**
- Modify: `crates/memoryd/src/dashboard/mod.rs`
- Create: `crates/memoryd/src/dashboard/status.rs`
- Create: `crates/memoryd/src/dashboard/entities.rs`
- Modify: `crates/memoryd/src/handlers/mod.rs`
- Modify: `crates/memoryd-web/src/routes/status.rs`
- Modify: `crates/memoryd-web/src/routes/entity_graph.rs`
- Modify: `crates/memoryd-web/src/routes/sync_dashboard.rs`
- Modify: `crates/memoryd-web/frontend/src/views/Entities.tsx`
- Modify: `crates/memoryd-web/frontend/src/views/Peers.tsx`
- Modify: `crates/memoryd-tui/src/client.rs`
- Modify: `crates/memoryd-tui/src/app.rs`
- Modify: `crates/memoryd-tui/tests/inbox_render.rs`

**Step 1: Define dashboard/TUI behavior inventory and take the first red slice**

Add these tests one behavior at a time:
- `/api/status` uses real `index_stats`, `review_queue_counts`, conflict count, peer sessions, and compact dream status.
- `/api/status` does not hardcode sync/dream counters without warning; unknown fields are `null` or warning-labelled.
- `/api/entity-graph/{entity_id}` returns actual related memory IDs and no fabricated `firstSeen`, `confidence`, or fake recent IDs.
- `/api/sync-dashboard` derives peer sessions and claim locks from daemon `PeerStatus`.
- TUI snapshot includes live dream status, Reality Check due items, and memory/entity summaries when daemon supports them; otherwise panels say "unavailable" rather than showing sample rows.

**Step 2: Run focused failing tests**

```bash
cargo test -p memoryd-web --test dashboard_endpoints -- --nocapture
cargo test -p memoryd-web --test entity_endpoints -- --nocapture
cargo test -p memoryd-tui --test inbox_render -- --nocapture
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/views/Entities.test.tsx tests/views/Peers.test.tsx
```

**Step 3: Implement daemon dashboard modules**

- `dashboard/status.rs`: git sync summary, current commit, remote, last push if derivable; dream promoted/queued/dropped from latest dream report; warning entries when not derivable.
- `dashboard/entities.rs`: entity detail DTO from `InspectEntities`, recent memory IDs, memory summaries by reading memory envelopes, first/last seen from event log where available.

Do not shell out to git in request hot paths without a timeout. Prefer existing substrate/git helpers; if shelling out is unavoidable, isolate it in one helper with a short timeout and explicit warning on failure.

**Step 4: Remove frontend heuristics**

- `Entities.tsx`: consume daemon detail response for confidence/dates/recent memories or show "unknown".
- `Peers.tsx`: replace trust inference from string matching with daemon/session fields; if no trust field exists, label "local active", "stale", or "unknown" based on heartbeat age only.
- `sync_dashboard.rs`: remove hardcoded `last_commit: None` where current commit can be derived.

**Step 5: Wire TUI panels**

- Fetch `DreamStatus`, `RealityCheck(List)`, `InspectEntities`, `NamespaceTree`, and `EventsLogPage` in `DaemonClient::fetch_snapshot`.
- Stop swallowing unexpected Reality Check variants into default state; return a visible client error row.
- Keep `DaemonSnapshot::sample()` only for tests/demo constructors, never for failed live fetches.

**Step 6: Run tests green**

```bash
cargo test -p memoryd-web --test dashboard_endpoints -- --nocapture
cargo test -p memoryd-web --test entity_endpoints -- --nocapture
cargo test -p memoryd-tui --test inbox_render -- --nocapture
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/views/Entities.test.tsx tests/views/Peers.test.tsx
```

**Verification plan:**
- Primary: focused web/TUI tests.
- Secondary: `bash scripts/check-dogfood.sh`.

> **Architecture note (2026-05-26):** The daemon-side `crates/memoryd/src/dashboard/status.rs` and `crates/memoryd/src/dashboard/entities.rs` modules called for in this task were not backfilled. Route logic lives directly in `crates/memoryd-web/src/routes/{status,entity_graph,sync_dashboard}.rs` instead. `crates/memoryd/src/dashboard/mod.rs` declares only `pub mod roi;`. Behavior matches the spec — `/api/status` and `/api/sync-dashboard` derive from live daemon state with no invented facts. Module layout diverges from the plan's recommendation. Decision accepted 2026-05-26: the routes-as-thin-adapters pattern is a reasonable layout for the current scope; backfilling daemon-side modules solely for plan-parity would be shuffling code without behavior change.

---

### Task 5: Broaden source grounding at daemon/MCP/CLI boundary

**Parallel:** yes after Task 1 if protocol/handler edits are coordinator-owned  
**Blocked by:** Task 1  
**Owned files:** `crates/memory-source/src/adapters.rs`, `crates/memory-source/src/extract.rs`, `crates/memory-source/src/capture.rs`, `crates/memory-source/src/model.rs`, `crates/memory-source/tests/http_capture.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/mcp.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd/tests/source_capture_contract.rs`, `crates/memoryd/tests/mcp_manifest.rs`  
**Invariants:** alpha supports deterministic text extraction and honest unsupported errors; it must not pretend to support screenshots/authenticated browser state without a text artifact.  
**Out of scope:** semantic privacy filter provider; OCR; persistent browser session capture.

**Files:**
- Modify: `crates/memory-source/src/adapters.rs`
- Modify: `crates/memory-source/src/extract.rs`
- Modify: `crates/memory-source/src/capture.rs`
- Modify: `crates/memory-source/src/model.rs`
- Modify: `crates/memory-source/tests/http_capture.rs`
- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/main.rs`
- Modify: `crates/memoryd/src/mcp.rs`
- Modify: `crates/memoryd/src/handlers/mod.rs`
- Modify: `crates/memoryd/tests/source_capture_contract.rs`
- Modify: `crates/memoryd/tests/mcp_manifest.rs`
- Leave source docs notes for Task 8 if behavior changes.

**Step 1: Define source-capture behavior inventory and take the first red slice**

Add these tests one behavior at a time:
- `memoryd source capture --file note.md --excerpt ...` works through daemon and writes `local_artifact_v1`.
- `memory_capture_source` accepts `mode: "local_artifact"` only when `local_path` is present and rejects path traversal, sensitive excerpt text, `key_path`, raw key material, and bypass flags.
- PDF input returns either real `pdf_text_v1` refs when an existing safe extraction path is present, or a typed `unsupported` error that instructs the user to export text/html; no fake successful PDF capture.
- Unsupported screenshot/image returns a typed unsupported error, not a fake successful capture.
- HTTP static behavior remains unchanged.

**Step 2: Run focused failing tests**

```bash
cargo test -p memory-source --test http_capture local_artifact_capture_records_local_semantics_without_http_metadata -- --nocapture
cargo test -p memoryd --test source_capture_contract -- --nocapture
cargo test -p memoryd --test mcp_manifest mcp_manifest_memory_capture_source_schema_has_modes -- --nocapture
```

**Step 3: Implement supported alpha modes**

- Surface existing `CaptureMode::LocalArtifact` through daemon CLI and MCP.
- Default alpha requirement: local text/html artifact capture plus unchanged HTTP static capture.
- Add `CaptureMethod::PdfTextV1` and `CaptureMode::PdfText` only if an existing safe dependency/path is already present and cheap to verify. If not, implement PDF as explicit unsupported and document that alpha supports local text/html imports only.
- Keep `BrowserRendered`, screenshots, and authenticated capture as explicit `unsupported` modes with actionable copy: "save/export a text/html/PDF artifact and import with --file".

**Step 4: Wire CLI/MCP**

- CLI: `memoryd source capture --url ...` remains default HTTP static.
- CLI: `memoryd source capture --file PATH --mode local-artifact`.
- CLI: `memoryd source capture --file PATH --mode pdf-text` only if PDF text support is implemented; otherwise this mode must fail with typed unsupported copy.
- MCP schema: include `mode`, `source`, and `local_path`; reject `key_path`, raw key material, unknown modes, and unknown bypass flags.

**Step 5: Run tests green**

```bash
cargo test -p memory-source --test http_capture local_artifact_capture_records_local_semantics_without_http_metadata -- --nocapture
cargo test -p memoryd --test source_capture_contract -- --nocapture
cargo test -p memoryd --test mcp_manifest mcp_manifest_memory_capture_source_schema_has_modes -- --nocapture
```

**Verification plan:**
- Primary: source and daemon source-capture tests.
- Secondary: `cargo test -p memoryd --test governance_web_capture -- --nocapture`.

---

### Task 6: Make visible web controls functional or explicitly disabled

**Parallel:** yes after Tasks 2-4 where UI depends on daemon routes  
**Blocked by:** Tasks 2, 3, 4  
**Owned files:** `crates/memoryd-web/src/server.rs`, `crates/memoryd-web/src/routes/search.rs`, `crates/memoryd-web/frontend/src/shell/TopBar.tsx`, `crates/memoryd-web/frontend/src/views/Recall.tsx`, `crates/memoryd-web/frontend/src/views/Peers.tsx`, `crates/memoryd-web/frontend/src/inspector/Inspector.tsx`, `crates/memoryd-web/frontend/src/views/inboxView/layouts/TwoPane.tsx`, `crates/memoryd-web/frontend/src/views/inboxView/layouts/ThreePane.tsx`, `crates/memoryd-web/frontend/src/views/inboxView/layouts/Drawer.tsx`, `crates/memoryd-web/frontend/src/views/inboxView/layouts/ModalSheet.tsx`, `crates/memoryd-web/frontend/tests/unit/shell.test.tsx`, `crates/memoryd-web/frontend/tests/views/recall.test.tsx`, `crates/memoryd-web/frontend/tests/views/Peers.test.tsx`, `crates/memoryd-web/frontend/tests/inbox/Inbox.test.tsx`  
**Invariants:** no clickable control may silently do nothing; if not implemented for alpha, it must be disabled with accessible explanatory copy.  
**Out of scope:** full device-pairing workflow unless a daemon protocol already exists.

**Files:**
- Create: `crates/memoryd-web/src/routes/search.rs`
- Modify: `crates/memoryd-web/src/server.rs`
- Modify: `crates/memoryd-web/frontend/src/shell/TopBar.tsx`
- Modify: `crates/memoryd-web/frontend/src/views/Recall.tsx`
- Modify: `crates/memoryd-web/frontend/src/views/Peers.tsx`
- Modify: `crates/memoryd-web/frontend/src/inspector/Inspector.tsx`
- Modify: `crates/memoryd-web/frontend/src/views/inboxView/layouts/*.tsx`
- Modify tests listed above.

**Step 1: Define visible-control behavior inventory and take the first red slice**

Add these tests one behavior at a time:
- Top-bar search submits `/api/search?q=...` and renders command/search results, or stays disabled only if `/api/search` route is not present.
- Recall `export csv` downloads a CSV containing visible rows.
- Peers pair button is disabled with `aria-disabled` and tooltip if no pairing API exists.
- Inbox inspector approve/reject/forget buttons call `useReviewActionMutation`; unsupported edit action is disabled.

**Step 2: Run focused failing tests**

```bash
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/unit/shell.test.tsx tests/views/recall.test.tsx tests/views/Peers.test.tsx tests/inbox/Inbox.test.tsx
cargo test -p memoryd-web --test api_contract test_search_route_forwards_to_daemon -- --nocapture
```

**Step 3: Implement controls**

- `/api/search`: forward daemon `RequestPayload::Search { include_body: false }`.
- TopBar: wire input to `/api/search`, debounce, show results or navigate to Recall.
- Recall: generate CSV from current visible rows in the browser; no daemon write needed.
- Peers: disable pair CTA for alpha with explicit copy unless a real pairing route is added in Task 4.
- Inbox: pass `onAction` through layout props to `Inspector`; map approve/reject/forget to existing review/forget routes. Disable edit until a policy-safe edit flow exists.

**Step 4: Run tests green**

```bash
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/unit/shell.test.tsx tests/views/recall.test.tsx tests/views/Peers.test.tsx tests/inbox/Inbox.test.tsx
cargo test -p memoryd-web --test api_contract test_search_route_forwards_to_daemon -- --nocapture
```

**Verification plan:**
- Primary: Vitest focused suite.
- Secondary: Playwright smoke for keyboard/search/export if browser tooling is available.

---

### Task 7: Daemon-backed, visible policy editor writes

**Parallel:** yes after Task 1 if protocol/handler edits are coordinator-owned  
**Blocked by:** Task 1  
**Owned files:** `crates/memoryd/src/policy_editor.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd-web/src/routes/policy_editor.rs`, `crates/memoryd-web/frontend/src/views/Settings.tsx`, `crates/memoryd-web/frontend/src/views/settings/PolicyEditorTab.tsx`, `crates/memoryd-web/frontend/src/api/queries.ts`, `crates/memoryd-web/frontend/src/api/mutations.ts`, `crates/memoryd/tests/policy_editor.rs`, `crates/memoryd-web/tests/policy_editor_daemon.rs`, `crates/memoryd-web/frontend/tests/views/settings.test.tsx`  
**Invariants:** invalid YAML or incomplete policy sets fail closed and do not mutate `policies/*.yaml`; successful writes are atomic and audited.  
**Out of scope:** letting agents arbitrarily write shared policy files over MCP.

**Files:**
- Create: `crates/memoryd/src/policy_editor.rs`
- Create: `crates/memoryd-web/frontend/src/views/settings/PolicyEditorTab.tsx`
- Create: `crates/memoryd/tests/policy_editor.rs`
- Create: `crates/memoryd-web/tests/policy_editor_daemon.rs`
- Modify: `crates/memoryd/src/handlers/mod.rs`
- Modify: `crates/memoryd-web/src/routes/policy_editor.rs`
- Modify: `crates/memoryd-web/frontend/src/views/Settings.tsx`
- Modify: `crates/memoryd-web/frontend/src/api/queries.ts`
- Modify: `crates/memoryd-web/frontend/src/api/mutations.ts`
- Modify: settings frontend tests as needed.

**Step 1: Define policy-editor behavior inventory and take the first red slice**

Add these tests one behavior at a time:
- `PolicyValidate` accepts valid YAML and returns summaries without writing.
- `PolicyWrite` writes `repo/policies/<safe-name>.yaml` atomically and emits an event/audit entry.
- Invalid YAML leaves existing files unchanged.
- Daemon-backed `GET /api/policy-editor` returns current policy YAML, file list, current file name, validation summaries, and `writable: true` when `repo/policies` is writable.
- Daemon-backed `POST /api/policy-editor` returns `200` and persists when valid, not backend unavailable.
- Settings renders a `Policies` tab that uses daemon GET data to enable save, submits through `usePolicyEditorMutation`, shows validation errors, and refreshes policy summaries/current YAML after save.

**Step 2: Run focused failing tests**

```bash
cargo test -p memoryd --test policy_editor -- --nocapture
cargo test -p memoryd-web --test policy_editor_daemon -- --nocapture
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/views/settings.test.tsx
```

**Step 3: Implement daemon policy editor module**

Move reusable validation/write logic out of web route into `memoryd`:
- `target_file_name`.
- safe filename check.
- validation directory.
- full `PolicySet::load_from_dir` validation.
- atomic write + parent fsync.
- event log append for policy change.

Web route becomes a thin client forwarding to daemon unless an explicit `policy_dir` is configured for tests.

The daemon policy editor response must include enough data for a writable UI:
- concatenated or selected `raw_yaml`,
- `files: Vec<String>`,
- `current_file: Option<String>`,
- `writable: bool`,
- policy summaries with source labels.

Do not map daemon `GovernancePolicyDump` to `writable: false` unless the daemon reports the policy directory is not writable.

**Step 4: Mount the policy editor UI**

- Add a `Policies` tab to `Settings.tsx` without removing existing appearance/keyboard/notifications/dev/about tabs.
- `PolicyEditorTab.tsx` should show current source, file selector/name, YAML textarea, policy summaries, save/validate state, and invalid YAML errors.
- Use existing query/mutation helpers; if they need stronger types from Task 1, update them here.
- Disable save while a mutation is pending or when `raw_yaml` is empty.
- Frontend test must prove a daemon-backed query with `writable: true` enables save and that successful POST refreshes the displayed YAML/summaries.

**Step 5: Run tests green**

```bash
cargo test -p memoryd --test policy_editor -- --nocapture
cargo test -p memoryd-web --test policy_editor_daemon -- --nocapture
pnpm --dir crates/memoryd-web/frontend exec vitest run tests/views/settings.test.tsx
```

**Verification plan:**
- Primary: daemon/web policy editor tests.
- Secondary: `cargo test -p memory-governance --test policy_contract -- --nocapture`.

---

### Task 8: Make source and dashboard docs truthful

**Parallel:** yes after Tasks 2-7  
**Blocked by:** Tasks 2, 3, 4, 5, 6, 7  
**Owned files:** `README.md`, `docs/getting-started.md`, `docs/mcp-wiring.md`, `docs/api/web-source-grounding-api.md`, `docs/api/stream-g-observability-api.md`, `docs/runbooks/dogfooding-day-one.md`, `scripts/docs-command-validity.sh`, `scripts/install-memorum.test.sh`  
**Invariants:** docs must not promise model privacy filter, browser-rendered capture, pairing, or full ROI semantics unless implemented.  
**Out of scope:** editing historical review docs except when they are linked as current operator guidance.

**Files:**
- Modify docs listed above.
- Modify docs validity scripts if new commands/options are introduced.

**Step 1: Define docs-validity behavior inventory and take the first red slice**

Add checks for:
- no `ROI deferred` language in current dashboard docs after Task 2.
- source capture docs list exact supported modes and explicit unsupported modes.
- web controls docs do not describe disabled alpha controls as working.
- MCP `memory_capture_source` schema examples match actual manifest.

**Step 2: Run docs checks**

```bash
bash scripts/docs-command-validity.sh
bash scripts/install-memorum.test.sh
```

Expected before docs updates: fail on stale examples or missing new modes.

**Step 3: Update docs**

Update:
- onboarding source-capture examples,
- dashboard route truth table,
- alpha limitations,
- eval/release gate section,
- dogfood smoke checklist.

**Step 4: Run docs checks green**

```bash
bash scripts/docs-command-validity.sh
bash scripts/install-memorum.test.sh
```

**Verification plan:**
- Primary: docs scripts.
- Secondary: `rg -n "deferred|coming soon|not implemented|placeholder" README.md docs/getting-started.md docs/mcp-wiring.md docs/api docs/runbooks/dogfooding-day-one.md`, then classify every hit as historical, explicitly unsupported alpha limitation, or stale-current-doc bug.

---

### Task 9: Eval/release semantic coverage and CI hardening

**Parallel:** yes, but use a separate worktree; may be large  
**Blocked by:** Task 1 for protocol compatibility; Task 5 if source-capture eval changes depend on new modes  
**Owned files:** `crates/memorum-eval/src/orchestrator.rs`, `crates/memorum-eval/src/harness_runner.rs`, `crates/memorum-eval/tests/eval/domain/t17_lease_contention_resolution.rs`, `crates/memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs`, `crates/memoryd/src/dream/lease.rs`, `crates/memoryd/src/dream/harness.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memory-privacy/src/keys.rs`, `.github/workflows/stream-h-eval.yml`  
**Invariants:** mock mode may skip real-harness semantics honestly; release mode must fail if required semantic coverage is missing.  
**Out of scope:** requiring paid provider calls on every normal push.

**Files:**
- Modify: `crates/memorum-eval/src/orchestrator.rs`
- Modify: `crates/memorum-eval/src/harness_runner.rs`
- Modify: `crates/memorum-eval/tests/eval/domain/t17_lease_contention_resolution.rs`
- Modify: `crates/memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs`
- Modify: `crates/memoryd/src/dream/lease.rs`
- Modify: `crates/memoryd/src/dream/harness.rs`
- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/main.rs`
- Modify: `crates/memory-privacy/src/keys.rs`
- Modify: `.github/workflows/stream-h-eval.yml`
- Leave dogfood-script/runbook notes for Task 10 unless this task is executed sequentially by the coordinator.

**Step 1: Define eval behavior inventory and take the first red slice**

Add these tests one behavior at a time:
- T17 no longer exits via `SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED` under simulator mode.
- T18 no longer exits via `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED` when using default device key setup.
- `memorum-eval --harness mock --required-release-set alpha` reports zero deferred tests.
- CI workflow has a required non-mock release path for tags/RCs and a visible neutral skip when secrets are unavailable on non-release pushes.

**Step 2: Run focused failing tests**

```bash
cargo test -p memorum-eval --test domain t17_preseeded_two_device_lease_blocks_loser_and_allows_retry_after_release -- --nocapture
cargo test -p memorum-eval --test domain t18_encrypted_tier_key_rotation_preserves_reads_and_forward_secrecy -- --nocapture
cargo run -p memorum-eval -- --harness mock --output json --filter t17
cargo run -p memorum-eval -- --harness mock --output json --filter t18
cargo test -p memorum-eval --test ci_workflow_shape -- --nocapture
cargo run -p memorum-eval -- --harness mock --required-release-set alpha --output json
```

**Step 3: Implement T17 lease semantics**

- Make same-device active dream leases re-entrant when lease record holder matches current device and scope.
- Preserve loser behavior across two clones/devices.
- Add a deterministic simulator harness path for `cli_override: "echo"` or replace the test override with a shipped no-network simulator adapter gated to eval/test mode.

**Step 4: Implement T18 key rotation contract**

- Implement rotation on the live key path: `crates/memory-privacy/src/keys.rs` plus `crates/memoryd/src/cli.rs` / `crates/memoryd/src/main.rs`.
- `memoryd device rotate-keys --runtime <path>` preserves the existing `FileKeyProvider::runtime_default` location unless a test requires a migration, and creates:
  - active key material at the existing runtime key store location,
  - an archived/decommissioned key location under the runtime privacy key store,
  - event-log entry for rotation.
- Old encrypted memories remain revealable.
- New encrypted writes use the new active recipient.
- Tree scan verifies plaintext bodies do not leak.

**Step 5: Harden eval orchestrator and CI**

- Remove `deferred: true` for T17/T18 only after tests pass.
- Add an `--required-release-set alpha` release gate mode and verify its exit-code policy directly in Task 9.
- Update `.github/workflows/stream-h-eval.yml` so RC/release tags fail on partial real-harness coverage.
- Keep normal push cheap: mock + simulator only, with explicit summary.
- If `scripts/check-dogfood.sh` needs the new release-set command, record the exact patch for Task 10 rather than editing that shared gate from a parallel Task 9 worktree.

**Step 6: Run tests green**

```bash
cargo test -p memorum-eval --test domain t17_preseeded_two_device_lease_blocks_loser_and_allows_retry_after_release -- --nocapture
cargo test -p memorum-eval --test domain t18_encrypted_tier_key_rotation_preserves_reads_and_forward_secrecy -- --nocapture
cargo run -p memorum-eval -- --harness mock --output json --filter t17
cargo run -p memorum-eval -- --harness mock --output json --filter t18
cargo test -p memorum-eval --test ci_workflow_shape -- --nocapture
cargo run -p memorum-eval -- --harness mock --required-release-set alpha --output json
cargo run -p memorum-eval -- --harness mock --output json
```

**Verification plan:**
- Primary: T17/T18 focused eval tests and mock catalog.
- Secondary: `cargo test -p memorum-eval -- --nocapture`.

---

### Task 10: Integrated gate, live dogfood smoke, and cleanup

**Parallel:** no  
**Blocked by:** Tasks 2-9  
**Owned files:** `scripts/check-dogfood.sh`, `docs/runbooks/dogfooding-day-one.md`, `docs/plans/2026-05-25-alpha-core-gap-closeout.md`  
**Invariants:** do not hide failing gates by weakening scripts; every skip must explain why it is acceptable for alpha.  
**Out of scope:** commit/push unless explicitly requested.

**Files:**
- Modify gate/docs only if previous tasks introduced new checks.

**Step 1: Add dogfood gate coverage**

Update `scripts/check-dogfood.sh` to include:
- `/api/roi` daemon smoke.
- `/api/notifications/stream` daemon notification smoke.
- source capture local artifact smoke.
- policy GET/writable validate/write smoke in a temp repo.
- eval required alpha release-set dry run.

**Step 2: Run focused gates**

```bash
bash scripts/check-dogfood.sh
pnpm --dir crates/memoryd-web/frontend run lint
pnpm --dir crates/memoryd-web/frontend run typecheck
pnpm --dir crates/memoryd-web/frontend exec vitest run
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

**Step 3: Live local runtime smoke**

Only after code gates pass:

```bash
export MEMORUM_REPO="$(mktemp -d)/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
bash scripts/install-memorum.sh --force-reinstall --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
memoryd serve --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --socket "$MEMORUM_SOCKET" --init --force-unsafe-durability &
MEMORYD_PID=$!
trap 'kill "$MEMORYD_PID" 2>/dev/null || true' EXIT
sleep 1
memoryd status --socket "$MEMORUM_SOCKET"
memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137
memoryd web status --socket "$MEMORUM_SOCKET" --json
cargo test -p memoryd --test daemon_e2e cli_client_write_note_then_search_then_get_through_live_daemon -- --nocapture
cargo test -p memoryd --test mcp_stdio mcp_stdio_tools_call_routes_through_daemon_forwarder -- --nocapture
```

Then smoke:
- MCP `memory_write` -> `memory_search` through the stdio bridge test or a real MCP client, not by running a bare blocking `memoryd mcp` process.
- web `/api/status`, `/api/roi`, `/api/notifications/stream`, `/api/policy-editor`.
- TUI launch and one refresh if interactive terminal is available.

**Step 4: Cleanup**

```bash
git diff --check
git status --short
```

Report:
- what passed,
- what was intentionally skipped,
- whether any live daemon remains running,
- whether the worktree is clean or contains intended changes.

**Verification plan:**
- Primary: integrated commands above.
- Secondary: browser visual QA if a dev server or `memoryd web enable` is available.

---

## Owned files duplicate check before parallel execution

Run this against the plan before assigning parallel workers:

```bash
rg '\*\*Owned files:\*\*' docs/plans/2026-05-25-alpha-core-gap-closeout.md \
  | sed 's/.*\*\*Owned files:\*\* *//' \
  | tr ',' '\n' \
  | sed 's/`//g' \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | rg -v '^$' \
  | sort \
  | uniq -d
```

Expected: every duplicate must be covered by the known-duplicate list near the top of this plan, marked `Parallel: no`, or routed through the coordinator before parallel execution. In practice, `protocol.rs`, `handlers/mod.rs`, `cli.rs`, `main.rs`, shared frontend API helpers, and final docs/gates require coordinator review.

---

## Final alpha acceptance checklist

Closeout state recorded 2026-05-26 from `docs/reviews/2026-05-26-alpha-gap-audit.md`.

- [x] `/api/roi` returns daemon data, not `501`. — Task 2.
- [x] `/api/notifications/stream` returns daemon passive notifications. — Task 3.
- [x] No visible dashboard button silently does nothing. — Task 6.
- [x] Dashboard status, peers, entities, and TUI panels never invent daemon facts. — Task 4. Frontend partials (`EntityTable.tsx normalizeKind`, `Peers.tsx normalizePeer`) fixed in 2026-05-26 pre-dogfood cleanup; architecture deviation accepted (see Architecture note in Task 4).
- [x] Source capture supports alpha modes through CLI, daemon, and MCP; unsupported rich modes fail clearly. — Task 5.
- [x] Policy editor GET shows daemon policy files/YAML as writable when allowed, and POST validates plus atomically writes daemon repo policies. — Task 7.
- [x] Eval catalog has no required alpha deferred tests; RC/release CI fails on missing required semantic coverage. — Task 9.
- [x] Privacy remains deterministic-first by explicit alpha decision. — Explicit scope exclusion; no implementation work required.
- [ ] `bash scripts/check-dogfood.sh`, workspace clippy, workspace tests, frontend typecheck/lint/Vitest all pass. — **Not exercised by the structural audit.** Gate script (`scripts/check-dogfood.sh`, 221 lines) covers all five required smoke categories; runtime confirmation is the residual verification step before claiming alpha-ready.
