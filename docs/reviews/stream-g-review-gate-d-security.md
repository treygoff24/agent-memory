Verdict: Changes requested

# Stream G Review Gate D Security Review

## Scope reviewed

Gate D after Tasks 14-16, focused on:

- Web dashboard CSRF, localhost binding, SSE content, non-audit body leakage, and concurrent mutation behavior:
  - `crates/memoryd-web/**`
- Slash command privacy and safe-title handling:
  - `crates/memoryd/src/slash_commands.rs`
  - `crates/memoryd/tests/slash_commands.rs`
- CLI/admin protocol/MCP exclusion:
  - `crates/memoryd/src/cli.rs`
  - `crates/memoryd/src/main.rs`
  - `crates/memoryd/src/handlers.rs`
  - `crates/memoryd/src/protocol.rs`
  - `crates/memoryd/src/mcp.rs`
  - related tests

## Findings

### Medium: MCP payload gate still forwards `PeerHeartbeat` instead of rejecting all peer-state/admin payloads

**Files:**

- `crates/memoryd/src/protocol.rs:97`
- `crates/memoryd/src/mcp.rs:223-242`
- `crates/memoryd/src/handlers.rs:290`
- `crates/memoryd/src/handlers.rs:343-363`

**What happens:**

`RequestPayload::PeerHeartbeat(PeerHeartbeat)` is a daemon protocol variant and the daemon dispatch path handles it as a mutating peer-presence/claim-lock-renewal request. The MCP forwarding guard rejects `TrustArtifact`, `Web*`, `RealityCheck`, `PeerStatus`, `PeerActivity`, and `PeerReleaseLock`, but it does not include `PeerHeartbeat` in the rejected match arm. Anything reaching `forward_payload_to_daemon(..., RequestPayload::PeerHeartbeat(...))` falls through to `client::request` and is sent to the daemon socket.

The normal nine-tool MCP manifest path does not expose a peer-heartbeat tool, so this is not directly exploitable through `ToolRequest` as currently declared. The problem is still an authorization-boundary gap in the public MCP payload gate: the code path whose job is to reject known admin/UI payloads before socket I/O has a peer-state mutation exception.

**Exploitability:**

Low-to-moderate today. A standard MCP client limited to manifest tool names cannot construct this through `forward_to_daemon`. A raw-payload MCP adapter, future bridge, or internal caller that uses the public `forward_payload_to_daemon` helper can forward a forged heartbeat over the daemon socket.

**Impact:**

A forged heartbeat can poison daemon peer-presence state and participate in heartbeat-driven claim-lock renewal semantics. That can make the dashboard/CLI/recall surfaces show fake active sessions or stale lock ownership, undermining the intended boundary that peer coordination state is daemon/hook/admin-owned and not raw MCP-owned.

**Minimal remediation:**

- Add `RequestPayload::PeerHeartbeat(_)` to the MCP-rejected match arm in `crates/memoryd/src/mcp.rs`.
- Add a regression test that constructs a `PeerHeartbeat`, calls `forward_payload_to_daemon` with a missing socket path, and asserts a local `method_not_allowed_on_mcp` error rather than socket I/O.
- Consider expanding the existing peer MCP test to cover all peer payload variants, not just `PeerStatus`.

## Positive validations

- **Mutating POSTs are CSRF-protected.** The only registered POST routes are `/api/reality-check/respond` and `/api/review/action`, and both are built inside `protected_post_routes` with `require_csrf` applied (`crates/memoryd-web/src/server.rs:135-139`). The CSRF token is generated from 32 random bytes and compared against `X-Memorum-CSRF` (`crates/memoryd-web/src/auth.rs:17-29`), with missing/wrong headers returning `403` (`crates/memoryd-web/src/auth.rs:32-41`).
- **Web bind is localhost-only and fail-closed.** `WebConfig::default` uses `127.0.0.1` (`crates/memoryd-web/src/config.rs:8-20`), validation rejects any other bind address before binding (`crates/memoryd-web/src/config.rs:34-40`), and `run` validates again before `TcpListener::bind` (`crates/memoryd-web/src/server.rs:163-166`). This is stricter than the spec's `127.0.0.1 or ::1` wording, but security-fail-closed rather than exposure.
- **Dashboard static assets are embedded/self-hosted.** The router serves embedded `RustEmbed` assets only (`crates/memoryd-web/src/server.rs:31-33`, `crates/memoryd-web/src/server.rs:187-223`), and the checked static shell has no CDN/external network load (`crates/memoryd-web/static/index.html:8-18`, `crates/memoryd-web/static/app.js:1-13`).
- **Non-audit routes do not expose the audit body fixture.** The only explicit full body fixture is `SafeContent::Plaintext("Task 14 audit-only fixture body")` in the audit artifact fixture (`crates/memoryd-web/src/routes/mod.rs:113-122`), and the full artifact is returned only by audit routes (`crates/memoryd-web/src/routes/audit.rs:67-112`). The API contract test covers non-audit routes against that canary (`crates/memoryd-web/tests/api_contract.rs:196-213`).
- **SSE content is bounded to a heartbeat snapshot and generic notification text.** The default notification is a threshold message with no memory body or memory id (`crates/memoryd-web/src/routes/mod.rs:77-80`), and the SSE handler emits one `heartbeat` event with JSON plus `text/event-stream`/`no-cache` headers (`crates/memoryd-web/src/routes/status.rs:112-121`).
- **Concurrent web review mutations are guarded.** `ReviewActionTracker` uses a mutex-protected active set (`crates/memoryd-web/src/server.rs:116-128`); `review_action` rejects non-reviewable or already-claimed ids before recording and returns the stable 409 body (`crates/memoryd-web/src/routes/review.rs:69-98`). The focused test exercises two concurrent POSTs for the same memory id (`crates/memoryd-web/tests/concurrent_access.rs:10-29`).
- **Daemon Reality Check mutations are serialized.** `RealityCheckRequest::List` stays read-only, while `Run`/`Respond`/`Skip`/`Snooze`/`Reset` acquire `state.reality_check_lock` before state/file mutation (`crates/memoryd/src/handlers.rs:501-568`). The regression test covers a `forget` vs `not_relevant` race and expects exactly one accepted response plus one stale-session refusal (`crates/memoryd/tests/responses.rs:249-285`).
- **Slash command output does not contain raw bodies and redacts encrypted/unsafe titles.** The formatter accepts only `RealityCheckItem` data, short-circuits encrypted items, runs titles through `safe_plaintext_fragment`, normalizes whitespace, and escapes quotes before display (`crates/memoryd/src/slash_commands.rs:8-29`, `crates/memoryd/src/slash_commands.rs:40-57`, `crates/memoryd/src/slash_commands.rs:79-81`). Tests cover formatted titles, encrypted-title omission, empty lists, and a secret-like body/title canary (`crates/memoryd/tests/slash_commands.rs:5-57`).
- **`memoryd ui` subprocess launch is not shell-injection-prone.** The CLI resolves a `memoryd-tui` path and starts it with `std::process::Command::new(binary).args(...)`, not a shell (`crates/memoryd/src/main.rs:484-501`). The subprocess arguments are `OsString` values for `--panel` and `--socket`, and panel values are clap-bounded to `1..=8` (`crates/memoryd/src/cli.rs:67-74`, `crates/memoryd/src/cli.rs:677-683`).
- **MCP manifest remains frozen to the nine agent-facing tools.** `ToolName` contains only the nine public tools (`crates/memoryd/src/mcp.rs:29-40`, `crates/memoryd/src/mcp.rs:245-258`), and the manifest tests verify admin web/reality-check names are absent (`crates/memoryd/tests/mcp_manifest.rs:7-68`). The code also rejects `TrustArtifact`, web dashboard, Reality Check, and peer status/activity/release-lock payloads before socket I/O (`crates/memoryd/src/mcp.rs:223-242`). The finding above is limited to the missing `PeerHeartbeat` arm.

## Commands run

```bash
cargo test -p memoryd-web --test api_contract --test csrf --test concurrent_access
```

Result: passed (`13 + 1 + 8` tests).

```bash
cargo test -p memoryd --test cli_contract --test slash_commands --test trust_artifact --test protocol_contract --test mcp_manifest --test server_smoke
```

Result: passed (`16 + 4 + 8 + 15 + 12 + 4` tests).

Static review commands used: targeted `rg` scans and line-numbered `nl -ba` reads over the scoped files listed above.

## Residual risk and confidence

Residual risk is mainly integration drift: the `memoryd-web` routes are still fixture-backed rather than daemon-backed, so this review validates the current placeholders and boundary checks, not a production data adapter. Browser hardening headers beyond CSRF, such as CSP and `X-Content-Type-Options`, were not blockers because the current asset shell does not render user-controlled HTML and the service is localhost-only.

Confidence: high for the reviewed code paths and the single MCP rejection gap; medium for future daemon-backed dashboard behavior because the current web crate still uses deterministic fixture data.
