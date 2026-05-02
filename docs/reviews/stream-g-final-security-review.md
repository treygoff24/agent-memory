Verdict: Approved

# Stream G Final Review Gate E Security/Privacy Review

## Scope

Review-only final security/privacy gate for Stream G, focused on:

- CSRF enforcement.
- Localhost-only web binding.
- No memory content in Slack/email payloads.
- SMTP password never stored in config files.
- Reality Check/admin daemon variants blocked from MCP forwarding.
- Daemon state files kept in the local runtime tree, not git-synced memory content.
- Trust artifact data sources do not invent ghost `memories` columns.
- `memoryd web enable` child/readiness behavior.

The worktree was already heavily modified before this pass. I did not edit production code; this document is the only artifact written.

## Findings

No blocking security or privacy findings.

## Evidence by review focus

### 1. CSRF enforcement

Status: pass.

- The dashboard generates a 32-byte random CSRF token, hex-encodes it, and requires an exact `x-memorum-csrf` header match (`crates/memoryd-web/src/auth.rs:10-29`).
- The CSRF middleware returns `403 Forbidden` unless the token matches (`crates/memoryd-web/src/auth.rs:32-40`).
- The only POST routes are `/api/reality-check/respond` and `/api/review/action`; both live in a protected sub-router with `require_csrf` applied before the routes are merged into the app (`crates/memoryd-web/src/server.rs:173-178`).
- The initial HTML embeds the per-state token (`crates/memoryd-web/src/server.rs:244-247`), and the frontend reads it from the meta tag (`crates/memoryd-web/static/app.js:1-5`).
- Regression coverage rejects missing/wrong tokens, accepts the correct token, verifies token shape, and verifies token rotation across server states (`crates/memoryd-web/tests/csrf.rs:11-61`). Reality Check POST is also covered with a correct token (`crates/memoryd-web/tests/csrf.rs:110-128`).

### 2. Localhost-only binding

Status: pass.

- `WebConfig` defaults to `127.0.0.1` and rejects any other bind address with `WebConfigError::NonLocalBindAddress` (`crates/memoryd-web/src/config.rs:8-20`, `crates/memoryd-web/src/config.rs:34-47`).
- `run_with_state` validates the config immediately before `TcpListener::bind`, so programmatic callers also hit the localhost check (`crates/memoryd-web/src/server.rs:216-223`).
- The `memoryd-web` child binary constructs a localhost-only `WebConfig` directly (`crates/memoryd-web/src/bin/memoryd-web.rs:8-14`).
- Config coverage explicitly rejects `0.0.0.0` (`crates/memoryd-web/tests/csrf.rs:64-80`).

### 3. Slack/email payloads contain no memory content

Status: pass.

- The internal notification event enum can carry identifiers, paths, counts, timestamps, and scopes, but no memory body fields (`crates/memoryd/src/protocol.rs:333-340`).
- Slack payloads and email messages are built from `external_summary(event)` plus generic instructions, not from memory titles, bodies, entities, paths, or IDs (`crates/memoryd/src/notifications/external.rs:345-372`).
- `external_summary` intentionally ignores memory-bearing fields, including `memory_id`, conflict `path`, and `scope`; it emits only content-free summaries/counts/timestamps (`crates/memoryd/src/notifications/external.rs:375-392`).
- The Slack regression test fires a Reality Check due event and asserts the captured payload omits title/entity/body canaries (`crates/memoryd/tests/dispatcher.rs:147-169`).

### 4. SMTP password is not stored in config files

Status: pass.

- Email config stores `smtp_password_env`, the name of the environment variable, not a password value (`crates/memoryd/src/notifications/config.rs:57-64`).
- Email dispatch reads the actual secret via `std::env::var(&request.config.smtp_password_env)` at send time and disables email delivery when the env var is missing (`crates/memoryd/src/notifications/external.rs:177-185`).
- `EmailMessage` carries the runtime password only after env lookup, and its custom `Debug` formatter redacts the password (`crates/memoryd/src/notifications/external.rs:33-58`).
- Regression coverage verifies the sent password value comes from the env var, not the config field name; verifies debug redaction; and verifies missing env disables delivery (`crates/memoryd/tests/dispatcher.rs:195-253`).

### 5. Reality Check/admin variants are MCP-blocked

Status: pass for Stream G admin/UI variants.

- The MCP manifest exposes exactly the nine agent-facing tools and excludes web/reality-check admin tool names (`crates/memoryd/src/mcp.rs:246-272`, `crates/memoryd/tests/mcp_manifest.rs:6-25`, `crates/memoryd/tests/mcp_manifest.rs:28-61`).
- The raw MCP payload forwarder rejects Stream G admin/UI daemon payloads before socket I/O: `TrustArtifact`, `WebEnable`, `WebDisable`, `WebStatus`, and all `RealityCheck(_)` requests. It also rejects peer-state payloads that reuse the same boundary (`crates/memoryd/src/mcp.rs:223-242`).
- The stable rejection error is `method_not_allowed_on_mcp`, non-retryable (`crates/memoryd/src/protocol.rs:782-805`).
- Regression coverage proves web admin payloads are rejected locally with a deliberately missing socket path (`crates/memoryd/tests/mcp_manifest.rs:70-87`) and Reality Check payloads are rejected without contacting the daemon (`crates/memoryd/tests/notification_channel.rs:37-52`).

### 6. State files stay in the local runtime tree

Status: pass.

- The roots model distinguishes the synced git repository root from the local per-device runtime root (`crates/memory-substrate/src/model.rs:36-48`).
- Stream G state files are named `state.json`, `reality-check-pending.json`, and `reality-check-session.json`, all under a `state/` directory rooted at the runtime root (`crates/memoryd/src/state.rs:9-14`, `crates/memoryd/src/state.rs:291-296`).
- Daemon state, pending cache, and session state write only through `state_dir(runtime_root)` / `state_file(runtime_root, ...)`, not through `roots.repo` (`crates/memoryd/src/state.rs:48-52`, `crates/memoryd/src/state.rs:114-121`, `crates/memoryd/src/state.rs:209-220`).
- The CLI default runtime is `.memoryd`, documented as local per-device runtime (`crates/memoryd/src/cli.rs:157-167`), and `.memoryd/` is git-ignored in this repository (`.gitignore:17-20`).
- Regression coverage verifies pending/session files are saved under the runtime `state/` directory and not at the runtime root, and that session deletion removes the runtime state file (`crates/memoryd/tests/daemon_state_files.rs:143-157`, `crates/memoryd/tests/daemon_state_files.rs:230-238`).

Residual note: a caller can still explicitly choose an unsafe runtime path. The reviewed implementation keeps Stream G state under the runtime root and the default runtime is ignored; a future hardening pass could reject `--runtime` paths that canonicalize inside an unignored git working tree.

### 7. No ghost `memories` columns for trust artifact data

Status: pass.

- The `memories` schema has the authorized Stream G/Stream I fields, including nullable `original_confidence`, existing `source_device`, and `indexed_at`; it does not add recall-count, provenance, policy, privacy-scan, supersession-array, or sync-state columns for trust artifact rendering (`crates/memory-substrate/src/index/schema.rs:14-50`).
- Migration v4 adds only `original_confidence` to `memories`, plus the derived `events_log` mirror and `memory_supersession` projection (`crates/memory-substrate/src/index/migrations.rs:125-160`).
- Trust artifact recall stats are derived from `events_log`, not `memories` (`crates/memoryd/src/trust_artifact.rs:214-238`).
- Provenance, devices, and policy decisions are derived from `events_log`; supersession links are derived from `memory_supersession` (`crates/memoryd/src/trust_artifact.rs:246-288`, `crates/memoryd/src/trust_artifact.rs:306-317`, `crates/memoryd/src/trust_artifact.rs:353-373`).
- Regression coverage proves recall totals/last-recalled are events-log derived and supersession links come from the projection (`crates/memoryd/tests/trust_artifact.rs:117-158`).

### 8. `memoryd web enable` child/readiness behavior

Status: pass.

- `WebDashboardRuntime::enable` returns the existing running status only when its tracked child is still alive on the requested port; otherwise it stops any old child, preflights localhost port availability, resolves `memoryd-web`, spawns without a shell using argv arguments, waits for readiness, and stores the child handle only after readiness succeeds (`crates/memoryd/src/handlers.rs:236-268`).
- Preflight bind checks `127.0.0.1:{port}` before spawn and reports a `port_in_use` error if occupied (`crates/memoryd/src/handlers.rs:306-311`, `crates/memoryd/src/handlers.rs:3252-3258`).
- The readiness loop checks `child.try_wait()` before accepting TCP readiness, so an early child exit cannot be reported as a running dashboard (`crates/memoryd/src/handlers.rs:313-328`).
- Runtime status refresh clears the dashboard status if the tracked child has exited (`crates/memoryd/src/handlers.rs:288-296`).
- Regression coverage binds an occupied localhost port, calls enable, asserts `port_in_use`, and asserts runtime status is not running (`crates/memoryd/src/handlers.rs:3316-3328`).

Residual note: readiness is still "tracked child alive and some TCP listener accepted on localhost" rather than a child-owned readiness protocol. The TOCTOU window between preflight bind release and child bind is narrow and not a release blocker for a localhost admin surface, but a future hardening pass could use inherited listeners or an explicit readiness signal.

## Validations run

```bash
cargo test -p memoryd-web --test csrf --test api_contract
```

Result: passed (`8 + 15` tests).

```bash
cargo test -p memoryd --test dispatcher --test mcp_manifest --test daemon_state_files --test trust_artifact
```

Result: passed (`12 + 13 + 15 + 8` tests).

```bash
cargo test -p memory-substrate --test events_log_mirror --test migration_v4
```

Result: passed (`6 + 4` tests).

## Residual risk and confidence

Residual risk is low for the reviewed Stream G security/privacy surface. The most important release boundaries are enforced in code and covered by focused regression tests: mutating web routes require CSRF, web binding is localhost-only, external notifications are content-free, SMTP secrets come from env, Stream G admin/UI daemon payloads are rejected from MCP, state files are runtime-local by default, trust artifact data is derived from authorized sources, and web enable no longer reports a preoccupied port or early-exited child as healthy.

Confidence: high for the focused Gate E verdict. I did not run a full workspace gate or review unrelated Stream H/I code beyond shared surfaces touched by the requested Stream G checks.
