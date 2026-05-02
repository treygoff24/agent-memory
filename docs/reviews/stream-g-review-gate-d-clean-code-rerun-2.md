Verdict: Changes requested

# Stream G Review Gate D clean-code rerun 2

## Intended outcome

Gate D is trying to make the Stream G local observability/admin dashboard truthful and usable through the daemon boundary. For this rerun I treated the two remaining blockers as the acceptance criteria: `memoryd web enable` must start a real `memoryd-web` child wired to the daemon socket, and it must not report `running` until the child itself has bound the localhost port.

## Executive summary

The rerun materially improves the prior state: `memoryd-web` now receives the daemon socket, `WebState::daemon(...)` exists, several routes call the daemon client, and the default router still fails closed instead of serving fixture data. However, the readiness blocker is not actually fixed. The parent readiness check only opens a TCP connection to the requested port; if another process already owns the port, the probe succeeds and `memoryd web enable` reports `running` even though the spawned `memoryd-web` child could not have bound that port. I also found that daemon backing is partial and the tests obscure that fact: several user-facing dashboard APIs still return deferred/unavailable responses when socket-backed, while tests named as daemon-dispatch tests use fixture state and only assert local recorders.

## Original blocker verification

1. `memoryd web enable` must not merely toggle in-memory state; it should start a real `memoryd-web` child wired to the daemon socket so API routes are daemon-backed rather than backendless/fixture-backed. **Partially fixed, with remaining scope risk.** The daemon now spawns a child with `--socket` and `--port`, and the child constructs `WebState::daemon(args.socket)`. Several routes use `memoryd::client::request(...)`. But daemon backing is not complete: entity graph/detail, ROI, Reality Check history, audit walk, and notifications are still deferred/unavailable on daemon-backed state.
2. `memoryd web enable` should not report running before the child has bound the localhost port. **Not fixed.** There is a TCP poll, but it proves only that something accepts connections on the port, not that the spawned child bound it. A port-in-use case can still return `running`.

## Blocker findings

### [High] Reliability - Readiness probe can report another process's listener as the web child

- **Evidence:** `WebDashboardRuntime::enable` starts `memoryd-web`, stores the `Child`, then calls `wait_for_web_dashboard_ready(port)` before returning `Ok(self.status(now))` (`crates/memoryd/src/handlers.rs:209-228`). The readiness helper only does `TcpStream::connect_timeout(&address, WEB_DASHBOARD_READY_POLL).is_ok()` against `127.0.0.1:<port>` (`crates/memoryd/src/handlers.rs:270-279`). It never checks whether the spawned child is still alive during the poll, never checks child exit after the connect succeeds, and cannot distinguish the child from an unrelated pre-existing listener. The child performs the real bind later in its own process via `TcpListener::bind(address).await?` (`crates/memoryd-web/src/server.rs:221-225`).
- **Why it matters:** The exact false-success class from the rerun remains for a common operational edge case. If the requested port is already occupied, the spawned `memoryd-web` process cannot bind it, but the parent's TCP probe can connect to the unrelated service and return success. The CLI can then print `Web dashboard enabled at http://localhost:<port>` while the dashboard child is dead or failing startup.
- **Reasoning:** `Command::spawn()` proves only that the child process was created. `TcpStream::connect_timeout` proves only that a listener exists at that address. Combining them does not prove that this child owns the listener. Because stdout/stderr are discarded at spawn (`crates/memoryd/src/handlers.rs:214-216`), the bind failure is also hidden from the enable command. `refresh_status` may clear state later if called after the child exits (`crates/memoryd/src/handlers.rs:249-256`), but that does not prevent the initial false `running` response.
- **Recommendation:** Make readiness child-owned. Options: pre-bind the listener in the daemon and pass it to the child; have the child emit a readiness signal after successful bind over a pipe/IPC channel that the parent reads before detaching; or poll both HTTP health and `child.try_wait()` and reject if the child exits, while also avoiding success on ports that were already listening before spawn. Add a regression that binds `127.0.0.1:0`, requests `WebEnable` on that occupied port, and asserts a `web_unavailable`/non-running response rather than success.
- **Confidence:** High

### [Medium] Tests / Business Logic - Socket-backed dashboard coverage is partial and tests still hide fixture behavior

- **Evidence:** The binary now passes the parsed socket to `WebState::daemon(args.socket)` (`crates/memoryd-web/src/bin/memoryd-web.rs:8-13`), and some routes do call the daemon: status (`crates/memoryd-web/src/routes/status.rs:113-125`), review queue/action (`crates/memoryd-web/src/routes/review.rs:46-83`, `crates/memoryd-web/src/routes/review.rs:110-188`), Reality Check list/respond (`crates/memoryd-web/src/routes/reality_check.rs:76-108`, `crates/memoryd-web/src/routes/reality_check.rs:137-229`), and audit/temporal (`crates/memoryd-web/src/routes/audit.rs:70-98`, `crates/memoryd-web/src/routes/audit.rs:134-170`). But several registered API routes still do not use the daemon-backed state: `/api/entity-graph` and `/api/entity-graph/:id` return `deferred_response(...)` when `daemon_socket` is present (`crates/memoryd-web/src/routes/entity_graph.rs:114-129`), `/api/roi` returns `deferred_response("roi")` (`crates/memoryd-web/src/routes/roi.rs:65-70`), `/api/reality-check/history` returns `deferred_response("reality_check_history")` (`crates/memoryd-web/src/routes/reality_check.rs:120-128`), `/api/audit/:id/walk` returns `deferred_response("audit_walk")` (`crates/memoryd-web/src/routes/audit.rs:100-109`), and `/api/notifications/stream` still returns `backend_unavailable` unless fixture data exists (`crates/memoryd-web/src/routes/status.rs:128-141`). The tests named `test_post_review_action_approve_calls_daemon` and `test_post_reality_check_respond_dispatches_to_daemon` instantiate `WebState::fixture()` and assert local recorder mutations, not daemon socket I/O (`crates/memoryd-web/tests/api_contract.rs:67-86`, `crates/memoryd-web/tests/api_contract.rs:154-186`).
- **Why it matters:** The rerun can be mistaken as proving a fully daemon-backed dashboard when it only proves a subset. Users who enable the dashboard can still hit 501/503 on major Stream G surfaces, and future reviewers may trust misleading test names that never exercise daemon forwarding.
- **Reasoning:** This is safer than the original fixture-backed production router, but it is still not a clean closure of the "API routes are daemon-backed" blocker. The implementation mixes three behavior modes in production routes: real daemon forwarding, explicit deferral, and backend-unavailable. That may be an acceptable product deferral only if surfaced as such, but the current enable/status wording still says the dashboard is simply running.
- **Recommendation:** Either finish daemon backing for the remaining Gate D routes, or narrow the product contract/output so `web enable` reports a partially available dashboard with explicit deferred routes. Rename the fixture-recorder tests to say they exercise fixture mode, and add socket-backed route tests using a fake Unix socket daemon for review action and Reality Check respond. Add coverage for the deferred/unavailable daemon-backed routes so the deferral is intentional rather than hidden.
- **Confidence:** High

## Non-blocking clean-code notes

- `DashboardData::default()` and route-level `fixture(...)` constructors remain in production modules (`crates/memoryd-web/src/routes/mod.rs:58-83` and related route files). The explicit `fixture_router()` split is good, but moving fixture builders under test support or naming the type `FixtureDashboardData` would further reduce future accidental production use.
- `reality_check_request_payload` validates `--until` twice (`crates/memoryd/src/cli.rs:722-727`). This is harmless, but parsing once would be clearer.

## Test gaps

- Missing regression for occupied-port `WebEnable`: another listener bound to the requested localhost port must not produce a running status.
- Missing lifecycle test proving the spawned `memoryd-web` child, not just any listener, is the process serving the enabled URL.
- Missing socket-backed web-route tests with a fake daemon for review action and Reality Check respond; current tests with daemon-dispatch names use fixture state.
- Missing explicit tests documenting the current daemon-backed 501/503 behavior for entity graph, ROI, Reality Check history, audit walk, and notifications stream.

## Focused validations run

- `cargo test -p memoryd-web --test api_contract` - passed, 15 tests.
- `cargo test -p memoryd --test cli_contract web` - passed, 4 tests.
- `cargo test -p memoryd --test protocol_contract web_dashboard_requests` - passed, 1 test.

## Residual context / scope notes

- I reviewed only the requested Gate D paths and did not attempt to adjudicate unrelated Stream H/I or substrate worktree changes.
- I did not edit production code. This file is the requested review artifact.
