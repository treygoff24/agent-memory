Verdict: Changes requested

# Stream G Review Gate D — Clean-code / Correctness Review

## Intended outcome

Tasks 14-16 appear intended to make the Stream G user-facing observability/admin surfaces usable through the daemon boundary: a localhost-only web dashboard with CSRF-protected mutating routes, harness slash-command formatting for Reality Check, and CLI/protocol/admin-daemon commands for `memoryd ui`, `memoryd web`, and `memoryd reality-check`. The business outcome is that users can safely inspect and act on real daemon state without exposing admin surfaces through MCP or leaking raw memory bodies.

## Executive summary

The slash-command formatter and most protocol/MCP separation choices are directionally good, and the parent evidence says the targeted tests, clippy, and fmt gates pass. However, the Gate D slice is not ready to ship because the web dashboard is still effectively a fixture router and `memoryd web enable` does not start, configure, or supervise that router. That creates a dangerous false-success path: the CLI reports a dashboard URL as enabled while no dashboard server exists, and the standalone web router, if started separately, returns hard-coded Task 14 data and records POSTs in local test vectors instead of calling the daemon. I also found CLI option contract drift for `--top-n` on interactive Reality Check runs and `snooze --until`; both parse successfully but are discarded before reaching the daemon.

## Findings

### [High] Correctness — `memoryd web enable` reports success without starting a web server

- **Evidence:** `crates/memoryd/src/handlers.rs:186-213` defines `WebDashboardRuntime` as only `port` and `enabled_at`. `web_enable_response` just calls `dashboard.enable(port, now)` and returns `WebDashboardStatus` (`crates/memoryd/src/handlers.rs:316-322`). The memoryd crate does not depend on or call `memoryd-web` (`crates/memoryd/Cargo.toml:1-28`), and the only `memoryd-web::run` entry point is isolated in `crates/memoryd-web/src/server.rs:163-185`.
- **Why it matters:** A user can run `memoryd web enable`, receive `Web dashboard enabled at http://localhost:7137`, and still have nothing listening on that port. This misses the Task 16 CLI outcome and makes the operational status surface actively misleading.
- **Reasoning:** The daemon protocol state and the actual Axum server lifecycle are disconnected. The implementation models web enablement as an in-memory status flag, not as starting/configuring a server task, persisting config, or validating a real bind. The CLI output at `crates/memoryd/src/main.rs:510-521` treats that status as a real enabled dashboard URL.
- **Recommendation:** Wire `WebEnable`/`WebDisable` to a real web-dashboard lifecycle boundary. Concretely: the daemon should either own a dashboard task handle and start `memoryd-web` on `127.0.0.1:<port>` with graceful shutdown, or the CLI should stop claiming enablement and the design should be explicitly changed to a separate `memoryd-web` process. Add a test that runs the daemon handler/lifecycle far enough to prove an HTTP GET to `/api/status` succeeds after enable and fails or reports stopped after disable.
- **Confidence:** High

### [High] Correctness — Web dashboard API routes are fixture-only production behavior

- **Evidence:** `WebState::new()` always installs `DashboardData::default()` (`crates/memoryd-web/src/server.rs:44-57`), and `router()` exposes that state as the default production router (`crates/memoryd-web/src/server.rs:131-160`). `DashboardData::default()` hard-codes status, entity graph, ROI, Reality Check items, audit artifact, reviewable IDs, and notifications (`crates/memoryd-web/src/routes/mod.rs:58-83`). Routes then read those fixtures directly: `/api/status` clones fixture status (`crates/memoryd-web/src/routes/status.rs:108-110`), `/api/reality-check` returns fixture items (`crates/memoryd-web/src/routes/reality_check.rs:70-79`), `/api/audit/:id` rewrites the fixture artifact id (`crates/memoryd-web/src/routes/audit.rs:67-72`), `/api/review/action` only records into an in-memory vector (`crates/memoryd-web/src/routes/review.rs:69-85`), and `/api/reality-check/respond` only records into an in-memory vector (`crates/memoryd-web/src/routes/reality_check.rs:92-103`).
- **Why it matters:** The dashboard can display plausible but false daemon state and can accept review/Reality Check actions without mutating the real daemon/session/governance state. That is worse than an unimplemented dashboard because users may trust stale fixture data or believe an admin action succeeded.
- **Reasoning:** The tests assert that local vectors were appended (`crates/memoryd-web/tests/api_contract.rs:40-59`, `crates/memoryd-web/tests/api_contract.rs:127-160`), not that daemon `RequestPayload` values were sent. The Task 14 contract requires web routes to call the daemon/trust-artifact paths and serialize daemon errors, with fixture data limited to tests/mocks. Here the fixture object is the production state model.
- **Recommendation:** Introduce an explicit dashboard backend/client abstraction with a real daemon implementation and a test/mock implementation. Production `router()` should require or construct the daemon-backed implementation, while tests can call `router_with_state`/`router_with_backend` with fixtures. POST routes should forward `ReviewApprove`/`ReviewReject`/Reality Check payloads through the daemon socket and map real protocol errors to HTTP status codes. GET audit should call `TrustArtifactBuilder` or daemon `TrustArtifact`; status/ROI/entity/reality-check routes should derive from daemon/substrate state or return a clearly documented 501 until backed by real data.
- **Confidence:** High

### [Medium] API Contract — Parsed Reality Check CLI options are dropped before the daemon

- **Evidence:** `RealityCheckRunArgs` parses `--top-n` and `--json` (`crates/memoryd/src/cli.rs:128-144`), but non-JSON `memoryd reality-check run --top-n N` maps to `RealityCheckRequest::Run { session_id: None, namespace }` and discards `top_n` (`crates/memoryd/src/main.rs:287-301`; helper mirror at `crates/memoryd/src/cli.rs:708-717`). The protocol `Run` variant has no `limit` field (`crates/memoryd/src/protocol.rs:150-156`). Similarly, `RealityCheckSnoozeArgs` parses `--until` (`crates/memoryd/src/cli.rs:147-154`), but main only validates it and sends `RealityCheckRequest::Snooze` with no date (`crates/memoryd/src/main.rs:312-321`); the handler always sets `now + 7 days` (`crates/memoryd/src/handlers.rs:549-554`).
- **Why it matters:** Users receive accepted CLI syntax that does not do what the spec promises. `--top-n` only works in JSON/list mode, not for the interactive run described in spec §9.5, and `snooze --until 2026-05-10` silently snoozes for the daemon's hard-coded default instead.
- **Reasoning:** This is a contract drift between clap parsing, daemon protocol shape, and handler behavior. The tests cover parsing and helper mapping for JSON mode/invalid date only (`crates/memoryd/tests/cli_contract.rs:233-283`), so they do not catch the lost values.
- **Recommendation:** Extend the protocol before shipping the CLI contract: add `limit: Option<usize>` to `RealityCheckRequest::Run` and `until: Option<DateTime<Utc>>` or `Option<NaiveDate>` to `RealityCheckRequest::Snooze`, forward the parsed values from `main.rs`/helpers, and have the handler honor them. Add protocol round-trip tests plus CLI contract tests for `run --top-n` in non-JSON mode and `snooze --until` reaching the daemon request.
- **Confidence:** High

## Non-blocking simplifications

- Once the web dashboard has a real backend boundary, move most Task 14 fixture constructors out of production modules into test support. Keeping large fixtures in route modules makes it too easy for future contributors to mistake demo data for real behavior.
- Consider making `memoryd-web`'s default exported `router()` impossible to call without an explicit backend/config. A production-safe constructor and a clearly named `fixture_router()` would make misuse harder.

## Test gaps

- No test proves `memoryd web enable` starts an HTTP listener or that `web disable` drains/stops it.
- Web API tests assert fixture/local-recorder behavior instead of daemon forwarding for review actions, Reality Check responses, status, audit, ROI, or entity graph data.
- No CLI/protocol test covers non-JSON `memoryd reality-check run --top-n` forwarding a limit.
- No CLI/protocol/handler test covers `memoryd reality-check snooze --until <date>` preserving and honoring the requested date.

## Questions / uncertainties

- I did not rerun the parent gates; I relied on the provided passing evidence and inspected the scoped files directly.
- It is unclear whether the intended architecture is daemon-owned web task or separate `memoryd-web` process. The plan says the web server runs as a Tokio task inside `memoryd`, but the current crate dependency direction prevents `memoryd` from directly calling `memoryd-web` without refactoring.
- I did not review broader Stream H/I worktree changes outside the requested Gate D scope.

## Positives

- CSRF enforcement is centralized on the known POST routes and covered by negative tests.
- MCP rejection for admin/UI payloads uses a stable `method_not_allowed_on_mcp` path, preserving the admin-surface boundary.
- The slash-command formatter uses `safe_plaintext_fragment` and has focused tests for encrypted/sensitive-title suppression.
