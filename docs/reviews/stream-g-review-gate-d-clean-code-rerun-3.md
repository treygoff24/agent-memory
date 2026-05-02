Verdict: Approved

# Stream G Review Gate D clean-code rerun 3

## Intended outcome

This rerun is intended to verify that the Gate D remediation closed the high readiness blocker from `docs/reviews/stream-g-review-gate-d-clean-code-rerun-2.md`: `memoryd web enable` must not report a dashboard as running when an unrelated listener already owns the requested localhost port, and readiness should observe spawned child early exit instead of treating any TCP listener as success.

## Executive summary

No material issues found. The high blocker from rerun 2 is remediated for the practical preoccupied-port case: `WebDashboardRuntime::enable` now rejects an already-bound localhost port before spawning `memoryd-web`, and the readiness loop checks `child.try_wait()` before accepting TCP readiness. The implementation is still not a fully child-owned readiness protocol because it releases the probe bind before spawning and then polls TCP, but that residual TOCTOU race is narrow and not enough to block this rerun. The remaining daemon-backed route scope and misleading fixture test names remain non-blocking cleanup items rather than Gate D blockers.

## Findings

No material issues found.

## Blocker verification

- Preoccupied-port false success is fixed for the reported blocker. `WebDashboardRuntime::enable` stops any existing child, calls `ensure_web_dashboard_port_available(port)`, and maps a bind failure to `port_in_use` before resolving or spawning `memoryd-web` (`crates/memoryd/src/handlers.rs:207-220`, `crates/memoryd/src/handlers.rs:266-271`). The regression test binds `127.0.0.1:0`, calls `runtime.enable(...)` on that occupied port, asserts `port_in_use`, and asserts the runtime is not running (`crates/memoryd/src/handlers.rs:3234-3246`).
- Spawned child early exit is now accounted for during readiness. `wait_for_web_dashboard_ready` receives `&mut Child`, polls `child.try_wait()`, and returns `web_unavailable` if the child exits before the TCP readiness check succeeds (`crates/memoryd/src/handlers.rs:220-223`, `crates/memoryd/src/handlers.rs:273-288`).
- The web child still binds its own localhost listener through `memoryd-web` with daemon-backed state (`crates/memoryd-web/src/bin/memoryd-web.rs:8-13`, `crates/memoryd-web/src/server.rs:216-241`).
- The default web router continues to fail closed rather than serving fixtures, and daemon state attempts daemon socket I/O for status (`crates/memoryd-web/src/server.rs:165-199`, `crates/memoryd-web/tests/api_contract.rs:27-52`).

## Non-blocking simplifications

- Child-owned readiness could be made stronger by passing a pre-bound listener/file descriptor to the child or by having the child emit a readiness signal after successful bind. The current pre-bind-then-spawn approach fixes the already-owned-port failure, but a process that grabs the port in the narrow window between `ensure_web_dashboard_port_available` and the child bind could still make the TCP probe ambiguous (`crates/memoryd/src/handlers.rs:207-220`, `crates/memoryd/src/handlers.rs:273-288`). I would not block Gate D on this unless the product needs adversarial/process-race hardening.
- `wait_for_web_dashboard_ready` is short and readable, but the readiness contract would be clearer if its name or documentation said it proves "some listener is accepting while child remains alive" rather than strict child ownership.
- `DashboardData::default()` and fixture constructors remain in production route modules. The `fixture_router()` split keeps production default behavior safe, but moving fixture builders into test/support modules would reduce accidental future fixture use.

## Test gaps

- There is direct coverage for the important preoccupied-port regression (`crates/memoryd/src/handlers.rs:3234-3246`) and it passed locally.
- There is not yet a deterministic test for child early exit during readiness. The production code now handles it via `child.try_wait()`, but a fake child/binary test would make that branch regression-proof.
- There is no test for the narrow race where another process binds the port after `ensure_web_dashboard_port_available` releases it but before the child binds. This is a robustness gap, not a current blocker.
- Daemon-backed route coverage remains partial. Status, review queue/action, Reality Check list/respond, audit, and audit temporal have daemon-backed code paths, while entity graph/detail, ROI, Reality Check history, audit walk, and notifications stream remain deferred/unavailable when daemon-backed (`crates/memoryd-web/src/routes/entity_graph.rs:114-129`, `crates/memoryd-web/src/routes/roi.rs:65-70`, `crates/memoryd-web/src/routes/reality_check.rs:120-128`, `crates/memoryd-web/src/routes/audit.rs:100-109`, `crates/memoryd-web/src/routes/status.rs:128-141`). This should stay explicit in product/release notes if the dashboard is presented as partially available.
- The tests named `test_post_review_action_approve_calls_daemon` and `test_post_reality_check_respond_dispatches_to_daemon` still use `WebState::fixture()` and assert local recorder mutations, not daemon socket forwarding (`crates/memoryd-web/tests/api_contract.rs:67-86`, `crates/memoryd-web/tests/api_contract.rs:154-187`). Rename them or add true fake-daemon socket tests in a follow-up.

## Focused validations run

- `cargo test -p memoryd handlers::tests::web_dashboard_enable_rejects_preoccupied_port_before_spawn` - passed.
- `cargo test -p memoryd-web --test api_contract` - passed, 15 tests.

## Questions / uncertainties

- I did not run the full orchestrator gate again because it was already reported successful and the focused rerun covered the changed blocker surface. I relied on the orchestrator-reported passes for `cargo test -p memoryd --test cli_contract web`, `cargo clippy -p memoryd -p memoryd-web --all-targets --all-features -- -D warnings`, and `cargo fmt -p memoryd -p memoryd-web -- --check`.
- I did not review unrelated large Stream G changes outside the requested Gate D files.

## Positives

- The blocker fix is small and easy to reason about: pre-spawn bind rejection handles the real user-facing false-running case without adding complex IPC.
- Readiness now considers child liveness during the poll, which closes the obvious early-exit gap from rerun 2.
- The added regression test is direct, fast, and behavior-focused.
