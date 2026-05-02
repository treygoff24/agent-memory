Verdict: Approved

# Stream G Review Gate D Security Rerun

## Scope reviewed

Rerun after remediation of the prior Gate D MCP boundary finding. Focus areas:

- MCP raw-payload forwarding boundary for `RequestPayload::PeerHeartbeat(_)`.
- Regression coverage proving rejection happens before socket I/O.
- Web dashboard process spawn and default router changes, specifically whether the production/default router now fails closed instead of serving fixture data and whether the new subprocess launch path introduces injection, exposure, or auth-boundary regressions.

## Findings

No blocking security findings.

## Original finding verification

### Closed: MCP forwarding rejects `PeerHeartbeat` before socket I/O

The prior finding was that `forward_payload_to_daemon` rejected web/reality/trust and most peer payloads, but allowed `RequestPayload::PeerHeartbeat(_)` to fall through to daemon socket forwarding.

Current code closes that gap:

- `RequestPayload::PeerHeartbeat(_)` is now included in the rejected MCP payload match arm at `crates/memoryd/src/mcp.rs:223-242`.
- The rejected arm returns `ProtocolError::method_not_allowed_on_mcp()` locally, rather than calling `client::request(...)`; the only daemon socket I/O path is the wildcard fallthrough at `crates/memoryd/src/mcp.rs:242`.
- `PeerHeartbeat` remains a daemon protocol variant at `crates/memoryd/src/protocol.rs:97` and would be handled by daemon peer-state mutation code if it reached dispatch (`crates/memoryd/src/handlers.rs:341`), so the MCP-side pre-socket rejection is the right boundary.
- Regression coverage constructs a real `RequestPayload::PeerHeartbeat(PeerHeartbeat { ... })`, uses a deliberately missing socket path, and asserts `method_not_allowed_on_mcp` at `crates/memoryd/tests/mcp_manifest.rs:89-111`. Because the socket path is missing, a passing test demonstrates local rejection before socket I/O.

Exploitability after remediation: low. The normal manifest still exposes only the nine agent-facing tools, and the raw-payload helper now blocks the previously missed peer heartbeat variant before any daemon connection attempt.

Impact after remediation: the specific fake peer-presence / heartbeat-driven claim-lock renewal vector identified in the original Gate D review is closed for MCP forwarding.

## Web process spawn / default router review

### Default router now fails closed

The production/default `router()` now builds `WebState::new()` / `WebState::unconfigured()` rather than fixture-backed state (`crates/memoryd-web/src/server.rs:45-58`, `crates/memoryd-web/src/server.rs:146-152`). Fixture data is still available, but only through the explicit `fixture_router()` / `WebState::fixture()` test path (`crates/memoryd-web/src/server.rs:60-72`, `crates/memoryd-web/src/server.rs:150-152`).

That means API handlers must see configured dashboard data before returning fixture-backed content:

- Status and SSE return `backend_unavailable` when `dashboard_data` is absent (`crates/memoryd-web/src/routes/status.rs:108-128`).
- Entity graph/detail return `backend_unavailable` when unconfigured (`crates/memoryd-web/src/routes/entity_graph.rs:114-128`).
- ROI returns `backend_unavailable` when unconfigured (`crates/memoryd-web/src/routes/roi.rs:65-70`).
- Reality Check GET/history/POST return `backend_unavailable` when unconfigured (`crates/memoryd-web/src/routes/reality_check.rs:71-105`).
- Review queue/action return `backend_unavailable` when unconfigured (`crates/memoryd-web/src/routes/review.rs:44-75`).
- Audit routes return `backend_unavailable` when unconfigured (`crates/memoryd-web/src/routes/audit.rs:68-122`).
- The shared fail-closed response is a `503` JSON body with `dashboard_backend_unavailable` (`crates/memoryd-web/src/server.rs:182-191`).

The regression test `test_default_router_does_not_serve_fixture_dashboard_data` asserts `router()` returns `503 Service Unavailable` for `/api/status` instead of fixture status data (`crates/memoryd-web/tests/api_contract.rs:27-37`). This directly covers the remediation claim.

### Web subprocess launch did not introduce command injection or remote exposure

The daemon-side web enable path validates the privileged port boundary, serializes dashboard runtime state behind the existing mutex, resolves a `memoryd-web` binary, and launches it via `std::process::Command::new(binary).arg(...)` without a shell (`crates/memoryd/src/handlers.rs:193-215`, `crates/memoryd/src/handlers.rs:367-372`). The socket path and port are passed as argv values, not interpolated into a shell command.

The spawned `memoryd-web` binary binds only to localhost:

- CLI parsing rejects ports below 1024 (`crates/memoryd-web/src/bin/memoryd-web.rs:23-45`).
- The binary constructs `WebConfig { bind_address: 127.0.0.1, ... }` directly (`crates/memoryd-web/src/bin/memoryd-web.rs:6-11`).
- `WebConfig::validate_localhost` rejects any non-`127.0.0.1` bind address (`crates/memoryd-web/src/config.rs:34-40`).
- `run` validates again before `TcpListener::bind` and uses the fail-closed default router (`crates/memoryd-web/src/server.rs:193-199`).

CSRF protection remains applied only to the mutating POST sub-router (`crates/memoryd-web/src/server.rs:154-158`), using a generated 32-byte token and exact header match (`crates/memoryd-web/src/auth.rs:17-40`). The relevant CSRF, contract, and concurrent mutation tests passed in this rerun.

Residual non-blocker: `resolve_memoryd_web_binary` falls back from a sibling binary to `PATH` lookup (`crates/memoryd/src/handlers.rs:3099-3112`). I do not treat that as a Gate D blocker because the daemon still uses argv-based process launch, the web command is admin/socket-only, and successful exploitation would require controlling the daemon's execution environment or PATH search locations. A future hardening pass could prefer a configured absolute binary path and make PATH fallback opt-in for packaged installs.

## Commands run

```bash
cargo test -p memoryd-web --test api_contract --test csrf --test concurrent_access
```

Result: passed (`14 + 1 + 8` tests).

```bash
cargo test -p memoryd --test cli_contract --test slash_commands --test trust_artifact --test protocol_contract --test mcp_manifest --test server_smoke --test responses
```

Result: passed (`18 + 4 + 8 + 16 + 13 + 4 + 17` tests).

```bash
cargo clippy -p memoryd -p memoryd-web --all-targets --all-features -- -D warnings
```

Result: passed.

```bash
cargo fmt -p memoryd -p memoryd-web -- --check
```

Result: passed.

## Residual risk and confidence

Residual risk is mostly integration drift: the default web router now correctly fails closed, but the production dashboard still needs a daemon-backed data adapter before these routes can serve real data. This rerun validates the current safe placeholder/fail-closed behavior and the MCP boundary remediation, not a future daemon-backed dashboard implementation.

Confidence: high for the closed `PeerHeartbeat` MCP boundary and current default-router behavior; medium-high for the web process spawn review because it was code/test based and did not include an end-to-end browser session against a spawned process.
