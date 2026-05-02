Verdict: Approved

# Stream G Final TEST-COVERAGE Review Rerun 2

Review scope: final Stream G test-coverage rerun after `docs/reviews/stream-g-final-test-review-rerun.md` requested deterministic coverage for `memoryd web enable` subprocess/readiness lifecycle. This was a review-only pass. I inspected current code/tests and only wrote this review artifact.

## Summary

Approved. The prior blocking web-dashboard subprocess/readiness coverage finding is closed by a testable daemon-side lifecycle seam and focused tests that exercise the failure modes called out in the previous rerun. The earlier blockers also remain resolved:

1. `memoryd web enable` now has deterministic daemon-side subprocess/readiness lifecycle coverage.
2. `memorum-eval` Test #19 remains cfg-aware under both default and `--all-features` builds.
3. The web audit route remains aligned with the top-level Stream G API shape, and the acceptance test rejects the old wrapper shape.

I found no blocking or requested-change findings.

## Prior blocker: web subprocess/readiness lifecycle

Closed.

### Production behavior reviewed

- `HandlerState` owns a process-local `WebDashboardRuntime` behind a mutex, preserving daemon-owned lifecycle state rather than pushing subprocess state into CLI parsing (`crates/memoryd/src/handlers.rs:91-124`).
- The production launcher still resolves and spawns `memoryd-web` with the expected socket/port argv and detached stdio: `memoryd-web --socket <socket_path> --port <port>` (`crates/memoryd/src/handlers.rs:243-256`).
- `WebDashboardRuntime::enable` preserves the intended production semantics: same live port is idempotent; otherwise it stops any old child, checks localhost port availability before spawning, waits for readiness, terminates the child on readiness failure, then records running status only after readiness succeeds (`crates/memoryd/src/handlers.rs:303-323`).
- The production readiness loop still polls localhost and detects early child exit before timeout (`crates/memoryd/src/handlers.rs:369-383`), and termination kills a still-running child then waits/reaps it (`crates/memoryd/src/handlers.rs:386-390`).
- Daemon request handling continues to route `RequestPayload::WebEnable` through `web_enable_response`, including low-port validation and daemon-owned status response (`crates/memoryd/src/handlers.rs:526-538`).

By inspection, the seam keeps production behavior equivalent while making the high-risk process lifecycle testable.

### Coverage reviewed

The new unit seam is meaningful for the previous blocker:

- `WebDashboardLauncher` / `WebDashboardChild` isolate spawn, readiness, `try_wait`, kill, and wait operations (`crates/memoryd/src/handlers.rs:229-238`).
- `web_dashboard_enable_success_records_running_status_and_spawn_argv` verifies successful enable records running status, port, URL, and the socket/port launch contract passed to the launcher (`crates/memoryd/src/handlers.rs:3492-3511`).
- `web_dashboard_enable_child_exit_before_binding_cleans_up_and_stops_status` verifies early child-exit failure maps to `web_unavailable`, leaves status stopped, does not kill an already-exited child, and waits/reaps it (`crates/memoryd/src/handlers.rs:3513-3528`).
- `web_dashboard_enable_readiness_timeout_kills_child_and_stops_status` verifies readiness timeout maps to `web_unavailable`, leaves status stopped, kills the still-running child, and waits/reaps it (`crates/memoryd/src/handlers.rs:3530-3545`).
- `web_dashboard_enable_same_live_port_is_idempotent_without_second_spawn` verifies same-port live reuse does not spawn a second child (`crates/memoryd/src/handlers.rs:3547-3560`).
- `web_dashboard_enable_rejects_preoccupied_port_before_spawn` verifies the pre-spawn localhost port guard returns `port_in_use` and does not record running status (`crates/memoryd/src/handlers.rs:3562-3574`).
- The CLI contract still verifies `memoryd web enable` delegates to the daemon payload with the default port and socket (`crates/memoryd/tests/cli_contract.rs:256-268`).

This closes the prior gap because regressions in daemon lifecycle state transitions, cleanup, idempotency, and failure mapping are now covered without depending on timing-sensitive real subprocess startup.

Non-blocking residual risk: the unit tests intentionally use a fake launcher rather than spawning the real binary. That is the right tradeoff for determinism. The production `OsWebDashboardLauncher` argv construction and `wait_for_web_dashboard_ready` polling logic are therefore protected by code review plus clippy/fmt, not by a direct subprocess integration test. I do not consider that a release blocker because the previous blocker asked for deterministic lifecycle coverage, and the current seam covers the load-bearing daemon behavior.

## Prior blocker: memorum-eval cfg-aware T19

Still resolved.

- `TestOutcome` now supports `Skipped`, so the mock harness can report feature-gated tests explicitly (`crates/memorum-eval/src/harness_runner.rs:54-58`).
- Without `stream-i-deps`, mock test #19 returns `Skipped` with the exact reason explaining that peer-update framing requires `memorum-coordination::framing_tests::assert_framing` (`crates/memorum-eval/src/harness_runner.rs:296-309`).
- With `stream-i-deps`, mock test #19 calls `assert_framing` and returns explicit attribution/directive/awareness outputs (`crates/memorum-eval/src/harness_runner.rs:367-388`).
- The smoke tests cover both compile-time branches: default feature skip (`crates/memorum-eval/tests/mock_harness_smoke.rs:31-49`) and all-features pass metadata/output (`crates/memorum-eval/tests/mock_harness_smoke.rs:51-68`).
- The standalone regression test remains cfg-gated, so default builds skip cleanly while `stream-i-deps` builds compile the real sampling matrix (`crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs:14-20`, `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs:44-60`).
- The feature is declared as optional dependency activation (`crates/memorum-eval/Cargo.toml:8-13`).

Both validation branches passed locally in this rerun.

## Prior blocker: web audit top-level shape

Still resolved.

- `AuditMemoryResponse` now exposes the Stream G audit/trust fields at top level: `memory_id`, title/body/status/namespace/confidence, recall fields, provenance, policy, privacy, supersession, and sync state (`crates/memoryd-web/src/routes/audit.rs:14-31`).
- Fixture-backed and daemon-backed `/api/audit/:id` both return `AuditMemoryResponse::from_artifact(...)`, not the old `{ artifact, sections }` wrapper (`crates/memoryd-web/src/routes/audit.rs:116-133`).
- The acceptance test asserts the normative top-level fields and explicitly rejects the old `artifact` and `sections` keys (`crates/memoryd-web/tests/api_contract.rs:101-121`).
- The Stream G spec and API docs both describe top-level audit object fields for `GET /api/audit/:id` (`docs/specs/stream-g-observability-v0.1.md:721-752`, `docs/api/stream-g-observability-api.md:154-178`).
- The non-audit leakage canary remains in place to ensure the audit-only body is not surfaced from unrelated routes (`crates/memoryd-web/tests/api_contract.rs:228-247`).

The focused audit contract test passed locally in this rerun.

## Findings

None. Approved.

## Validation executed

Passed:

```bash
cargo test -p memoryd web_dashboard -- --nocapture
cargo test -p memoryd --test cli_contract test_memoryd_web_enable_delegates_to_daemon -- --exact
cargo test -p memorum-eval --test mock_harness_smoke
cargo test -p memorum-eval --test mock_harness_smoke --all-features
cargo test -p memoryd-web --test api_contract test_get_audit_returns_full_trust_artifact -- --exact
cargo fmt -p memoryd -- --check
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Notes:

- The `cargo test -p memoryd web_dashboard -- --nocapture` filter ran the 5 new `handlers::tests::web_dashboard_*` unit tests and the existing web protocol serde test.
- The chained validation command briefly waited on Cargo package/build locks, then completed successfully.

## Files inspected

- `docs/reviews/stream-g-final-test-review.md`
- `docs/reviews/stream-g-final-test-review-rerun.md`
- `docs/specs/stream-g-observability-v0.1.md`
- `docs/api/stream-g-observability-api.md`
- `crates/memoryd/src/handlers.rs`
- `crates/memoryd/tests/cli_contract.rs`
- `crates/memorum-eval/Cargo.toml`
- `crates/memorum-eval/src/harness_runner.rs`
- `crates/memorum-eval/tests/mock_harness_smoke.rs`
- `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs`
- `crates/memoryd-web/src/routes/audit.rs`
- `crates/memoryd-web/tests/api_contract.rs`

## Residual risk

No release-blocking test-coverage risk found in the reviewed scope. The only residual hardening opportunity is a future micro-test around production command construction or the concrete `wait_for_web_dashboard_ready` loop, but the current fake-launcher lifecycle tests are deterministic and cover the previously missing daemon behavior.
