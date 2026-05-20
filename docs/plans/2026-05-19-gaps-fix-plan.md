# GAPS.md fix plan — post-dogfood operational gaps

**Plan version:** v0.1
**Authored:** 2026-05-19
**Author:** Claude (synthesis) over GAPS.md (Trey-authored, commit `11c266b` on `origin/main`)
**Source evidence:** five parallel composer-2.5 read-only verification lanes (reports archived at `/tmp/gaps-verify/lane{1..5}-report.md`)
**Predecessor:** `docs/plans/2026-05-08-dogfood-readiness-codex-gap-fix.md` (closed out 2026-05-11)

---

## Why this plan exists

`GAPS.md` documents 20 source-visible gaps Trey identified during a manual inventory pass on 2026-05-19. The system is dogfood-ready (Streams A–I shipped), but several boundaries are still operating on fixture data, deferred responses, default zeros, or partial daemon protocols that look operational without actually being so.

Five parallel composer lanes verified every gap against the actual code. **All 20 gaps verify**, with the qualifications captured in the table below. Line drift was minor; one gap (#19) is materially smaller than claimed because the web daemon path already wires the real `completion`; one gap (#5) is mostly handled in production already and reduces to test discipline.

The plan groups the 20 gaps into seven waves ordered by leverage × risk: cheap wiring fixes first, then progressive completion of daemon-protocol surfaces, then the large substrate-touching tracks (source-capture rebuild and privacy provider), then release-gate hygiene, then a single explicit deferral for Trey to decide on.

---

## Verification summary (from lane reports)

| Gap | Claim | Status | Effort | Wave | Notes |
|-----|-------|--------|--------|------|-------|
| 1   | Source capture is static HTTP/text only | Verified | Medium | W3 | Only `HttpStaticV1`; MIME set is `text/plain`, `text/html`, `application/xhtml+xml`. Browser adapter dropped per Trey 2026-05-19. |
| 2   | Encrypted source artifacts unsupported | Verified | Medium | W3 | `enforce_extracted_privacy` refuses `EncryptAtRest`; touches Stream D invariants 1–2 |
| 3   | Raw storage has no encrypted fallback | Verified | Medium | W3 | `RawStorage::OmittedPrivacy`; no encrypted variant; co-PR with #2 |
| 4   | Dashboard daemon status mostly synthetic | Verified | Large | W1 | Many zeroed/synthetic fields in `StatusDashboardResponse::from_daemon` |
| 5   | Fixture mode is a large product surface | Partial | Small | W1 | Prod path already `daemon-or-503`; reduces to test/CI discipline |
| 6   | ROI endpoint deferred in daemon mode | Verified | Large | W7 | Explicit `deferred_response("roi")` — Stream G v1.1 by design |
| 7   | Notifications stream doesn't surface daemon | Verified | Medium | W1 | `passive_notifications` already on `StatusResponse` but web ignores it |
| 8   | Reality Check history empty in daemon mode | Verified | Medium | W2 | `List` request returns pending queue, not history; needs new variant |
| 9   | Entity detail approximate in daemon mode | Verified | Medium | W2 | `RecallIndexRow` has the data; handler doesn't project it |
| 10  | Policy editor cannot write through daemon | Verified | Medium | W2 | No write protocol; fail-closed invariant must be preserved |
| 11  | Standalone daemon is health-only | Verified | Medium | W4 | Production never uses standalone; risk is operator misinterpretation |
| 12  | Privacy filter provider disabled by default | Partial | Small | W5 | Framework only per Trey 2026-05-19; actual provider implementations deferred |
| 13  | Notification delivery best-effort | Verified | Medium | W4 | Lagged broadcast drops events; SMTP env missing → silent `Ok(())` |
| 14  | Dreams not in general daemon status | Verified | Small | W1 | `StatusResponse.dreams = Default::default()`; rich data on separate route |
| 15  | Eval harness deferred / mock-only | Partial | Medium | W6 | T17/T18 implementations exist but stay `deferred: true`; catalog is 20 not 19 |
| 16  | Eval CI defaults to mock | Verified | Small | W6 | Scheduled cron only runs mock; RC partial gate matches `v1.*` only |
| 17  | Bench gate has placeholder baseline | Partial | Small | W6 | Linux baseline `runs: 0`; macOS is measured |
| 18  | Install scheduler optional | Verified | Small | W6 | Doctor doesn't warn about scheduler/harness; `--with-scheduler` exists |
| 19  | Web/TUI Reality Check progress incomplete | Partial | Small | W2 | Web daemon path already correct; TUI drops `completion`; `reality_check_session_progress` is dead code |
| 20  | Eval source grounding uses temp-file fixtures | Verified | Small | W3 | T20 builds artifacts manually; never exercises live capture |

**Verification statistics:** 20/20 substantiated, line drift minor across all files, 3 gaps marked Partial (#5, #12, #15, #17, #19) where the GAPS.md claim is true but smaller than implied or already mitigated in part.

---

## Wave structure and rationale

The waves are ordered so cheap, high-visibility fixes ship first; substrate-touching work follows once the daemon protocol surface is stable; pure-operational work (CI/scripts) runs in parallel; the one explicit roadmap deferral comes last for an explicit Trey decision.

- **Wave 1 — Status wiring quick wins** (≈1 week): Gaps 14, 7, 4, 5. Most are wiring gaps with data already on the wire; gap 4 cascades into the largest single piece of new daemon protocol but every field has a clear substrate source.
- **Wave 2 — Interaction completeness** (≈1.5 weeks): Gaps 19, 9, 8, 10. New protocol variants, but each isolated to one route.
- **Wave 3 — Source capture rebuild** (≈2 weeks): Gaps 2+3+20 together (encrypted-source PR), then Gap 1 as a separate track. Touches Stream A `memory-source` boundary; Stream D invariants apply.
- **Wave 4 — Daemon hardening** (≈1 week): Gaps 11, 13. Notification durability + standalone-mode readiness signaling.
- **Wave 5 — Privacy provider framework** (≈3–4 days): Gap 12. Ship the `{ name, endpoint, api_key_env, model_path, fail_mode, per_namespace }` config + trait shape + CLI wiring for "both behind config, default disabled" per Trey decision 2026-05-19. Defer actual OpenAI / local-model provider implementations to a later milestone.
- **Wave 6 — Release-gate hygiene** (≈1 week, parallelizable with W1–W5): Gaps 16, 17, 18, 15.
- **Wave 7 — Explicit roadmap deferral**: Gap 6 (ROI). Confirmed deferred to v1.1 per Trey decision 2026-05-19; documented in spec as roadmap, closed as deferral.

**Parallelism:** fan out as many concurrent waves as is reasonable. W1 + W6 from day one; W2 starts after W1.T3 lands the new protocol fields; W3, W4 independent; W5 small enough to slot anywhere.

**Branch model:** one branch per wave (`gaps-fix/wave-N-<slug>`), merged independently into `main`. This plan + GAPS.md sit on `gaps-fix/plan-v0.1` (this document's home); wave branches fork off `main` as each wave starts.

Total nominal effort: **3–5 weeks of focused work** with two concurrent agents and W6 + W7 in the background.

---

## Operational contract

This plan is authored for **Claude** execution via `@agent-task-builder` fan-out (not Codex). The conventions Codex uses (`update_plan`, worktree-per-task, gate scripts) are not enforced here — Claude operates from the main checkout with `pnpm run check:fast` / `pnpm run check:local` as the inner/outer gates per `CLAUDE.md`.

- **Branch model:** one branch per wave (`gaps-fix/wave-N-<slug>`), tasks within a wave land as separate commits.
- **Gate policy:** `check:fast` between tasks, `check:local` between waves, `check:full` only before merging the wave branch into `main`.
- **Spec invariant policy:** changes that risk any of `CLAUDE.md`'s seven critical invariants (esp. 1–2 in W3; 5 in W3) MUST surface in commit message and wave PR description with explicit verification.
- **Plan reviewer:** the `plan-reviewer` subagent should be run on this plan before W1 starts, then on individual wave plans if any wave's structure changes during execution.

---

## Wave 1 — Status wiring quick wins

Goal: the `/api/status` route, the notifications stream, and the TUI status panel show real daemon state instead of zeros and synthetic placeholders.

### W1.T1 — Compact dream summary on `StatusResponse` (Gap 14)

**Why first:** smallest unit; data already exists; unblocks W1.T2's `from_daemon` mapping.

**Current state** (`crates/memoryd/src/handlers/mod.rs:1152–1165`):
```rust
fn status_response(state: &HandlerState) -> StatusResponse {
    StatusResponse {
        state: "ready".to_string(),
        guidance: "...".to_string(),
        recall: state.recall.snapshot(),
        dreams: Default::default(),         // ← always zeros
        passive_notifications: ...,
    }
}
```

A separate route (`RequestPayload::DreamStatus`) returns the rich `DreamStatusReport` — full CLI inventory, leases, disclosure. Status just needs a *compact* projection.

**Implementation:**
1. Extend `DreamStatusCounters` (or add a new compact `DreamStatusSummary` field) with: `enabled: bool`, `disabled_sentinel: Option<String>`, `last_run_at: Option<DateTime<Utc>>`, `next_run_at: Option<DateTime<Utc>>`, `active_leases: u32`, `dream_runs_invoked_total: u64`, `dream_runs_failed_total: u64`.
2. In `status_response`, build the summary from `crate::dream::status::collect_counters` (or the cheapest available aggregator that does not require a substrate walk).
3. Mirror into `crates/memoryd-web/src/routes/status.rs` so `DreamingStatus` reflects real values; remove the `status: "daemon"` placeholder.
4. Update `crates/memoryd/tests/handler_contract.rs::status_response_includes_default_dream_counters` to assert real fields.

**Files touched:** `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/dream/status.rs`, `crates/memoryd-web/src/routes/status.rs`, `crates/memoryd/tests/handler_contract.rs`.

**Verification gate:** `cargo test -p memoryd handler_contract` + new test `dream_summary_matches_dream_status_route` asserting compact summary fields are a strict subset of `DreamStatusReport`.

**Risk:** Small. Additive DTO fields with serde defaults preserve compat.

---

### W1.T2 — Wire `passive_notifications` to dashboard notifications stream (Gap 7)

**Why second:** data already on the wire; only web mapping is missing.

**Current state** (`crates/memoryd-web/src/routes/status.rs:128–135`):
```rust
pub async fn notifications_stream(State(state): State<WebState>) -> Response {
    let notifications = if let Some(data) = state.dashboard_data() {
        data.notifications.clone()
    } else if state.daemon_socket().is_some() {
        Vec::new()                              // ← drops daemon notifications
    } else { return backend_unavailable("notifications_stream").into_response(); };
    // ...
}
```

`status_response` already populates `passive_notifications: Vec<PassiveNotificationStatus { message, created_at }>` from the in-memory `PassiveQueue` (cap 100).

**Implementation:**
1. In `notifications_stream`'s daemon branch, call `RequestPayload::Status` over the existing socket client and map `StatusResponse.passive_notifications` → `NotificationSnapshot`. Use a sane heartbeat cadence (e.g., 5s poll, matching the SSE heartbeat).
2. Add a `kind` field to `PassiveNotificationStatus` (currently message-only) so the dashboard can colorize/route. The dispatcher already has `NotificationEvent::kind()`; carry it through `PassiveQueue::append`.
3. Update `tests/api_contract.rs::test_daemon_configured_notifications_stream_returns_empty_heartbeat` — currently asserts the gap; flip the expectation.
4. Add a new test that seeds the passive queue, calls the stream, and asserts the notifications surface.

**Files touched:** `crates/memoryd-web/src/routes/status.rs`, `crates/memoryd/src/notifications/passive.rs`, `crates/memoryd/src/protocol.rs` (`PassiveNotificationStatus`), `crates/memoryd-web/tests/api_contract.rs`, possibly a new `crates/memoryd-web/tests/notification_visibility.rs`.

**Verification gate:** new daemon-backed test + flipped contract test.

**Risk:** Small–Medium. Note that this is *polling* over the Status request; a future PR could introduce a dedicated streaming subscription (out of scope here — see Out of Scope).

---

### W1.T3 — Real fields in `StatusDashboardResponse::from_daemon` (Gap 4)

**Why third:** the larger surface; sits on top of T1+T2; the protocol additions feed multiple subsequent routes.

**Current state** (`crates/memoryd-web/src/routes/status.rs:148–172`): index/sync/review/conflicts/peer fields are all hardcoded zeros or `"daemon"` strings. `uptime_seconds`, `version`, `pid` reflect the *web* process, not memoryd.

**Implementation (phased — break into commits as needed):**

1. **Daemon-side protocol additions** to `StatusResponse`:
   - `index: IndexStats { active_memories: u64, last_reindex: DateTime<Utc> }` from `Substrate::index_stats`
   - `git_sync: GitSyncStats { ahead: u32, behind: u32, last_push: Option<DateTime<Utc>>, remote: String }` from runtime git state
   - `review: ReviewStats { candidate: u32, quarantined: u32, dream_low_confidence: u32 }` from `memory-governance` queue
   - `conflicts: u32` from substrate `scan_blocking_conflicts` (cached; full scan is expensive — emit cached count)
   - `active_sessions: Vec<PeerSession>` from `memorum-coordination::status_summary`
   - `recall.peer_update_total: u64` from coordination event counter (it already exists; just plumb)
   - `daemon_version: String`, `daemon_pid: u32`, `uptime_seconds: u64` (daemon process — not web)
   All fields `#[serde(default)]` for additive compat.

2. **Daemon-side handler:** populate the above in `status_response`. Hot path; if any source is expensive (e.g., git status), cache the result with a short TTL and surface `last_refreshed_at`.

3. **Web-side mapping:** rewrite `from_daemon` to consume the new fields directly. Drop the synthetic placeholders.

4. **Sync-dashboard route** (`sync_dashboard.rs:77`): same plumbing — currently reuses `from_daemon(status).sync`, so it inherits the fix.

5. **Contract tests:** new integration test seeds a substrate with memories, conflicts, review items, peer sessions, dream artifacts, then asserts `/api/status` returns nonzero fields matching daemon state.

**Files touched:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd/src/handlers/status_caches.rs` (new, optional), `crates/memoryd-web/src/routes/status.rs`, `crates/memoryd-web/src/routes/sync_dashboard.rs`, `crates/memoryd/tests/handler_contract.rs`, `crates/memoryd-web/tests/api_contract.rs`.

**Verification gate:** `cargo test -p memoryd handler_contract` + new `cargo test -p memoryd-web daemon_status_contract` end-to-end test.

**Risk:** Large. Multiple sources to aggregate; cache invalidation in `scan_blocking_conflicts` is the trickiest piece. Mitigation: if a source is hard, ship the field as `Option<T>` and populate later.

---

### W1.T4 — Fixture-mode test discipline (Gap 5)

**Why last in W1:** doesn't fix code; tightens the test discipline so future regressions on T1–T3 don't get masked by fixture mode.

**Current state:** production binary `memoryd-web` already uses `WebState::daemon(args.socket)` only (`crates/memoryd-web/src/bin/memoryd-web.rs:11`); default `WebState::new()` returns 503. The gap is that *most tests* use `fixture_router()` and assert against `DashboardData::default()`, so daemon-mode regressions don't show up.

**Implementation:**
1. Audit every route handler that branches on `state.dashboard_data()`. Identify which ones currently lack a daemon-backed integration test (status, ROI, reality-check history, entity, notifications, audit, recall hits, policy editor).
2. For each route lacking coverage, add a daemon-backed test using `WebState::daemon(socket)` with a seeded `DaemonScaffold` (the same scaffold W1.T3 introduces).
3. Tag fixture-only tests with a module-level comment explaining "fixture demo path; daemon-backed coverage is in `<other_test>`."
4. Document the convention in `crates/memoryd-web/README.md` (or equivalent dev doc).
5. Optionally add an `X-Memorum-Fixture: true` response header when `dashboard_data()` is `Some` — explicit visual marker for ops looking at a fixture instance.

**Files touched:** `crates/memoryd-web/tests/*.rs` (additions), `crates/memoryd-web/src/server.rs` (header), `crates/memoryd-web/README.md`.

**Verification gate:** new tests pass; existing fixture tests untouched.

**Risk:** Small. Pure addition.

---

## Wave 2 — Interaction completeness

Goal: Reality Check, entity detail, and policy editor routes return real daemon data; TUI consumes the same completion semantics as the web.

### W2.T1 — TUI consumes `RealityCheckCompletion` (Gap 19, TUI side)

**Why first:** smallest fix; existing `reality_check_session_progress` API in TUI is dead code that already expects this data shape.

**Current state** (`crates/memoryd-tui/src/client.rs:110–118`):
```rust
let memoryd::protocol::RealityCheckResponse::Pending { session_id, items, .. } = response else {
    return Ok(crate::state::RealityCheckState::default());   // ← drops Respond completion
};
```

The daemon already returns `RespondAccepted { completion: RealityCheckCompletion::Progress | Complete }`. The web daemon path consumes it correctly (`reality_check.rs:227–248`); the TUI doesn't.

**Implementation:**
1. Replace the `let-else` with explicit match arms for `Pending`, `RespondAccepted { completion }`, and `Complete`. Map `completion.remaining` / `completion.deferred` to TUI state.
2. Wire `reality_check_session_progress` into the focus-mode app loop (`app.rs:381–385`) so it updates progress after every action, not just `Correct`.
3. Make unexpected response variants surface a visible error in TUI rather than silently returning `default()`.
4. Update `crates/memoryd-tui/tests/focus_mode_progress.rs` to use real daemon responses (or a faithful mock) instead of hand-built `RealityCheckState`.
5. Web fixture mode (`crates/memoryd-web/src/routes/reality_check.rs:179–187`) — fix the `remaining: 0, deferred: 0` placeholder by computing from the fixture session state (or removing the fixture respond path; fixture is for read-only UI demo).

**Files touched:** `crates/memoryd-tui/src/client.rs`, `crates/memoryd-tui/src/app.rs`, `crates/memoryd-tui/tests/focus_mode_progress.rs`, `crates/memoryd-web/src/routes/reality_check.rs`.

**Verification gate:** TUI integration test asserts focus-mode progress updates correctly across all action variants; web fixture test asserts realistic completion counts.

**Risk:** Small.

---

### W2.T2 — Project entity lifecycle/recall into `EntitySummary` (Gap 9)

**Current state** (`crates/memoryd-web/src/routes/entity_graph.rs:241–259`): daemon mode hardcodes `namespace: "daemon"`, `status: "unknown"`, `confidence: 0.0`, no first/last seen, no supersession, no recall.

The data exists in `RecallIndexRow` (`memory-substrate/src/model.rs:1245–1291`: `status`, `confidence`, `updated_at`, `entities`); `inspect_entities_response` (`crates/memoryd/src/handlers/mod.rs:592–629`) just doesn't project it.

**Implementation:**
1. Extend `EntitySummary` (`protocol.rs:308–318`) with: `status: Option<String>`, `confidence: Option<f32>`, `first_seen: Option<DateTime<Utc>>`, `last_seen: Option<DateTime<Utc>>`, `supersession_chain: Vec<MemoryId>`, `recall_history: Vec<RecallHitSummary>`. All `Option`/`Vec`-defaulted.
2. In `inspect_entities_response`, aggregate the new fields from `query_recall_index_including_metadata_only` plus envelope reads (for supersession) plus `recall_hits` query.
3. Replace web-side synthesis with direct mapping.
4. Update `tests/entity_endpoints.rs::test_daemon_backed_entity_graph_and_detail_return_live_entities` to assert the new fields.
5. **Co-mention edges:** the daemon graph emits only `co_mentioned` edges; fixture graph includes `supersedes`. Extend `co_mention_edges` to also project supersession edges from the new chain field.

**Files touched:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd-web/src/routes/entity_graph.rs`, `crates/memoryd-web/tests/entity_endpoints.rs`, possibly `crates/memory-substrate/src/index.rs` if a new projection is needed.

**Verification gate:** new test seeds entities with supersession chains and recall hits; asserts entity detail returns them.

**Risk:** Medium. The `recall_hits` join is the expensive part; cache or paginate if needed.

---

### W2.T3 — Reality Check history protocol variant (Gap 8)

**Current state** (`crates/memoryd-web/src/routes/reality_check.rs:137–139`): daemon respond to `RealityCheck(List)` is mapped to `RealityCheckHistoryResponse { sessions: Vec::new() }` — `List` returns the pending queue, not history.

History is not aggregated anywhere; per-action events (`EventKind::RealityCheckConfirmed`, etc.) live in the substrate event log but no rollup exists.

**Implementation:**
1. Add `RealityCheckRequest::History { namespace: Option<String>, limit: Option<u32> }` to `protocol.rs`.
2. Add `RealityCheckResponse::History { sessions: Vec<RealityCheckHistorySession> }` with per-session fields: `session_id`, `completed_at`, action counts (`confirmed`, `corrected`, `forgotten`, `not_relevant`, `skipped`), `total_scored`, `last_completed_at`.
3. Implement the handler by scanning the events log for `RealityCheck*` events in the window, grouping by session. Cache per session; sessions are immutable once closed.
4. Wire `crates/memoryd-web/src/routes/reality_check.rs` to call `History` and stop returning empty.
5. Update `crates/memoryd/src/reality_check/session.rs` to *emit* `EventKind::RealityCheckSessionCompleted` if it doesn't already (verify; the per-action events exist).
6. Tests: daemon-backed history round-trip with multiple completed sessions.

**Files touched:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers/mod.rs`, `crates/memoryd/src/reality_check/session.rs`, possibly a new `crates/memoryd/src/reality_check/history.rs`, `crates/memoryd-web/src/routes/reality_check.rs`, tests.

**Verification gate:** new daemon-backed integration test; protocol contract test for the new variant.

**Risk:** Medium. Event-log scan is the perf-sensitive piece; bound by `limit` and cache aggressively.

---

### W2.T4 — Policy editor write protocol (Gap 10)

**Current state** (`crates/memoryd-web/src/routes/policy_editor.rs:75–164`): read works via `GovernancePolicyDump`; write only works when `state.policy_dir()` is set (which the production binary doesn't configure). Daemon snapshots are marked `writable: false`.

**Implementation:**
1. Add `RequestPayload::GovernancePolicyValidate { file_name: String, raw_yaml: String }` and `RequestPayload::GovernancePolicyWrite { file_name: String, raw_yaml: String, expected_revision: Option<String> }` to `protocol.rs`.
2. Mirror the existing `validate_and_write_policy` logic from the web crate into the daemon handler — full `PolicySet::load_from_dir` validation (fail-closed per Stream C invariant), atomic write under `repo/policies/`, in-process policy set reload.
3. Return structured validation errors and a dry-run diff before applying.
4. Return `GovernancePolicySnapshot { writable: true }` when the request came from a write-capable daemon.
5. Emit `EventKind::PolicyEdited { actor, file_name, prev_hash, new_hash }` to the event log for audit.
6. Web: replace the `state.policy_dir()` branch with daemon write; preserve CSRF protection.
7. Keep the duplicate `validate_and_write_policy` helper in the web crate behind a feature flag or behind `policy_dir`-only mode for backward compat with disk-driven setups (or remove it if no one's using it — likely no one).

**Files touched:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers/mod.rs` (new policy write handler), `crates/memoryd-web/src/routes/policy_editor.rs`, `crates/memory-governance/src/policy.rs` (if validation primitives need to be shared), tests.

**Verification gate:** daemon-socket POST valid/invalid YAML; assert no partial write; assert daemon policy dump reflects reload; CSRF still enforced. Stream C invariant: confirm `PolicySet::load_from_dir` is called on the *full* policy set before commit (not single-file parse alone).

**Risk:** Medium. The fail-closed invariant is the critical constraint; the rest is plumbing.

---

## Wave 3 — Source capture rebuild

Goal: source capture supports private content (encrypted) and multiple capture types (browser-rendered, PDF, etc.), and the eval harness validates the real capture pipeline.

### W3.T1 — Encrypted extracted text + raw artifacts (Gaps 2 + 3 + 20, single PR)

**Why grouped:** the privacy gap (#2), raw-storage gap (#3), and eval coverage gap (#20) share infrastructure (`memory-privacy` encryptor + `RawStorage` enum + `WebCaptureArtifact`). One PR is cleaner than three.

**Current state:**
- `crates/memory-source/src/capture.rs:212–221` — `enforce_extracted_privacy` returns `Err(encrypted_source_artifacts_unsupported)` on `EncryptAtRest`
- `crates/memory-source/src/capture.rs:125–134` — `RawStorage::OmittedPrivacy` when raw text isn't safe
- `crates/memorum-eval/src/simulator.rs:258–268` — T20 fakes capture by writing temp files

**Implementation:**
1. **`RawStorage` variant:** add `RawStorage::Encrypted { ciphertext_sha256: String, dek_id: String }`. Update `crates/memory-source/src/model.rs:129–133`.
2. **Encrypted extracted-text path:** in `enforce_extracted_privacy`, instead of refusing `EncryptAtRest`, call into `memory-privacy::PrivacyEncryptor` to seal the extracted text. Persist as `<artifact_dir>/extracted.enc.bin` with `dek_id` in manifest. Plaintext must never touch disk.
3. **Encrypted raw path:** symmetric — compress + encrypt `raw_bytes` to `<artifact_dir>/raw.bin.enc.zst`. Manifest holds ciphertext hash + DEK ref. **Verify CLAUDE.md invariants 1 (secret never persisted) and 2 (ClassificationOutcome required).**
4. **Reveal path:** add a new `Substrate::reveal_web_capture` (or extend the existing reveal mechanism) that pairs with `Substrate::record_encrypted_content_revealed` per spec §4.
5. **Manifest schema bump:** new field for storage variant. Bump `WebCaptureManifestVersion` if it exists; otherwise additive with serde defaults.
6. **`storage.rs` updates:** read/write paths understand encrypted storage; `verify_web_capture` checks the right hash (post-encryption).
7. **Eval coverage (Gap 20):** add a new simulator action `CaptureSource { request: CaptureRequest }` that exercises the real `memory_capture_source` MCP tool. Update T20 to use it instead of hand-building `WebCaptureArtifact`. Add new test catalog entries for: encrypted capture, refusal, and live HTTP redirect.

**Files touched:** `crates/memory-source/src/capture.rs`, `model.rs`, `storage.rs`, `url_safety.rs`; `crates/memory-privacy/src/encryptor.rs` (verify API exists); `crates/memory-substrate/src/runtime/...` for reveal; `crates/memorum-eval/src/simulator.rs`, `orchestrator.rs`, `tests/eval/domain/t20_*.rs`; protocol updates if MCP tool shape changes.

**Verification gate:**
- Source-capture test where classifier routes extracted text to `EncryptAtRest`: assert capture succeeds, plaintext absent from manifest/artifact tree/indexes, reveal recovers content.
- Sensitive HTML capture: raw bytes encrypted, retrievable only via reveal.
- New eval T20a/T20b for live HTTP + encrypted-source branches.

**Risk:** Medium–Large. Touches Stream A artifact surface and Stream D invariants. Merge-driver behavior for encrypted artifacts (Stream A spec §14) needs explicit verification — check that `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` doesn't need bumping (CLAUDE.md invariant 5).

---

### W3.T2 — Multi-adapter capture pipeline (Gap 1)

**Why separate from T1:** keeps the encrypted-source PR small and lets us land each adapter independently.

**Headless browser dropped per Trey decision 2026-05-19.** GAPS.md listed "browser-rendered pages" as a hole; for Memorum's agent-grounding use case, HTTP-static covers the dominant pattern (docs, articles, API references). JS-only SPA grounding is a v1.1+ question if it ever comes up — the cost of sidecar/sandbox/render-timing is real and the demand isn't proven.

**Current state** (`crates/memory-source/src/capture.rs:61–69`, `extract.rs:33–48`): single HTTP-static path, hardcoded headers, three MIME types.

**Implementation:**
1. **`CaptureMethod` enum:** add variants for `PdfV1`, `LocalFileV1`, `ManualImportV1`. Keep `HttpStaticV1` as-is.
2. **Adapter trait:** define `trait CaptureAdapter { async fn capture(&self, request: &CaptureRequest) -> SourceResult<CaptureOutput>; }`. Each adapter:
   - HTTP-static: existing `reqwest` path, refactored into the trait.
   - PDF: `pdf-extract` or `lopdf` crate for text extraction (audit licenses; both MIT).
   - Local file: read + MIME-sniff (`infer` crate) + extract.
   - Manual import: takes raw bytes + content-type + URL from the caller (for screenshot/authenticated/imported content the agent already has in hand).
3. **Dispatch by MIME + request mode** in `capture_web_source`: detect content type, route to adapter. Manual-import bypasses HTTP entirely.
4. **Manifest extension:** add `adapter_type: String`. Skip `render_metadata` / `browser_context` since no rendered adapter ships.
5. **MCP tool surface:** extend `memory_capture_source` request shape with optional `mode: CaptureMode` (default `HttpStatic`) and `import_payload: Option<ImportPayload>`.
6. **Integration tests:** PDF fixture, unsupported MIME fixture, local file fixture, manual-import roundtrip, HTTP redirect cases (already covered, regression-check after refactor).

**Files touched:** new `crates/memory-source/src/adapters/{http,pdf,local,manual}.rs`, dispatcher in `capture.rs`, `model.rs` for new manifest fields, `crates/memoryd/src/handlers/mod.rs` for the extended MCP tool, tests.

**Verification gate:** integration tests per adapter; assert every adapter emits stable source refs and governance can consume them.

**Risk:** Medium. With browser dropped, each remaining adapter is a small bounded piece. `pdf-extract` is the only new external dep — vet it for unsafe code surface.

---

## Wave 4 — Daemon hardening

### W4.T1 — Notification durability + delivery failure signaling (Gap 13)

**Current state:**
- `crates/memoryd/src/notifications/dispatcher.rs:37–45` — lagged broadcast receivers logged but events lost
- `crates/memoryd/src/notifications/external.rs:177–183` — missing SMTP env returns `Ok(())` silently

**Implementation:**
1. **Persist critical events:** add `NotificationConfig::persist_critical: bool` (default `true`). Critical kinds (e.g., `SecretRefused`, `BlockingConflict`, `RealityCheckOverdue`) are appended to a bounded on-disk queue (JSONL under `runtime/notifications/`) before broadcast. Replay on startup until ack'd or aged out.
2. **Treat missing required credentials as failure** for configured channels: `smtp_password_env` unset when SMTP channel is configured → `Err(NotificationError::CredentialMissing)`. Surface in delivery status.
3. **Delivery status DTO:** extend `NotificationConfig` with a `delivery_status: Arc<DashMap<NotificationId, DeliveryStatus>>`. Status: `Pending | Delivered { at } | Failed { reason, attempts, last_attempt }`. Expose via a new `RequestPayload::NotificationsDeliveryStatus { since: Option<DateTime<Utc>> }`.
4. **Dashboard wiring:** the dashboard notifications panel surfaces delivery status + suppressed reasons.

**Files touched:** `crates/memoryd/src/notifications/dispatcher.rs`, `external.rs`, `config.rs`, new `crates/memoryd/src/notifications/persistence.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd-web/src/routes/status.rs` (or new route).

**Verification gate:** tests for lagged broadcast with replay, missing SMTP env, retry exhaustion, persisted queue surviving restart.

**Risk:** Medium–Large. The on-disk queue introduces a new format that must be safe to roll forward; mitigate by bounding to ~1MB and including a schema version field.

---

### W4.T2 — Standalone mode readiness signaling (Gap 11)

**Current state** (`crates/memoryd/src/server.rs:46–54`, `392–416`): `Dispatch::Standalone` returns `healthy_status()` for `Status` and `not_implemented` for everything else. Production `memoryd serve` never uses this path — but a manually launched standalone daemon (or a future change) could make MCP look "up" while memory tools fail.

**Implementation:**
1. Add `substrate_attached: bool` and `readiness: Readiness` (enum: `HealthOnly | SubstrateAttached | DegradedReindexing`) to `StatusResponse`.
2. Update `healthy_status()` to set `substrate_attached: false`, `readiness: HealthOnly`, with explicit guidance.
3. Update `status_response` (substrate path) to set `substrate_attached: true`.
4. MCP startup: if `mcp_stdio::probe_live_socket` succeeds but the daemon reports `HealthOnly`, refuse MCP startup with a clear error ("daemon is in health-only mode; memory operations unavailable"). Or alternatively, attempt to start a substrate daemon ourselves via `auto_start_daemon` (already supported).
5. Web `/api/status`: surface readiness so the dashboard can show a banner.
6. Tests: `server_smoke.rs` adds a `non_status_request_on_standalone_returns_not_implemented` test; MCP integration test against a standalone socket fails clearly.

**Files touched:** `crates/memoryd/src/server.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/mcp_stdio.rs`, `crates/memoryd-web/src/routes/status.rs`, tests.

**Verification gate:** new tests.

**Risk:** Medium. The MCP refusal logic is the careful piece — must not break the auto-start happy path.

---

## Wave 5 — Privacy provider framework (defer implementations)

### W5.T1 — Provider framework + config + CLI scaffolding (Gap 12)

**Decision (Trey, 2026-05-19):** ship the *framework* — config types, trait shape, CLI wiring, fail-mode policy — for "both behind config, default disabled." Defer the actual OpenAI / local-model provider implementations to a later milestone. The point is to make sure the next person (or later us) lands implementations against a stable surface, not to ship a model right now.

**Current state** (`crates/memory-privacy/src/privacy_filter.rs:13–24`, `classifier.rs:28–37`): `DisabledPrivacyFilter` exists as a sentinel; runtime uses `provider: None`; CLI `memoryd privacy-filter enable` returns `"provider runtime not configured"`.

**Implementation (framework only):**
1. Define `ProviderConfig` in `crates/memory-privacy/src/config.rs`: `name: ProviderName` (enum: `Disabled | OpenAi | Local | Custom(String)`), `endpoint: Option<String>`, `api_key_env: Option<String>`, `model_path: Option<PathBuf>`, `fail_mode: FailOpen | FailClosed`, `per_namespace: HashMap<Namespace, ProviderConfig>`.
2. Define a `BoxedPrivacyFilterProvider` constructor that takes `ProviderConfig` and returns one of: `DisabledPrivacyFilter` (the always-implemented sentinel), `FixturePrivacyFilter` (for tests), or `Err(PrivacyError::ProviderNotImplemented { name })` for `OpenAi` / `Local` / `Custom` — explicit "not built yet" error, not silent fallback.
3. Audit metadata: extend `PrivacyDecision::scan` (already has `model` field) to also record `provider_name`, `failure_mode`. Disabled provider records that fact honestly; it doesn't pretend a scan happened.
4. Fail-closed/fail-open policy per namespace: default `Project` namespace = fail-closed, `Personal` = fail-open with warning. Encode the matrix in `PrivacyPolicy::resolve` so future providers slot in without re-deciding policy.
5. Wire through `DeterministicPrivacyClassifier::with_provider` everywhere the classifier is constructed — handler writes, dream candidate generation, recall, trust artifact assembly. Today every site uses `::new()` (no provider); add an explicit "load provider from runtime config" step.
6. CLI: `memoryd privacy-filter enable --provider <name>` should return a clear "provider implementation not yet shipped; framework is in place" message for the unimplemented variants, and succeed for `disabled`/`fixture`. `--disable` works as today.
7. Tests: contract tests asserting (a) `Disabled` config produces unavailable error, (b) `OpenAi`/`Local` configs produce `ProviderNotImplemented`, (c) classifier-with-provider plumbing works end-to-end with the fixture provider, (d) per-namespace fail-mode matrix.

**Files touched:** `crates/memory-privacy/src/privacy_filter.rs`, `classifier.rs`, new `crates/memory-privacy/src/config.rs`; daemon handler wiring; CLI in `crates/memoryd/src/main.rs`; integration tests.

**Verification gate:** all unit + contract tests; CLI enable→fail-with-clear-message cycle; runtime config round-trip.

**Spec invariant check:** must not weaken invariant 1 (`secret` never persisted) or invariant 2 (`ClassificationOutcome` required). Disabled provider does **not** synthesize a fake "all clear" decision — Layer 1 deterministic scanning continues independently, and the absence of provider spans is recorded as such.

**Out of scope (deferred):** actual `OpenAiPrivacyFilter` / `LocalModelPrivacyFilter` implementations, model packaging, network endpoint negotiation, prompt-injection hardening. When we come back to ship a provider, the framework is ready — just drop in the trait impl and flip the `ProviderConfig` enum branch.

**Risk:** Small. Pure scaffolding; no model choice required.

---

## Wave 6 — Release-gate hygiene (parallelizable)

These four tasks can run independently of W1–W5 and each other.

### W6.T1 — Linux bench baseline + strict mode (Gap 17)

1. Commit a real measured `bench/baseline.linux-x86_64.json` (run the bench on a Linux CI host or the equivalent). Set `runs >= 5`.
2. Add `BENCH_STRICT=1` mode to `scripts/bench-regression-check.sh` that fails on `runs == 0` baseline. Default off for dev; on for release/CI.
3. Wire `BENCH_STRICT=1` into `.github/workflows/stream-a-perf.yml` for release-tagged runs.
4. Add a contract test in `release_gate_contracts.rs` asserting the Linux baseline has `runs > 0`.

**Files:** `bench/baseline.linux-x86_64.json`, `scripts/bench-regression-check.sh`, `scripts/check.sh`, `.github/workflows/stream-a-perf.yml`, `crates/memorum-eval/tests/release_gate_contracts.rs`.

**Risk:** Small.

---

### W6.T2 — CI matrix: scheduled real-harness lane (Gap 16)

1. Add a `schedule`-triggered real-harness lane to `.github/workflows/stream-h-eval.yml` that runs when secrets are present, emits a neutral check when not.
2. Broaden the RC partial gate (currently `startsWith(github.ref, 'refs/tags/v1.')`) to match any `v*.*.*-rc.*` tag.
3. Add a required-status job for protected `main` branch (configured outside the repo, but documented in the workflow comments).
4. Update `crates/memorum-eval/tests/ci_workflow_shape.rs` to assert the new structure.

**Files:** `.github/workflows/stream-h-eval.yml`, `crates/memorum-eval/tests/ci_workflow_shape.rs`.

**Risk:** Small.

---

### W6.T3 — Install summary + `doctor` scheduler check (Gap 18)

1. Add a structured summary block at the end of `scripts/install-memorum.sh`: `daemon running: yes/no`, `launchd installed: yes/no`, `scheduler installed: yes/no`, `harness CLI: <name>/none`, `dreams active: yes/no`.
2. Add a `--full` flag that runs all of: substrate init, daemon serve, launchd daemon, launchd dream-scheduler, doctor.
3. Extend `crates/memoryd/src/handlers/doctor.rs` to check: launchd presence (on macOS), scheduler agent presence, harness CLI auth state. Flag combinations like "dreaming enabled but no scheduler" or "dreaming enabled but no harness CLI" as warnings.
4. Tests: `scripts/install-launchd.test.sh` extension; new `memoryd doctor` integration test.

**Files:** `scripts/install-memorum.sh`, `scripts/install-launchd.sh`, `crates/memoryd/src/handlers/doctor.rs`, install tests.

**Risk:** Small.

---

### W6.T4 — T17/T18 product contracts + un-defer (Gap 15)

This one is *not* purely operational — it requires shipping the underlying product contracts.

1. **T17 (`lease_contention_resolution`):** ship the missing lease reentrancy contract in `memorum-coordination`. The test today emits `SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED`. Implement reentrant lease acquisition for the same device + retry after release.
2. **T18 (`encrypted_tier_key_rotation`):** ship the missing key rotation contract in `memory-privacy`. The test emits `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED`. Implement DEK rotation preserving reads + forward secrecy.
3. Once both ship, flip `deferred: true` → `deferred: false` in `crates/memorum-eval/src/orchestrator.rs:357–380`.
4. Add a release gate: mock CI run with any `deferred: true` || `skipped` in the required set fails.
5. Update `crates/memorum-eval/tests/honesty.rs` to reflect that T17/T18 should now pass, not skip.

**Files:** `crates/memorum-coordination/src/leases.rs`, `crates/memory-privacy/src/encryptor.rs`, `crates/memorum-eval/src/orchestrator.rs`, T17/T18 domain test files, `tests/honesty.rs`, `.github/workflows/stream-h-eval.yml`.

**Risk:** Medium. The product contracts are real engineering work; the eval flip is trivial.

---

## Wave 7 — Explicit roadmap deferral (gap 6, ROI)

**Decision (Trey, 2026-05-19):** defer to v1.1.

### W7.T1 — Document ROI deferral

1. Amend `docs/specs/stream-g-observability-v0.1.md` §"Deferred v1.1+" with an explicit ROI section listing the metrics that would matter (promotion rate, promotion precision, refusal breakdown, dreaming ROI, Reality Check adherence) and the design question that's blocking (what "promotion precision" means operationally).
2. Leave the `deferred_response("roi")` helper in place; update its note to point at the spec section.
3. Close the gap in any GAPS-tracker artifact as "deferred-v1.1" rather than "open."

**Files touched:** `docs/specs/stream-g-observability-v0.1.md`, `crates/memoryd-web/src/routes/mod.rs` (helper note text).

**Risk:** None — pure documentation.

---

## Cross-cutting concerns

### Spec invariants at risk

- **W3 (gaps 1, 2, 3, 20):** invariants 1 (secret never persisted) and 2 (ClassificationOutcome required) are central. Every PR in W3 must include explicit verification that no plaintext touches disk on the encrypted-path code paths.
- **W3.T1 (Gap 1/encrypted source):** invariant 5 (`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`) — confirm encrypted artifacts don't need a schema bump for the merge driver; if they do, bump in lockstep.
- **W5 (gap 12):** privacy provider failure must default to *more conservative* behavior; document the fail-closed policy explicitly per namespace.

### Tooling and dependency notes

- W3.T2 (multi-adapter capture) adds `pdf-extract` (or `lopdf`) and `infer` for MIME detection. Audit licenses; both candidates are MIT/Apache-2 and Rust-native. Headless browser is **dropped** from scope (Trey decision 2026-05-19).
- W5.T1 adds no LLM provider crates — framework only. Actual model runtime dependencies (`tract`, `candle`, `reqwest`-based moderation client) come in when implementations land.

### Test discipline

Per W1.T4: every wave landing must add daemon-backed contract tests for the routes/protocol it touches. Fixture-only coverage is insufficient. The "test discipline" task in W1 is the policy; subsequent waves implement it concretely.

### Documentation updates

Each wave's PR description must include the affected spec/API docs:
- W1 → `docs/api/stream-g-observability-api.md`, `docs/runbooks/reality-check.md`
- W2 → `docs/api/stream-c-governance-api.md` (policy write), `docs/api/stream-g-observability-api.md`
- W3 → `docs/specs/stream-d-privacy-v0.1.md` amendment (encrypted source artifacts), `docs/api/stream-d-privacy-api.md`
- W4 → `docs/api/stream-g-observability-api.md` (notification delivery status)
- W5 → amend `stream-d-privacy-v0.1.md` with the provider framework + fail-mode matrix; defer the provider-implementation spec to whatever milestone ships them
- W6 → `docs/runbooks/release-gate.md` (new or amended), `docs/dev/eval-harness.md`
- W7 → amend `docs/specs/stream-g-observability-v0.1.md` §"Deferred v1.1+" with ROI requirements

---

## Decisions ratified 2026-05-19

All five plan-time questions resolved by Trey:

1. **W5 provider:** ship framework only, defer implementations. ✓
2. **W7 ROI:** defer to v1.1. ✓
3. **W3.T2 browser adapter:** dropped from scope (HTTP-static covers the dominant grounding pattern; headless browser cost not justified by demand). ✓
4. **Parallelism:** fan out as many concurrent waves as reasonable. ✓
5. **Branch model:** one branch per wave, merged independently to `main`; this plan lives on `gaps-fix/plan-v0.1`. ✓

---

## Out of scope

These are surfaced by the lane reports as adjacent issues but are not part of this plan:

- True push-based SSE over Unix socket for the notifications stream (W1.T2 uses polling; push is a v1.1 enhancement).
- Headless browser capture (deferred per W3.T2 recommendation).
- Reality Check session-level analytics (adherence trends, week-over-week comparisons) — covered by W2.T3 history surface, but the analytics layer on top is a separate UX project.
- Remote dashboard auth — explicit Stream G v1.1+ deferral per `CLAUDE.md`.
- Sync dashboard rich peer/device topology — explicit Stream G v1.1+ deferral per `CLAUDE.md`.

---

## Appendix A — Lane report archives

- Lane 1 (gaps 1, 2, 3, 20 — source capture): `/tmp/gaps-verify/lane1-report.md`
- Lane 2 (gaps 4, 5, 6, 7 — dashboard status family): `/tmp/gaps-verify/lane2-report.md`
- Lane 3 (gaps 8, 9, 10, 19 — interaction surfaces): `/tmp/gaps-verify/lane3-report.md`
- Lane 4 (gaps 11, 12, 13, 14 — daemon hardening): `/tmp/gaps-verify/lane4-report.md`
- Lane 5 (gaps 15, 16, 17, 18 — release gates): `/tmp/gaps-verify/lane5-report.md`

Lane reports include full code excerpts, line drift annotations, fix surface enumeration, and related-issue notes from each composer-2.5 verification pass.

---

## Appendix B — Wave dependency graph

```
W1.T1 ── W1.T2 ── W1.T3 ── W1.T4
                    │
                    └────── W2.T1 ── W2.T2 ── W2.T3 ── W2.T4
                                      
W3.T1 ── W3.T2  (independent of W1/W2)
W4.T1 ── W4.T2  (independent)
W5.T1           (blocked on provider decision)
W6.T1, T2, T3, T4 (parallel, independent)
W7.T1           (decision gate)
```

W1.T3 introduces the `DaemonScaffold` seeded with substrate/conflict/peer/dream/RC data; W2 reuses it. That's the single hard dependency between waves.

---

## Plan revision history

- **v0.1 (2026-05-19):** initial synthesis from five-lane composer-2.5 verification pass against GAPS.md commit `11c266b`.
