Verdict: Changes requested

# Stream G Final TEST-COVERAGE Review Rerun

Review scope: final Stream G test-coverage rerun after the prior `docs/reviews/stream-g-final-test-review.md` blockers. I reviewed the current tests, production code, Stream G spec/API docs, and reran focused plus broad validation. I did not modify production or test code.

## Summary

The two prior test-review blockers are resolved:

1. `memorum-eval` Test #19 now has cfg-aware expectations. Without `stream-i-deps`, `mock_harness_skips_test_19_without_stream_i_deps` asserts a `Skipped` outcome and exact reason; with `stream-i-deps`, `mock_harness_runs_test_19_with_stream_i_deps` asserts a `Passed` outcome plus attribution/directive-awareness flags (`crates/memorum-eval/tests/mock_harness_smoke.rs:31-68`). Both branches passed locally.
2. The web audit route and acceptance test now guard the normative top-level audit response shape. `AuditMemoryResponse` exposes top-level fields (`crates/memoryd-web/src/routes/audit.rs:14-31`), `/api/audit/:id` returns that DTO for fixture and daemon paths (`crates/memoryd-web/src/routes/audit.rs:116-133`), and `test_get_audit_returns_full_trust_artifact` asserts top-level fields while explicitly rejecting the old `artifact`/`sections` wrapper (`crates/memoryd-web/tests/api_contract.rs:101-121`). This now matches the Stream G spec/API docs (`docs/specs/stream-g-observability-v0.1.md:721-752`, `docs/api/stream-g-observability-api.md:154-178`).

However, I found one remaining high-risk Stream G test coverage gap: `memoryd web enable` subprocess/readiness success and failure modes are still not covered by a real test. Because the review brief explicitly called out web enable subprocess/readiness as a high-risk area, I am not approving this lane yet.

## Blocking finding

### 1. `memoryd web enable` subprocess/readiness path has no success-path or readiness-failure coverage

- Evidence: the production `WebDashboardRuntime::enable` path stops any old child, verifies port availability, resolves the `memoryd-web` binary, spawns it with `--socket`/`--port`, waits for localhost readiness, terminates on readiness failure, then records running status (`crates/memoryd/src/handlers.rs:237-267`). The readiness loop specifically handles early child exit and timeout (`crates/memoryd/src/handlers.rs:313-327`).
- Existing coverage is materially weaker than that production path:
  - CLI-level coverage only verifies that `memoryd web enable` maps to `RequestPayload::WebEnable { port, socket_path }` (`crates/memoryd/tests/cli_contract.rs:257-267`).
  - Protocol coverage only round-trips the web request variants (`crates/memoryd/tests/protocol_contract.rs:160-170`).
  - Handler unit coverage only checks the negative pre-spawn port-in-use guard (`crates/memoryd/src/handlers.rs:3317-3327`).
  - Web crate coverage checks router/config behavior, including localhost bind validation, but does not exercise daemon-owned subprocess spawning/readiness (`crates/memoryd-web/tests/csrf.rs:64-80`, `crates/memoryd-web/src/server.rs:216-242`).
- Why this matters: Stream G's user-facing web acceptance path is `memoryd web enable`, and the riskiest part is not clap parsing or the axum router; it is process orchestration and readiness. A regression in binary resolution, argv construction, readiness polling, child exit handling, timeout cleanup, or idempotent status could pass the current suite while making the dashboard impossible to start from the daemon.
- Concise fix recommendation: add a deterministic integration/unit seam around `WebDashboardRuntime` process spawning. Prefer an injectable launcher/readiness probe so tests can cover:
  1. success path records `running: true`, expected port/URL, and uses the exact `memoryd-web --socket <path> --port <port>` argv;
  2. child exits before binding -> `web_unavailable`, child cleaned up, status stopped;
  3. readiness timeout -> `web_unavailable`, child killed, status stopped;
  4. enabling the same port while a child is alive is idempotent and does not spawn a second child.

## High-risk coverage review

### Web enable subprocess/readiness

Not adequately covered. See blocking finding above.

### TUI daemon dispatch/retry

Covered by real daemon-protocol assertions, not just smoke tests:

- Production loop now drains queued daemon calls on each tick (`crates/memoryd-tui/src/app.rs:581-593`).
- Dispatch drains the queue, marks socket connected on success, keeps failed calls queued, and surfaces socket/error state (`crates/memoryd-tui/src/app.rs:324-340`).
- `DaemonClient` maps review actions to daemon protocol payloads (`crates/memoryd-tui/src/client.rs:53-99`) and Reality Check actions to `RequestPayload::RealityCheck(Respond { ... })` (`crates/memoryd-tui/src/client.rs:101-127`).
- Tests use a Unix socket fake daemon and assert actual outgoing `RequestPayload`s:
  - review approve reaches daemon and clears queue (`crates/memoryd-tui/tests/keymap.rs:147-165`);
  - daemon error remains queued/retryable and visible (`crates/memoryd-tui/tests/keymap.rs:167-187`);
  - Reality Check dispatch uses the selected row's memory id, not the title or first row (`crates/memoryd-tui/tests/keymap.rs:189-247`).

### Audit response shape/leakage

Covered:

- Top-level audit shape is asserted with specific fields and old wrapper rejection (`crates/memoryd-web/tests/api_contract.rs:101-121`).
- Non-audit route leakage coverage asserts the audit-only fixture body is absent from status/entity/ROI/Reality Check/review responses (`crates/memoryd-web/tests/api_contract.rs:228-247`).
- Trust-artifact daemon tests cover all sections, encrypted redaction, chronological provenance, policy fields, recall counts from events, and supersession projection (`crates/memoryd/tests/trust_artifact.rs:1-260` reviewed; broad test gate passed).

### Production scoring path

Covered with both behavioral and performance assertions:

- Production scorer batches over real index/event/supersession data (`crates/memoryd/src/reality_check/scoring.rs:23-68`, `crates/memoryd/src/reality_check/scoring.rs:137-180`).
- `test_score_memories_at_10k_fixture_under_500ms_p95` drives `score_memories_at` over 10,000 rows, asserts finite scores, exact scored count, and p95 <= 500ms (`crates/memoryd/tests/scoring.rs:306-328`).
- `stream_g_bench` also calls `memoryd::reality_check::score_memories_at` for `scoring_10k_memories` (`crates/memoryd/src/bin/stream_g_bench.rs:209-232`), and the canonical baseline records 10,000 input/scored memories (`bench/stream-g-observability-results.darwin-arm64.json:12-33`).

### cfg feature behavior

Prior blocker resolved:

- `mock_harness_smoke.rs` now asserts the no-feature skip reason (`crates/memorum-eval/tests/mock_harness_smoke.rs:31-49`) and the all-features pass metadata/output (`crates/memorum-eval/tests/mock_harness_smoke.rs:51-68`).
- Local reruns passed for both `cargo test -p memorum-eval --test mock_harness_smoke` and `cargo test -p memorum-eval --test mock_harness_smoke --all-features`.
- The broad `cargo test --workspace --all-targets --all-features` gate also passed locally, closing the prior all-features failure.

### Notification/reality-check behavior

Covered:

- Scheduling tests cover due, not-due, snoozed, overdue, invalid cron fallback, direct event firing, and shared handler notification channel (`crates/memoryd/tests/scheduling.rs:7-83`).
- Pending-attention tests cover due emission, suppression when not due/missing/snoozed, once-per-window behavior, no memory-content leakage, total-cap behavior, and XML version stability (`crates/memoryd/tests/reality_check_pending_attention.rs:15-155`).
- Dispatcher tests cover passive queue retention, OS enable/disable, Slack retry/fallback, webhook URL redaction, no memory-content Slack payload, lagged receiver continuation, SMTP secret env lookup, and email debug redaction (`crates/memoryd/tests/dispatcher.rs:24-230`).
- Reality Check response tests cover encrypted confirm/not-relevant metadata-only behavior, correct supersession, governance refusal without session advance, skip deferral, concurrent response serialization, and completion (`crates/memoryd/tests/responses.rs:312-423`, broad gate passed).
- MCP admin rejection covers web and Reality Check/admin payloads before socket I/O (`crates/memoryd/tests/mcp_manifest.rs:45-86`).

## Verification executed

Passed:

```bash
cargo test -p memorum-eval --test mock_harness_smoke
cargo test -p memorum-eval --test mock_harness_smoke --all-features
cargo test -p memoryd-web --test api_contract --test csrf
cargo test -p memoryd-tui --test keymap
cargo test -p memoryd --test scoring test_score_memories_at_10k_fixture_under_500ms_p95 -- --exact --nocapture
cargo test -p memoryd --test dispatcher
cargo test -p memoryd --test scheduling --test reality_check_pending_attention
cargo test -p memoryd --test mcp_manifest mcp_forward_rejects_admin_web_payloads_before_socket_io -- --exact
cargo test -p memoryd --test cli_contract test_memoryd_web_enable_delegates_to_daemon -- --exact
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
cargo test --workspace --all-targets --all-features
```

Note: I first ran `cargo test -p memoryd --test dispatcher --test scheduling --test reality_check_pending_attention --test responses test_correct_governance_refusal_does_not_advance_session`, but that command's filter meant only the `responses` test executed and the other listed test binaries reported zero tests. I corrected that by rerunning `dispatcher`, `scheduling`, and `reality_check_pending_attention` without the filter; those passed.

## Files inspected

- `docs/reviews/stream-g-final-test-review.md`
- `docs/specs/stream-g-observability-v0.1.md`
- `docs/api/stream-g-observability-api.md`
- `docs/reviews/stream-g-bench-evidence.md`
- `bench/stream-g-observability-results.darwin-arm64.json`
- `crates/memorum-eval/tests/mock_harness_smoke.rs`
- `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs`
- `crates/memoryd-web/src/routes/audit.rs`
- `crates/memoryd-web/src/server.rs`
- `crates/memoryd-web/src/config.rs`
- `crates/memoryd-web/tests/api_contract.rs`
- `crates/memoryd-web/tests/csrf.rs`
- `crates/memoryd-tui/src/app.rs`
- `crates/memoryd-tui/src/client.rs`
- `crates/memoryd-tui/tests/keymap.rs`
- `crates/memoryd/src/cli.rs`
- `crates/memoryd/src/main.rs`
- `crates/memoryd/src/handlers.rs`
- `crates/memoryd/src/reality_check/scoring.rs`
- `crates/memoryd/src/bin/stream_g_bench.rs`
- `crates/memoryd/src/recall/render.rs`
- `crates/memoryd/tests/cli_contract.rs`
- `crates/memoryd/tests/protocol_contract.rs`
- `crates/memoryd/tests/scoring.rs`
- `crates/memoryd/tests/scheduling.rs`
- `crates/memoryd/tests/reality_check_pending_attention.rs`
- `crates/memoryd/tests/dispatcher.rs`
- `crates/memoryd/tests/responses.rs`
- `crates/memoryd/tests/mcp_manifest.rs`
- `crates/memoryd/tests/trust_artifact.rs`

## Residual risk

The current suite is strong on protocol shape, daemon-backed TUI dispatch, audit response shape, privacy/leakage, Reality Check behavior, notification dispatch, and production scoring performance. The remaining release risk is specifically the daemon-owned web subprocess lifecycle. Until that path has deterministic success/failure/readiness coverage, a dashboard startup regression can slip through despite all current tests passing.
