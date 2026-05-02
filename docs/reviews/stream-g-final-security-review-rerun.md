# Stream G Final Security Review Rerun

Date: 2026-05-02  
Reviewer: Codex security auditor  
Verdict: **Approved**

## Scope

This rerun inspected the current Stream G code and docs after remediation touched web/TUI daemon dispatch, the web daemon router, audit route shape, and production scoring/query paths. The review focused on:

- auth, bind, and CSRF behavior;
- local-only assumptions and socket/backend fail-closed behavior;
- secret and PII leakage in TUI, web, notifications, docs, and bench artifacts;
- command execution and path traversal;
- audit route exposure;
- regressions in recent web/TUI/scoring fixes.

Mandatory skills loaded and applied before review: `clean-code`, `tdd`, `rust-engineer`.

## Verdict

**Approved.** I found no blocking security or privacy regressions in the current Stream G implementation. No production or test code changes are requested.

## Findings by Severity

### High

None.

### Medium

None.

### Low / Residual

These are not release blockers under Stream G's local-only threat model:

1. **Local dashboard trust boundary is intentionally same-host and unauthenticated.** The web server rejects non-loopback bind addresses and protects mutating browser requests with CSRF, but any same-user local process can still call the daemon socket or localhost dashboard while it is running. This matches the current local operator model. Future hardening would require an explicit local auth token or per-launch bearer credential.
2. **Web child readiness still has a localhost time-of-check/time-of-use shape.** The daemon preflights and then starts `memoryd-web`, and readiness waits for a listener while checking child liveness. That is acceptable for this local CLI workflow, but inherited sockets or a child-owned readiness handshake would be stronger.
3. **Child binary resolution falls back to `PATH`.** Web/TUI launches use `Command::new` with arguments and no shell, and prefer sibling binaries, but the fallback still trusts the daemon process environment. This is acceptable for same-user local tooling; future hardening could make the binary path explicit or refuse PATH fallback in production profiles.
4. **Audit routes intentionally expose plaintext for plaintext memories.** Encrypted memories are redacted by the trust-artifact path, but plaintext memory audit output is visible to callers who can reach the localhost dashboard or daemon socket. This is expected functionality; keep the local-only bind and socket permissions as required invariants.

## Security Evidence

### Auth, Bind, and CSRF

- `crates/memoryd-web/src/auth.rs:10-29` creates a 32-byte random CSRF token and hex-encodes it.
- `crates/memoryd-web/src/auth.rs:32-41` rejects protected requests unless `x-memorum-csrf` exactly matches the server token.
- `crates/memoryd-web/src/server.rs:173-178` applies CSRF middleware to the mutating dashboard routes `/api/reality-check/respond` and `/api/review/action`.
- `crates/memoryd-web/src/server.rs:244-247` embeds the token into the index response for same-origin dashboard JavaScript.
- `crates/memoryd-web/static/app.js:1-5` reads the embedded token and uses it for API calls.
- `crates/memoryd-web/src/config.rs:8-20` defaults to `127.0.0.1`; `crates/memoryd-web/src/config.rs:34-47` rejects non-loopback bind addresses.
- `crates/memoryd-web/src/server.rs:216-223` validates config before binding.
- `crates/memoryd-web/src/bin/memoryd-web.rs:8-14` and `crates/memoryd-web/src/bin/memoryd-web.rs:44-46` force child dashboard binds to localhost and reject privileged ports.
- `crates/memoryd/src/handlers.rs:477-483` rejects daemon web-enable requests for ports below 1024.
- Coverage inspected: `crates/memoryd-web/tests/csrf.rs:11-61`, `crates/memoryd-web/tests/csrf.rs:110-128`.

Assessment: mutating web routes are not exposed without CSRF, and the dashboard is constrained to loopback. I did not find a regression in the recent web router changes.

### Socket and Backend Fail-Closed Behavior

- `crates/memoryd-web/src/server.rs:201-209` builds a backend-unavailable router instead of serving fake live data when daemon setup fails.
- `crates/memoryd-web/src/routes/status.rs:109-125` returns `BAD_GATEWAY` when daemon-backed status cannot reach the socket.
- `crates/memoryd-web/tests/api_contract.rs:27-52` verifies default routes do not serve fixture data and daemon routes fail closed if the socket is missing.
- `crates/memoryd/src/server.rs:142-169` binds the Unix socket and attempts `0600` permissions on Unix.
- `crates/memoryd/src/server.rs:193-243`, `crates/memoryd/src/server.rs:245-258`, and `crates/memoryd/src/server.rs:269-304` enforce frame size, malformed/oversize failure paths, and idle timeouts.

Assessment: the web dashboard does not silently fall back to stale/sample data on backend failure, and daemon socket framing is bounded.

### MCP and Daemon Authorization Boundary

- `crates/memoryd/src/mcp.rs:223-242` rejects admin/interactive payloads including `TrustArtifact`, `WebEnable`, `WebDisable`, `WebStatus`, `RealityCheck(_)`, and peer payloads before socket I/O.
- `crates/memoryd/src/protocol.rs:782-805` maps that denial to a stable non-retryable method-not-allowed error.
- `crates/memoryd/tests/mcp_manifest.rs:28-68` verifies admin tools are not exposed in the MCP manifest.
- `crates/memoryd/tests/mcp_manifest.rs:70-87` verifies blocked web payloads are rejected locally.

Assessment: MCP remains a restricted boundary and does not inherit local daemon admin powers.

### Audit Route and Trust Artifact Exposure

- `crates/memoryd-web/src/routes/audit.rs:14-31` and `crates/memoryd-web/src/routes/audit.rs:33-56` flatten trust-artifact output through safe display text fields.
- `crates/memoryd-web/src/routes/audit.rs:116-138` sends `/api/audit/{id}` through daemon `TrustArtifact` in daemon mode.
- `crates/memoryd-web/src/routes/audit.rs:175-212` applies the same daemon path for temporal audit output.
- `crates/memoryd/src/trust_artifact.rs:12-29` defines the encrypted redaction display contract.
- `crates/memoryd/src/trust_artifact.rs:132-160` maps encrypted title/body fields to `SafeContent::Encrypted`; plaintext output is only emitted for plaintext memories.
- `crates/memoryd/src/trust_artifact.rs:214-229` reads recall stats from `events_log`.
- `crates/memoryd/src/trust_artifact.rs:246-284` derives provenance from `events_log`.
- `crates/memoryd/src/trust_artifact.rs:353-373` returns governance-policy evidence from event payloads rather than hidden secrets.
- `crates/memoryd/src/trust_artifact.rs:395-424` treats encrypted privacy scan content as unavailable/encrypted instead of plaintext.

Assessment: the audit shape change did not introduce encrypted-content leakage. Plaintext audit exposure remains intentional local-only functionality.

### TUI Daemon Dispatch and Stale Data

- `crates/memoryd-tui/src/client.rs:53-99` dispatches review actions through typed daemon calls and fails unsupported actions client-side.
- `crates/memoryd-tui/src/client.rs:101-127` and `crates/memoryd-tui/src/client.rs:153-164` map Reality Check actions to typed protocol requests, reject malformed memory IDs, and avoid shell execution.
- `crates/memoryd-tui/src/app.rs:300-340` keeps failed review actions queued/retryable and surfaces socket state instead of pretending success.
- `crates/memoryd-tui/src/app.rs:511-523` requires an active Reality Check session and selected memory before dispatch.
- `crates/memoryd-tui/src/app.rs:543-553` requests trust artifacts only for resolved valid memory IDs.
- `crates/memoryd-tui/src/app.rs:855-862` renders socket-unreachable state without cached memory panel content.
- `crates/memoryd-tui/tests/socket_unreachable.rs:43-53` verifies the unreachable snapshot does not show sample memory content.
- `crates/memoryd-tui/tests/keymap.rs:147-187`, `crates/memoryd-tui/tests/keymap.rs:189-247`, and `crates/memoryd-tui/tests/keymap.rs:249-274` cover daemon dispatch, selected IDs, and unsupported actions.

Assessment: recent TUI dispatch changes preserve the daemon boundary and fail visibly rather than leaking stale/sample content.

### Reality Check State, Mutations, and Forget Reasons

- `crates/memoryd/src/handlers.rs:662-677` keeps Reality Check list read-only while serializing mutating operations.
- `crates/memoryd/src/handlers.rs:732-782` reloads active session/item state before applying confirm/correct/forget/not-relevant/skip behavior.
- `crates/memoryd/src/handlers.rs:1003-1022` sanitizes forget reasons, redacts unsafe/secret/PII markers, and truncates to 160 characters.
- `crates/memoryd/src/state.rs:9-15`, `crates/memoryd/src/state.rs:48-52`, `crates/memoryd/src/state.rs:114-122`, `crates/memoryd/src/state.rs:209-220`, and `crates/memoryd/src/state.rs:254-270` keep Stream G state files under the configured runtime state directory and use fixed filenames plus atomic writes.

Assessment: state paths are not caller-controlled, and recent Reality Check changes do not introduce path traversal or unbounded secret-bearing logs.

### Notifications, Secrets, and PII

- `crates/memoryd/src/protocol.rs:332-340` defines notification events with IDs, counts, paths, timestamps, and scopes, not memory body/title content.
- `crates/memoryd/src/notifications/config.rs:28-65` stores SMTP password configuration as an environment-variable name, not the password value.
- `crates/memoryd/src/notifications/external.rs:45-58` redacts password material from `EmailMessage` debug output.
- `crates/memoryd/src/notifications/external.rs:156-159` logs sanitized external-delivery failures.
- `crates/memoryd/src/notifications/external.rs:177-185` reads SMTP password values from the environment at send time and disables sending if missing.
- `crates/memoryd/src/notifications/external.rs:345-392` builds Slack/email summaries without memory content.
- `crates/memoryd/src/notifications/dispatcher.rs:50-72` emits passive/OS notifications without memory content.
- `crates/memoryd/src/notifications/os.rs:72-90` uses direct command arguments, not shell interpolation, for OS notification delivery.
- Coverage inspected: `crates/memoryd/tests/dispatcher.rs:125-169`, `crates/memoryd/tests/dispatcher.rs:195-253`.

Assessment: I did not find secret-bearing notification logs, memory body/title leakage in notification messages, or SMTP password exposure.

### Command Execution and Path Traversal

- `crates/memoryd/src/handlers.rs:250-258` spawns `memoryd-web` with `Command::new` and fixed arguments, not through a shell.
- `crates/memoryd/src/handlers.rs:3217-3229` prefers a sibling `memoryd-web` binary before falling back to PATH.
- `crates/memoryd/src/main.rs:500-510` launches the TUI binary with `Command::new` and arguments.
- `crates/memoryd/src/main.rs:361-363` invokes `git diff` with explicit arguments.
- `crates/memoryd/src/recall/project.rs:165-181` invokes git with arguments, `current_dir`, and a timeout.
- `crates/memoryd/src/trust_artifact.rs:435-456` validates repo paths, rejects colon-prefixed pathspecs, and calls `git status` with `--literal-pathspecs` and `--`.
- `crates/memoryd-web/src/server.rs:259-280` serves embedded static assets through `RustEmbed`, not direct filesystem paths.

Assessment: I did not find shell-injection or filesystem path traversal in the reviewed Stream G surfaces.

### Scoring and Query Paths

- `crates/memoryd/src/reality_check/scoring.rs:137-174` uses SQLite placeholders for recall-count queries and only dynamically builds placeholder counts.
- `crates/memoryd/src/reality_check/scoring.rs:176-216` queries static scoring fields with placeholders.
- `crates/memoryd/src/reality_check/scoring.rs:218-254` uses a recursive CTE with placeholders and a bounded depth condition.
- `crates/memoryd/src/reality_check/scoring.rs:29-39`, `crates/memoryd/src/reality_check/scoring.rs:101-103`, `crates/memoryd/src/reality_check/scoring.rs:115-122`, and `crates/memoryd/src/reality_check/scoring.rs:265-270` keep candidate selection/scoring bounded.
- `crates/memoryd/src/reality_check/session.rs:172-190` and `crates/memoryd/src/reality_check/session.rs:229-243` build Reality Check list data from index-safe fields and avoid encrypted body/title content.

Assessment: the production scoring/query fix path remains parameterized and bounded; I did not find SQL injection or encrypted-content exposure.

### Docs and Bench Artifacts

- `bench/stream-g-observability-results.darwin-arm64.json:1-175` contains timings, counts, and fixture labels only.
- Repository-wide secret/credential string search, excluding `target`, lockfiles, and common dependency directories, found only code/test canaries and documentation examples; no live API keys, webhook URLs, private keys, or bearer tokens were identified.
- Reviewed docs did not expose production credentials or private memory content.

Assessment: Stream G docs and bench artifacts do not appear to leak real secrets or PII.

## Validation Performed

The prompt stated integrated validation had passed. I also reran targeted security-relevant checks:

```text
cargo test -p memoryd-web --test csrf --test api_contract --test concurrent_access
```

Result: passed (`csrf` 8/8, `api_contract` 15/15, `concurrent_access` 1/1).

```text
cargo test -p memoryd-tui --test keymap --test socket_unreachable --test panel_render
```

Result: passed (`keymap` 15/15, `socket_unreachable` 3/3, `panel_render` 11/11).

```text
cargo test -p memoryd --test dispatcher --test mcp_manifest --test daemon_state_files --test scoring
```

Result: passed (`dispatcher` 12/12, `mcp_manifest` 13/13, `daemon_state_files` 15/15, `scoring` 21/21).

## Files Inspected

Docs and artifacts:

- `docs/reviews/stream-g-final-security-review.md`
- `docs/specs/stream-g-observability-v0.1.md`
- `docs/plans/2026-05-01-stream-g-observability.md`
- `docs/api/stream-g-observability-api.md`
- `docs/dev/stream-g-architecture.md`
- `docs/runbooks/reality-check.md`
- `bench/stream-g-observability-results.darwin-arm64.json`

Web:

- `crates/memoryd-web/src/auth.rs`
- `crates/memoryd-web/src/config.rs`
- `crates/memoryd-web/src/server.rs`
- `crates/memoryd-web/src/routes/mod.rs`
- `crates/memoryd-web/src/routes/audit.rs`
- `crates/memoryd-web/src/routes/reality_check.rs`
- `crates/memoryd-web/src/routes/review.rs`
- `crates/memoryd-web/src/routes/status.rs`
- `crates/memoryd-web/src/routes/entity_graph.rs`
- `crates/memoryd-web/src/routes/roi.rs`
- `crates/memoryd-web/src/bin/memoryd-web.rs`
- `crates/memoryd-web/static/app.js`
- `crates/memoryd-web/tests/csrf.rs`
- `crates/memoryd-web/tests/api_contract.rs`
- `crates/memoryd-web/tests/concurrent_access.rs`

TUI:

- `crates/memoryd-tui/src/client.rs`
- `crates/memoryd-tui/src/app.rs`
- `crates/memoryd-tui/src/panels/reality_check.rs`
- `crates/memoryd-tui/src/panels/review_queue.rs`
- `crates/memoryd-tui/src/widgets/trust_artifact.rs`
- `crates/memoryd-tui/src/main.rs`
- `crates/memoryd-tui/src/config.rs`
- `crates/memoryd-tui/tests/keymap.rs`
- `crates/memoryd-tui/tests/socket_unreachable.rs`
- `crates/memoryd-tui/tests/panel_render.rs`

Daemon, protocol, MCP, notifications, scoring, and substrate:

- `crates/memoryd/src/protocol.rs`
- `crates/memoryd/src/mcp.rs`
- `crates/memoryd/src/server.rs`
- `crates/memoryd/src/client.rs`
- `crates/memoryd/src/cli.rs`
- `crates/memoryd/src/main.rs`
- `crates/memoryd/src/handlers.rs`
- `crates/memoryd/src/state.rs`
- `crates/memoryd/src/trust_artifact.rs`
- `crates/memoryd/src/reality_check/scoring.rs`
- `crates/memoryd/src/reality_check/session.rs`
- `crates/memoryd/src/reality_check/types.rs`
- `crates/memoryd/src/notifications/config.rs`
- `crates/memoryd/src/notifications/external.rs`
- `crates/memoryd/src/notifications/dispatcher.rs`
- `crates/memoryd/src/notifications/passive.rs`
- `crates/memoryd/src/notifications/os.rs`
- `crates/memoryd/tests/mcp_manifest.rs`
- `crates/memoryd/tests/daemon_state_files.rs`
- `crates/memoryd/tests/dispatcher.rs`
- `crates/memoryd/tests/scoring.rs`
- `crates/memory-substrate/src/api.rs`
- `crates/memory-substrate/src/index/schema.rs`
- `crates/memory-substrate/src/index/query.rs`
- `crates/memory-substrate/src/index/migrations.rs`

## Residual Risk and Confidence

Residual risk is low and mostly inherent to the chosen local-operator architecture: localhost web access, same-user daemon socket access, and plaintext memory visibility for plaintext audit routes. Those boundaries are documented by the current code and tests, and the reviewed fixes did not widen them.

Confidence: high for the inspected Stream G surfaces and targeted security concerns. Confidence is lower for unrelated areas outside Stream G and for platform/runtime behavior not exercised by the targeted tests.
