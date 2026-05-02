Verdict: Changes requested

# Stream G Final Review Gate E Test Coverage Review

Review scope: Stream G final test coverage only. I reviewed Stream G spec §10 acceptance coverage, vertical TDD evidence, fixture/snapshot determinism, and the requested negative paths: corrupt state files, socket unreachable, governance refusal in a Reality Check session, wrong CSRF, concurrent 409, MCP rejection, and production-router fixture leakage.

## Blockers

### 1. Broad final workspace test gate fails under `--all-targets --all-features`

The focused Stream G surface tests pass, but the broader final gate required by the Stream G plan is not green. `cargo test --workspace --all-targets --all-features` failed in `memorum-eval`:

- `crates/memorum-eval/tests/mock_harness_smoke.rs:31-39` expects mock test #19 to return `TestOutcome::Skipped` when `stream-i-deps` is disabled.
- The command returned `TestOutcome::Passed` instead, causing the panic at `crates/memorum-eval/tests/mock_harness_smoke.rs:38-39`.

This may be Stream H/I-adjacent rather than Stream G-owned, but Gate E cannot honestly approve while the repo-wide all-target/all-feature test gate fails.

### 2. The web audit acceptance test is a false positive against the normative spec shape

Spec §4.3 defines `GET /api/audit/:id` as a top-level audit object with fields such as `title`, `body`, `status`, `namespace`, `confidence`, `recall_count_total`, `recall_count_30d`, `last_recalled`, `provenance_chain`, `policy_decisions`, `privacy_scan`, `supersession_history`, and `sync_state` (`docs/specs/stream-g-observability-v0.1.md:721-752`).

The implementation returns a different shape: `AuditMemoryResponse { memory_id, artifact, sections }` (`crates/memoryd-web/src/routes/audit.rs:11-16`), and both fixture-backed and daemon-backed audit paths return that wrapper (`crates/memoryd-web/src/routes/audit.rs:70-77`, `crates/memoryd-web/src/routes/audit.rs:89-92`). The acceptance test encodes the implementation wrapper rather than the spec response shape: it asserts `response["artifact"]["id"]`, `response["artifact"]["body"]`, nested artifact arrays, and `response["sections"]` (`crates/memoryd-web/tests/api_contract.rs:101-115`).

Impact: `test_get_audit_returns_full_trust_artifact` can pass while a client written to the Stream G spec fails. This is a test-coverage blocker because the acceptance matrix reports the route covered, but the test does not protect the normative API contract.

## Acceptance matrix coverage

Covered and passing in focused gates:

- TUI §10.1: all named panel/keymap/socket/resize acceptance tests are present and pass. Evidence examples: panel tests at `crates/memoryd-tui/tests/panel_render.rs:25-120`, keymap tests at `crates/memoryd-tui/tests/keymap.rs:22-99`, socket-unreachable tests at `crates/memoryd-tui/tests/socket_unreachable.rs:12-40`, resize tests at `crates/memoryd-tui/tests/resize.rs:11-35`.
- Web §10.2: status/entity/review/audit/ROI/CSRF/concurrent tests are present and pass, except for the audit-shape false positive called out above. CSRF and concurrency are directly covered at `crates/memoryd-web/tests/csrf.rs:11-54` and `crates/memoryd-web/tests/concurrent_access.rs:9-30`.
- Reality Check §10.3: scoring, scheduling, and response tests are present and pass. Equivalent naming is used for some cases, e.g. `test_corroboration_requires_two_distinct_harnesses` covers the two-distinct-source assertion at `crates/memoryd/tests/scoring.rs:62-75`; the governance-refusal RC path is covered at `crates/memoryd/tests/responses.rs:398-423`.
- Notifications §10.4: dispatcher tests cover passive queue, OS notification enable/disable, Slack retry/fallback, no-memory-content payloads, and lagged receiver continuation at `crates/memoryd/tests/dispatcher.rs:25-180`.
- Trust artifact §10.5: daemon DTO tests and TUI render tests cover all sections, encrypted redaction, chronological provenance, and all policy decision fields (`crates/memoryd/tests/trust_artifact.rs:16-90`; `crates/memoryd-tui/tests/trust_artifact.rs:88-154`).

## Negative-path coverage

Covered:

- Corrupt `state.json`: defaults/fallback reason covered at `crates/memoryd/tests/daemon_state_files.rs:46-69`.
- Corrupt `reality-check-session.json`: renamed to a forensic `.corrupt-*` file at `crates/memoryd/tests/daemon_state_files.rs:189-207`.
- Socket unreachable: TUI unreachable and reconnect flows covered at `crates/memoryd-tui/tests/socket_unreachable.rs:12-40`.
- Governance refusal in RC session: refused correction does not advance session at `crates/memoryd/tests/responses.rs:398-423`.
- Wrong/missing CSRF: 403 tests at `crates/memoryd-web/tests/csrf.rs:11-27`.
- Concurrent review mutation 409: `crates/memoryd-web/tests/concurrent_access.rs:9-30`.
- MCP admin/UI rejection: local pre-socket rejection in `crates/memoryd/src/mcp.rs:223-242`, with web/peer coverage at `crates/memoryd/tests/mcp_manifest.rs:70-111` and Reality Check coverage at `crates/memoryd/tests/notification_channel.rs:37-52`.
- No fixture-backed production router: default router fails closed and daemon router attempts socket I/O rather than fixture data (`crates/memoryd-web/src/server.rs:165-199`; `crates/memoryd-web/tests/api_contract.rs:27-52`).

Non-blocking hardening gap: `reality-check-pending.json` has stale/fresh/round-trip/delete coverage (`crates/memoryd/tests/daemon_state_files.rs:124-168`), but I did not find an explicit corrupt-pending-cache test analogous to the `state.json` and session-file corrupt tests. The implementation path uses the shared `load_versioned_json` fallback (`crates/memoryd/src/state.rs:109-127`, `crates/memoryd/src/state.rs:228-252`), so I would add this as a small regression test but would not block solely on it.

## Vertical TDD evidence

The plan consistently required RED tests then GREEN implementation for Stream G tasks, e.g. the subagent contract and vertical TDD requirement at `docs/plans/2026-05-01-stream-g-observability.md:40-55`, and task-level RED/GREEN steps throughout the plan. Review artifacts show remediation loops with focused reruns, including Gate A coverage remediation and passing rerun evidence (`docs/reviews/stream-g-review-gate-a-clean-code-rerun.md:17-42`), Gate B security rerun passing evidence (`docs/reviews/stream-g-review-gate-b-security-rerun.md:104-127`), and Gate D security rerun passing evidence (`docs/reviews/stream-g-review-gate-d-security-rerun.md:70-88`). Task 17 also records a concrete RED/GREEN bench cycle (`docs/reviews/stream-g-bench-evidence.md:7-14`).

This is acceptable TDD-process evidence for a review lane, with one caveat: I did not independently reconstruct every subagent's first RED command from terminal logs.

## Snapshot and fixture determinism

- TUI tests render through `ratatui::backend::TestBackend` with fixed dimensions and `DaemonSnapshot::sample()` (`crates/memoryd-tui/tests/panel_render.rs:7-15`), so they are deterministic.
- Time-sensitive tests mostly use fixed timestamps in scoring/scheduling/trust-artifact paths (`crates/memoryd/tests/scoring.rs:11-48`, `crates/memoryd/tests/scheduling.rs:8-37`, `crates/memoryd/tests/trust_artifact.rs:213-218`).
- The current TUI panel tests are not committed snapshot-file comparisons despite the spec labeling `panel_render.rs` as snapshot tests (`docs/specs/stream-g-observability-v0.1.md:1661-1672`). They assert important frame fragments rather than full frames (`crates/memoryd-tui/tests/panel_render.rs:25-120`). I am not blocking on this because the tests are deterministic and target behavior, but a future hardening pass should add full-frame snapshots for layout regressions.

## Verification executed

Passed:

```bash
cargo test -p memoryd-tui --test panel_render --test keymap --test socket_unreachable --test resize --test trust_artifact
cargo test -p memoryd-web --test api_contract --test csrf --test concurrent_access
cargo test -p memoryd --test scoring --test scheduling --test responses --test dispatcher --test trust_artifact --test daemon_state_files --test notification_channel --test protocol_contract --test mcp_manifest
```

Failed:

```bash
cargo test --workspace --all-targets --all-features
```

Failure: `mock_harness_skips_test_19_when_stream_i_deps_feature_is_disabled` in `crates/memorum-eval/tests/mock_harness_smoke.rs:31-39`.

## Required fixes before this lane can approve

1. Fix or isolate the broad workspace test failure so `cargo test --workspace --all-targets --all-features` is green for Gate E.
2. Make the web audit route/test/spec contract consistent. Either adapt `/api/audit/:id` to the top-level spec shape and update `test_get_audit_returns_full_trust_artifact`, or explicitly revise the spec/API docs to the `{ memory_id, artifact, sections }` contract and add a test that guards that versioned contract.
