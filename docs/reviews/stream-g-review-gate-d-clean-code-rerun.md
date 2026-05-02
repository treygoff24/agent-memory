Verdict: Changes requested

# Stream G Review Gate D — Clean-code rerun after remediation

## Intended outcome

Gate D appears intended to close the prior clean-code/correctness blockers for the Stream G user-facing observability/admin surfaces: `memoryd web enable` should start and supervise a localhost dashboard, production `memoryd-web` should not serve plausible fixture data as if it were daemon state, and parsed Reality Check CLI options should survive the CLI/protocol/daemon boundary. I treated the remediation target as both safety (no false fixture data, no dropped options) and user-visible utility (an enabled dashboard should be able to talk to the daemon or clearly not claim a working dashboard).

## Executive summary

The Reality Check CLI/protocol remediation is closed: `Run` now carries `limit`, `Snooze` carries `until`, both CLI paths forward the parsed values, the session handler honors `limit`, and the daemon persists the requested snooze date. The fixture-backed production router issue is also closed in the narrow safety sense: `router()` now uses an unconfigured state and API routes return `503 dashboard_backend_unavailable` instead of hard-coded data, while tests explicitly opt into `fixture_router()` / `WebState::fixture()`.

However, the web-enable remediation still does not meet the product outcome. The daemon now spawns a `memoryd-web` child, but the child ignores the supplied daemon socket and starts `router()`, whose API routes are intentionally unconfigured and therefore return 503. That means `memoryd web enable` can now truthfully start a process, but the enabled dashboard cannot inspect or mutate real daemon state. There is also no readiness handshake after spawn, so bind/startup failures can still produce an immediate “running” response until a later status refresh notices the child exited.

## Original finding verification

1. `memoryd web enable` reported success without starting/supervising the web server — **partially closed**. The daemon now resolves and spawns `memoryd-web`, stores the child handle, kills it on disable, and refreshes stopped status when the child has exited. Remaining issue: success is returned before proving the child bound the port or can serve the dashboard, and the spawned dashboard is backendless.
2. `memoryd-web` default production router served fixture-backed plausible data — **closed for false-data safety, not for dashboard utility**. Default `router()` no longer installs fixtures and returns `503 dashboard_backend_unavailable` for API routes. Fixture data is behind explicit fixture constructors for tests. Remaining issue: the daemon-spawned binary uses that default backendless router, so `memoryd web enable` starts an unusable API surface.
3. Parsed Reality Check CLI options were dropped before daemon — **closed**. `RealityCheckRequest::Run` has `limit`; `Snooze` has `until`; `cli.rs`, `main.rs`, the handler, and session scoring preserve/honor the values. Targeted CLI/protocol regression tests pass.

## Findings

### [High] Business Logic — `memoryd web enable` starts a backendless dashboard that cannot serve real daemon data

- **Evidence:** `crates/memoryd/src/handlers.rs:194-219` spawns `memoryd-web --socket <socket> --port <port>` and returns `WebDashboardStatus::running`. But `crates/memoryd-web/src/bin/memoryd-web.rs:8-10` parses the socket into `_socket_path` and never uses it. `memoryd-web::run` then serves `router()` (`crates/memoryd-web/src/server.rs:193-199`), and `router()` is built from `WebState::new()` (`crates/memoryd-web/src/server.rs:146-148`), which is `WebState::unconfigured()` with `dashboard_data: None` (`crates/memoryd-web/src/server.rs:45-57`). API routes such as `/api/status` return `backend_unavailable` when `dashboard_data` is absent (`crates/memoryd-web/src/routes/status.rs:108-110`; common response at `crates/memoryd-web/src/server.rs:182-190`).
- **Why it matters:** The false fixture-data risk is fixed, but the user-facing command still does not achieve the Stream G dashboard outcome. A user can run `memoryd web enable`, receive an enabled localhost URL, open the dashboard, and hit 503s for the real observability/admin API instead of seeing daemon status, Reality Check, review queue, audit, ROI, or entity graph data.
- **Reasoning:** The remediation moved production behavior from “plausible but false data” to “safe but unavailable data.” That is safer, but the daemon child process has no backend/client path despite being given a socket path. The `socket_path` addition only changes protocol shape; it is not connected to `memoryd-web` route handling. This leaves the dashboard enabled status semantically misleading for the actual business workflow.
- **Recommendation:** Wire `memoryd-web` to a daemon-backed backend before reporting this as a working dashboard. Concretely, parse and store the socket path in `WebConfig`/`WebState`, construct a daemon client/backend for production `router()`, and have GET/POST API routes call the daemon/trust-artifact paths instead of requiring fixture `DashboardData`. If daemon-backed routes are intentionally deferred, change `memoryd web enable`/status/output to make the dashboard backend-unavailable state explicit rather than saying only that the dashboard is running.
- **Confidence:** High

### [Medium] Reliability — web enable returns running before validating that the child bound the port

- **Evidence:** `WebDashboardRuntime::enable` calls `Command::new(...).spawn()` and immediately sets `port`, `enabled_at`, and `child` before returning `Ok(self.status(now))` (`crates/memoryd/src/handlers.rs:204-219`). The actual bind happens later inside the child at `TcpListener::bind(address).await?` (`crates/memoryd-web/src/server.rs:193-197`). If bind fails because the port is already in use or the child exits during startup, the parent only discovers that on a later `refresh_status` call (`crates/memoryd/src/handlers.rs:240-247`).
- **Why it matters:** This preserves a reduced version of the original false-success class: `memoryd web enable` can report `running` even when no listener successfully starts. Port conflicts and immediate child startup failures are common enough operational cases that the CLI should not claim success before readiness is known.
- **Reasoning:** `Command::spawn()` proves only that the OS created a child process; it does not prove the web server bound `127.0.0.1:<port>` or is accepting requests. Because stdout/stderr are discarded (`crates/memoryd/src/handlers.rs:211-213`), the failure is also hard for the operator to diagnose from the enable command itself.
- **Recommendation:** Add a readiness handshake before returning running: wait briefly for either child exit or a successful localhost health/API request, or have the child signal readiness over an IPC channel/stdout line before stdio is detached. If the child exits or readiness times out, return `web_unavailable` and clear the runtime state. Add a regression that occupies the requested port and asserts `WebEnable` fails instead of reporting running.
- **Confidence:** High

## Non-blocking simplifications

- The `WebState` naming is now safer, but `DashboardData::default()` still lives in production route modules. Moving fixture builders into test support or naming them `fixture_*` throughout would further reduce the chance of accidentally reintroducing demo data into production construction paths.
- `reality_check_request_payload` calls `validate_snooze_until` twice for the same argument. This is harmless, but parsing once would make the helper easier to read.

## Test gaps

- Missing daemon/web lifecycle test proving `memoryd web enable` produces an HTTP listener that can serve at least a health/status route and that `web disable` stops it.
- Missing negative lifecycle test for port-in-use or immediate `memoryd-web` child startup failure; this is the path that would catch the remaining false-running response.
- Missing production-router/backend integration test proving a daemon socket path is used by `memoryd-web` to fetch real status/review/Reality Check data. Current tests prove default router returns 503 and fixture router serves fixture data, but not that the enabled dashboard can serve daemon data.

## Questions / uncertainties

- I did not rerun the full parent evidence set; I ran targeted regressions for the reviewed contracts and inspected the scoped code paths. The parent supplied broader successful test/clippy/fmt evidence.
- It is unclear whether a backendless 503 dashboard is considered an acceptable temporary state for this gate. If the product requirement is only “do not serve fixture data,” then finding 1 is a known deferral; if the requirement is “`memoryd web enable` enables a usable dashboard,” it should block.
- I did not review unrelated Stream H/I worktree changes outside the Gate D remediation paths.

## Validation performed

- `cargo test -p memoryd-web --test api_contract test_default_router_does_not_serve_fixture_dashboard_data` — passed.
- `cargo test -p memoryd --test cli_contract reality_check` — passed.
- `cargo test -p memoryd --test protocol_contract reality_check_request` — passed.
- `cargo test -p memoryd --test protocol_contract web_dashboard_requests` — passed.

## Positives

- The fixture-data remediation uses an explicit `fixture_router()` / `WebState::fixture()` split, which is much harder to misuse than the prior default fixture-backed router.
- The Reality Check protocol changes are clean and additive, with CLI helper, main path, serde round-trip, handler, and session scoring all aligned.
- Web child tracking is a real improvement over the previous in-memory flag: disable kills the child and status refresh clears exited children.
