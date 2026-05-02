# Stream G Fresh-Eyes Review (Claude, 2026-05-02)

Snapshot: 6095cf6
Method: spec/plan/code triangulation, read-only

---

## Verdict

Risks worth surfacing: no blockers found, but three risks deserve attention before final sign-off — one logic gap in `is_overdue`, one bench-file honesty concern around TUI synthetic measurements, and an IPv6 localhost exclusion that mismatches the spec's stated valid values.

---

## Blockers (must-fix before merge)

None found.

---

## Risks (worth surfacing, not blockers)

**R1 — `is_overdue` never fires when `last_completed_at` is `None`**

`crates/memoryd/src/reality_check/scheduling.rs:63–67`:

```rust
pub fn is_overdue(&self, state: &RealityCheckState, now: DateTime<Utc>) -> bool {
    state
        .last_completed_at
        .is_some_and(|last_completed_at| now.signed_duration_since(last_completed_at) > OVERDUE_WINDOW)
}
```

`is_some_and` returns `false` when `last_completed_at` is `None`. A user who has _never_ completed a Reality Check will never see the overdue notification, regardless of how long they've gone without running one. The spec (§5.5) defines overdue as `last_completed_at` more than 21 days ago, implying a fresh install that has never run is at least as overdue as a 21-day lapse. The `is_due` path correctly uses `is_none_or` (`scheduling.rs:54–57`) — the `is_overdue` branch should arguably do the same: treat `None` as "infinitely overdue."

The scheduling tests (`crates/memoryd/tests/scheduling.rs`) do not cover the `None → is_overdue` case, so there's no test catching this gap either.

Severity: risk, not blocker. The overdue notification channel (Slack/email + passive) is not correctness-critical to memory storage, but it is the spec's stated mechanism for nudging users who've lapsed entirely.

---

**R2 — TUI benchmark measurements are entirely synthetic, and the p95s are suspiciously low**

`bench/stream-g-observability-results.darwin-arm64.json`, `tui_panel_switch.measured_ms = 0.001` (budget 16 ms) and `tui_detail_modal_open.measured_ms = 0.001` (budget 32 ms).

The bench detail field says: `"fixture": "synthetic in-process key event to frame render"` and `"rendered_bytes_last_frame": 144`. A 144-byte frame with no actual terminal I/O and no daemon socket call does not exercise the render loop that matters. The spec's §12.1 budget is for input-to-visible-frame-change latency, which requires a real terminal backend and at least a mock socket response. The bench numbers look like they're measuring the time to apply a state transition in memory, not the end-to-end ratatui render path.

This is a risk because: (a) these numbers are what the canonical baseline records as evidence, and (b) if the real render path regresses, these synthetic measurements will not catch it. The spec says "TUI synthetic benchmark: N key events pumped; measure total wall time / N" — which is closer to what's done here — but the frame size (144 bytes for a full 8-panel TUI) suggests the mock backend isn't actually doing a full frame write.

Not a blocker for correctness. Flagging because the baseline is supposed to inform future regression detection.

---

**R3 — Web `validate_localhost` rejects `::1` (IPv6 loopback), contradicting the spec**

`crates/memoryd-web/src/config.rs:34–41`: `validate_localhost` passes only when `bind_address == DEFAULT_BIND_ADDRESS` where `DEFAULT_BIND_ADDRESS = IpAddr::V4(Ipv4Addr::LOCALHOST)` (127.0.0.1). Any other address, including `::1`, returns an error.

The spec §8 config notes say: `bind_address: "127.0.0.1"` as default, and `"0.0.0.0" rejected at config load with ERROR`. The spec also explicitly documents `127.0.0.1 or ::1` as the valid value set (§4.4, "The dashboard binds to `localhost` only"). A user on an IPv6-only system or a dual-stack system configured to prefer `::1` would hit a confusing error message saying `bind_address must be 127.0.0.1`. The fix would be to accept `IpAddr::V6(Ipv6Addr::LOCALHOST)` (::1) as well.

Risk not blocker: the server still only binds localhost in practice; this is a UX/compatibility gap rather than a security hole.

---

**R4 — `audit_walk` returns 501 deferred for daemon-backed state**

`crates/memoryd-web/src/routes/audit.rs:148`: The audit walk route (`GET /api/audit/:id/walk`) returns `deferred_response("audit_walk")` unconditionally — even in the daemon-backed path. The spec §4.3 defines this route as part of v1 (not v1.1+), and the API doc confirms it should return `ProvenanceWalkResponse`. The deferred sections explicitly listed in §11 (v1.1+) are policy editor and sync dashboard only; audit walk is not on that list.

This is a risk because the web Audit Explorer's "Walk provenance" feature is silently stubbed. A user clicking "Walk provenance" on the dashboard gets a 501 with no warning. This may be an intentional scope reduction by Codex during the 12-hour run but it is not documented as deferred in the spec.

---

## Nits (style/clarity, optional)

- `crates/memoryd/tests/scheduling.rs`: spec test `test_overdue_after_21_days` uses `Duration::days(22)` (strictly greater than 21). A test at exactly 21 days to verify the boundary behavior (strictly greater vs greater-or-equal) would close an edge case.
- `crates/memoryd/src/reality_check/scoring.rs:34`: opens a second `Index` connection via `open_index` directly rather than going through `Substrate::index`. This bypasses the substrate's mutex-guarded `Index` but is read-only, so it's safe. Worth a comment explaining why a separate read handle is opened (perf: avoids holding the substrate lock for multi-query batch scoring).
- `docs/api/stream-g-observability-api.md`: `RealityCheckRequest` in the API doc adds a `limit` field to `Run { session_id, namespace, limit }` that is not in the spec's §5.7 wire shapes (spec has only `session_id` and `namespace` on `Run`). The implementation (`protocol.rs:153`) has the `limit` field and it aligns with the API doc. The spec is the slight stale outlier here; not a code issue.
- The `DailyMetricsSummaryReady` variant test in `crates/memoryd/tests/dispatcher.rs` is present but the `daily_synthesis_summary` trigger path in the external notifier is not obviously exercised end-to-end. Low priority since the dispatcher architecture is symmetric across events.

---

## Coherence observations

The 12-hour run held together well architecturally. Stream G's implementation is coherent from the substrate additions (migration v4, `events_log` mirror, `memory_supersession`, `RecallHit` / `RealityCheck*` / `ClaimLockContention` event variants) through the daemon protocol (`RealityCheckRequest`/`Response`, `NotificationEvent`, `ProtocolErrorCode::MethodNotAllowedOnMcp`) through the TUI crate and web crate. The drift score formula in `scoring.rs` faithfully implements the spec's §5.1 weights and normalization functions, including the `NULL source_harness` conservative-floor behavior (tested at lines 134 and 149).

There is mild evidence of late-run scope pressure: `audit_walk` and `entity_graph` (daemon-backed path) stub to `deferred_response`, and the reality-check-history endpoint also stubs. These are not named deferred in the spec. The TUI panel implementations in `app.rs` are substantial (43 KB), but the bench measurements suggest they were validated against a lightweight mock rather than a full-fidelity terminal session. Overall naming and conventions are consistent throughout — no signs of mid-run terminology drift between early and late files. The atomic write pattern for state files (`state.rs`) mirrors Stream A's idiom correctly.

---

## Spec coverage matrix

| Spec section                                            | Implementing file:symbol                                                            | Status                                                             |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| §1.3 #1 — 4 new EventKind variants                      | `memory-substrate/src/events/log.rs:105–142`                                        | ok                                                                 |
| §1.3 #2 — events_log SQLite mirror + dual-write         | `memory-substrate/src/index/migrations.rs:122–165`, `api.rs:mirror_event_fail_soft` | ok                                                                 |
| §1.3 #2 — events_log_mirror_health + doctor wiring      | `api.rs:events_log_mirror_health`, `handlers.rs:1316–1328`                          | ok                                                                 |
| §1.3 #3 — memory_supersession join table                | `migrations.rs:migrate_v4`, `query.rs:sync_supersession`                            | ok                                                                 |
| §1.3 #4 — Frontmatter.original_confidence               | `model.rs` (field), `query.rs:upsert_memory_row_with_full_metadata`                 | ok                                                                 |
| §1.3 #5 — RecallIndexRow indexed_at + source_device     | `query.rs:row_to_recall_index_row`                                                  | ok                                                                 |
| §1.3 #6 — Daemon state files                            | `state.rs:{DaemonState,RcPendingCache,RcSessionStore}`                              | ok                                                                 |
| §1.3 #7 — Reality Check request/response variants       | `protocol.rs:151–330`                                                               | ok                                                                 |
| §1.3 #8 — MethodNotAllowedOnMcp                         | `protocol.rs:782–806`, `mcp.rs:229–244`                                             | ok                                                                 |
| §1.3 #9 — NotificationEvent broadcast (7 variants)      | `protocol.rs:332–341`                                                               | ok                                                                 |
| §1.3 #10 — RecallHit emission in recall path            | `recall/render.rs:emit_recall_hits`                                                 | ok                                                                 |
| §1.3 #11 — pending-attention reality_check_due item     | `reality_check_pending_attention.rs` (test), `recall/` (emission)                   | ok                                                                 |
| §3 — 8-panel TUI                                        | `memoryd-tui/src/app.rs`, `panels/`                                                 | ok                                                                 |
| §4 — Web dashboard (4 API sections)                     | `memoryd-web/src/routes/`                                                           | partial (audit_walk, entity_graph daemon path, rc_history stubbed) |
| §4.4 — CSRF token enforcement                           | `memoryd-web/src/auth.rs:require_csrf`, `server.rs:router`                          | ok                                                                 |
| §4.4 — localhost-only bind                              | `memoryd-web/src/config.rs:validate_localhost`                                      | partial (rejects ::1, see R3)                                      |
| §5.1 — Drift-risk scoring formula                       | `reality_check/scoring.rs`                                                          | ok                                                                 |
| §5.2 — Scheduling                                       | `reality_check/scheduling.rs`                                                       | partial (is_overdue None gap, see R1)                              |
| §5.3 — Session lifecycle + state files                  | `reality_check/session.rs`, `state.rs`                                              | ok                                                                 |
| §5.4 — User response actions                            | `handlers.rs` (RealityCheck handler), `responses.rs` (tests)                        | ok                                                                 |
| §5.5 — Stale sessions / overdue                         | `scheduling.rs:is_overdue`                                                          | partial (see R1)                                                   |
| §5.7 — Wire shapes                                      | `protocol.rs`                                                                       | ok                                                                 |
| §5.8 — Crash recovery semantics                         | `state.rs` (atomic writes, corrupt session rename)                                  | ok                                                                 |
| §6 — Notification dispatcher                            | `notifications/{dispatcher,passive,os,external}.rs`                                 | ok                                                                 |
| §7 — Trust artifact rendering                           | `trust_artifact.rs`, `memoryd-web/src/routes/audit.rs`                              | ok                                                                 |
| §9 — CLI surface                                        | `cli.rs` (ui, web, reality-check subcommands)                                       | ok                                                                 |
| §10 — Acceptance tests                                  | `crates/memoryd/tests/`                                                             | ok (tests present and asserting real behavior)                     |
| §11 — Deferred sections (policy editor, sync dashboard) | `server.rs:195–196` (501 routes)                                                    | ok                                                                 |
| §12 — Performance budgets                               | `bench/stream-g-observability-results.darwin-arm64.json`                            | partial (TUI measurements synthetic, see R2)                       |
