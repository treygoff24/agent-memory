# Agent Memory Gap Inventory

Date: 2026-05-19

Repository: `/Users/treygoff/Development/agent-memory`

Provenance: source code, tests, scripts, manifests, and workflows only. Gemini Flash subagents did not return usable analysis during the prior pass; they failed on Google quota and embedded-session lock errors. This file is grounded in local inspection, not in those failed reports.

Scope note: this inventory records source-visible gaps I could substantiate. It does not claim every planned product requirement is missing, because I intentionally did not rely on repo plans, specs, reviews, handoffs, README files, or docs for factual claims.

## Executive Summary

The core substrate, daemon, governance, privacy, recall, dreaming, web, TUI, coordination, and eval surfaces are present, but several parts still have fixture or placeholder behavior at the boundary where the system should become operationally trustworthy:

- Source capture is currently static HTTP text capture, not a general source-grounding system.
- Sensitive extracted source text cannot be stored encrypted; it is refused instead.
- The web dashboard has a real daemon path, but many daemon-backed fields are zeros, empty vectors, or deferred responses.
- Notifications are partly process-local and passive; dashboard streaming does not yet surface daemon notifications.
- Eval and release gates still rely heavily on mock/default behavior unless real harness credentials and explicit modes are present.
- Bench and eval bootstrap paths still permit placeholder baselines or deferred tests.

## Gaps

### 1. Source Capture Is Static HTTP/Text Only

Evidence:

- `crates/memory-source/src/capture.rs:61-69` performs a direct `reqwest` GET with fixed headers.
- `crates/memory-source/src/capture.rs:154` records `CaptureMethod::HttpStaticV1`.
- `crates/memory-source/src/extract.rs:33-49` supports only `text/plain`, `text/html`, and `application/xhtml+xml`.

Impact:

The system cannot yet ground rendered JavaScript pages, logged-in pages, browser-visible state, screenshots, PDFs, office documents, or other rich artifacts. This limits the evidence layer for exactly the kinds of mutable external facts where source grounding matters most.

Implementation work:

- Add capture adapters for browser-rendered pages, screenshots, PDF/text extraction, manual authenticated imports, and local file artifacts.
- Extend the capture manifest with adapter type, render metadata, browser context metadata, and artifact-type-specific hashes.
- Add dispatch by MIME type and request mode instead of treating capture as one HTTP-static path.

Verification gate:

- Integration tests with a local HTML+JS server, a PDF fixture, an unsupported MIME fixture, redirect cases, and a privacy-sensitive page.
- Assert that every adapter emits stable source refs and governance can consume those refs.

### 2. Encrypted Source Artifacts Are Unsupported

Evidence:

- `crates/memory-source/src/capture.rs:212-220` classifies extracted text and returns `encrypted_source_artifacts_unsupported` when the privacy classifier requests `EncryptAtRest`.
- `crates/memory-source/src/capture.rs:125-133` stores raw bytes only when the raw textual projection is safe plaintext; otherwise raw storage is omitted, not encrypted.

Impact:

Private but valid sources cannot be captured as durable evidence. The system either refuses the capture or omits raw source material, which blocks grounded private memory and makes later audit/replay weaker.

Implementation work:

- Store encrypted extracted text and encrypted raw artifacts through `memory-privacy`.
- Keep safe descriptors and source refs visible while sealing plaintext.
- Add reveal/audit paths that can prove an encrypted source exists without leaking its contents.

Verification gate:

- Source capture test where classifier routes extracted text to `EncryptAtRest`.
- Assert capture succeeds, safe descriptors are exposed, plaintext is absent from manifest/index, and explicit reveal can recover the content.

### 3. Raw Source Storage Has No Encrypted Fallback

Evidence:

- `crates/memory-source/src/capture.rs:123-133` treats unsafe raw textual projection as `RawStorage::OmittedPrivacy`.
- `crates/memory-source/src/capture.rs:143-145` records only a warning, `raw_omitted_privacy`.

Impact:

For sensitive source pages, the audit trail can preserve hashes and excerpts but not the original raw payload. That weakens future dispute resolution, recapture comparison, and forensic inspection.

Implementation work:

- Add `RawStorage::Encrypted` or equivalent manifest state.
- Compress, encrypt, and hash raw bytes before storage.
- Add storage integrity checks for encrypted raw artifacts.

Verification gate:

- Capture a sensitive HTML fixture; assert raw bytes are encrypted and retrievable only through explicit reveal.
- Confirm raw plaintext does not appear in artifact files or indexes.

### 4. Dashboard Daemon Status Is Mostly Synthetic

Evidence:

- `crates/memoryd-web/src/routes/status.rs:147-172` maps daemon status to dashboard status with `uptime_seconds: 0`, `active_memories: 0`, zero sync counts, zero review counts, zero conflicts, empty active sessions, zero dream run counts, and `peer_update_total: 0`.
- `crates/memoryd/src/handlers/mod.rs:1127-1139` returns `dreams: Default::default()` in `StatusResponse`.

Impact:

The dashboard can appear connected to a daemon while hiding the actual state of the index, sync, conflicts, review pressure, dreaming, and peer activity.

Implementation work:

- Expand daemon `StatusResponse` to include live index stats, last reindex, git sync state, review queue counts, conflict counts, active peer sessions, dream status summary, and peer-update counters.
- Replace web-side zero/default mapping with direct daemon values.

Verification gate:

- Seed substrate with memories, conflicts, review items, peer sessions, and dream artifacts.
- Assert `/api/status` returns nonzero/live fields that match daemon/substrate state.

### 5. Web Dashboard Fixture Mode Is Still a Large Product Surface

Evidence:

- `crates/memoryd-web/src/routes/mod.rs:64-93` builds a full `DashboardData::default()` fixture.
- `crates/memoryd-web/src/server.rs:75-88` exposes `WebState::fixture()`.
- Fixture data feeds status, entity graph, ROI, reality check, audit, notifications, and recall hits.

Impact:

Tests and local UI can pass against polished fixture data even when daemon-backed behavior is incomplete. This increases the risk of mistaking a good demo surface for an operational dashboard.

Implementation work:

- Keep fixture mode only for explicit tests/demo flags.
- Add daemon-backed contract tests for every dashboard route.
- Make fixture routes visually or programmatically distinguishable from live daemon routes.

Verification gate:

- Test that production/router default without daemon returns backend unavailable rather than fixture data.
- Test that daemon mode exercises daemon requests for every live route.

### 6. ROI Endpoint Is Deferred In Daemon Mode

Evidence:

- `crates/memoryd-web/src/routes/roi.rs:65-72` returns `deferred_response("roi")` whenever dashboard data is absent but a daemon socket exists.
- `crates/memoryd-web/src/routes/mod.rs:113-120` defines deferred responses as HTTP 501 with a Stream G future-section note.

Impact:

Operators cannot use the dashboard to measure promotion rate, promotion precision, refusal breakdown, dreaming ROI, or Reality Check adherence from real daemon data.

Implementation work:

- Add daemon protocol DTOs for ROI aggregates.
- Compute promotion/refusal/dream/reality-check metrics from event log and substrate state.
- Wire `/api/roi` to daemon mode.

Verification gate:

- Seed event log with promotions, refusals, dream candidates, review outcomes, and Reality Check sessions.
- Assert `/api/roi?window=30`, `90`, and `365` match expected aggregates.

### 7. Dashboard Notifications Stream Does Not Surface Daemon Notifications

Evidence:

- `crates/memoryd-web/src/routes/status.rs:128-135` returns fixture notifications when fixture data exists, but returns `Vec::new()` when a daemon socket exists.
- `crates/memoryd/src/protocol.rs:20-24` says notification broadcast is process-internal and not persisted by the protocol layer.

Impact:

Daemon events such as leaked secret attempts, blocking merge conflicts, review thresholds, dream completions, or Reality Check due/overdue can be emitted but not reliably visible in the web dashboard.

Implementation work:

- Add a daemon request or stream endpoint for passive notification entries.
- Consider persisting important notifications or exposing a bounded recent notification queue.
- Wire web SSE to daemon notification state rather than returning an empty heartbeat.

Verification gate:

- Emit blocking-conflict and Reality Check notifications in a seeded daemon.
- Assert `/api/notifications/stream` returns those events in daemon mode.

### 8. Reality Check History Is Empty In Daemon Mode

Evidence:

- `crates/memoryd-web/src/routes/reality_check.rs:124-150` calls daemon `RealityCheck(List)` but maps any successful response to `RealityCheckHistoryResponse { sessions: Vec::new() }`.

Impact:

The dashboard cannot show real Reality Check session history, adherence trends, skipped weeks, or prior correction/forget decisions.

Implementation work:

- Add a daemon history response distinct from the pending-list response, or extend the existing Reality Check protocol with historical sessions.
- Store and expose Reality Check session outcomes in runtime state or event log.

Verification gate:

- Run multiple Reality Check sessions with confirm/correct/forget/skip actions.
- Assert `/api/reality-check/history` returns sessions and action summaries in daemon mode.

### 9. Entity Detail Is Approximate In Daemon Mode

Evidence:

- `crates/memoryd-web/src/routes/entity_graph.rs:183-215` builds daemon graph edges only from overlap in recent memory ids.
- `crates/memoryd-web/src/routes/entity_graph.rs:218-245` fills daemon entity detail with `namespace: "daemon"`, `status: "unknown"`, `confidence: 0.0`, no first/last seen, no supersession chain, and no recall history.

Impact:

Entity pages can show a graph, but they do not yet provide trustworthy lifecycle, confidence, supersession, or recall context for real entities.

Implementation work:

- Expand daemon `InspectEntities` to include entity lifecycle metadata, related memory summaries, supersession links, and recall history.
- Replace synthetic detail fields with daemon data.

Verification gate:

- Seed entities with supersession chains, multiple memories, and recall hits.
- Assert entity detail includes real status, confidence, dates, supersession chain, and recall history.

### 10. Policy Editor Cannot Write Through The Daemon

Evidence:

- `crates/memoryd-web/src/routes/policy_editor.rs:75-84` can read daemon policy snapshots via `GovernancePolicyDump`.
- `crates/memoryd-web/src/routes/policy_editor.rs:87-121` writes only when `state.policy_dir()` is configured, or accepts fixture-mode validation without persistence.
- `crates/memoryd-web/src/routes/policy_editor.rs:123-130` marks daemon snapshots `writable: false`.

Impact:

The dashboard can inspect policy state, but cannot safely propose or persist daemon-backed policy edits unless it is pointed at a disk policy directory outside the daemon protocol.

Implementation work:

- Add daemon protocol methods for policy validation, staging, atomic write, and reload.
- Return structured validation errors and a dry-run diff before applying.
- Keep CSRF protection and add write audit events.

Verification gate:

- Post valid and invalid policy YAML against a daemon-backed web server.
- Assert valid writes are atomic, invalid writes do not mutate policy files, and daemon reload state changes predictably.

### 11. Standalone Daemon Mode Is Health-Only

Evidence:

- `crates/memoryd/src/server.rs:43-50` describes standalone daemon as status-only.
- `crates/memoryd/src/server.rs:342-355` returns healthy status for `Status` and `not_implemented` for every other request.

Impact:

If launched without substrate, `memoryd` can look alive but cannot serve memory operations. This is acceptable as a low-level socket health mode, but dangerous if operators interpret it as a functional daemon.

Implementation work:

- Make status guidance and web/backend health clearly distinguish standalone from substrate-backed readiness.
- Consider refusing MCP startup in standalone mode unless explicitly requested.

Verification gate:

- Start standalone daemon and assert non-status MCP calls produce a clear operator-facing failure.
- Start substrate daemon and assert full operation set is available.

### 12. Privacy Filter Provider Is Disabled By Default

Evidence:

- `crates/memory-privacy/src/privacy_filter.rs:13-24` defines `DisabledPrivacyFilter` as the default provider and returns `PrivacyFilterUnavailable`.
- The available non-disabled provider in this file is a deterministic test fixture at `crates/memory-privacy/src/privacy_filter.rs:27-48`.

Impact:

Layer 1 deterministic scanning still runs, but model-assisted privacy detection is not active by default. Subtle private spans that require semantic classification may be missed unless they match deterministic rules.

Implementation work:

- Add a production privacy-filter provider behind explicit configuration.
- Record provider name, version, decision spans, and failure mode in audit metadata.
- Define fail-open/fail-closed policy by namespace.

Verification gate:

- Contract tests for disabled, fixture, unavailable, and live provider paths.
- Privacy e2e where a semantic private span is detected only by the provider and handled according to policy.

### 13. Notification Delivery Is Mostly Best-Effort

Evidence:

- `crates/memoryd/src/notifications/dispatcher.rs:37-45` logs lagged broadcast receivers but does not replay missed notifications.
- `crates/memoryd/src/notifications/external.rs:141-146` silently returns if no external channel is configured or retry max is zero.
- `crates/memoryd/src/notifications/external.rs:177-183` logs a missing SMTP password env var and returns `Ok(())`, meaning the dispatcher does not treat it as a delivery failure.

Impact:

Important operational notifications can be lost or silently suppressed depending on config and runtime timing. Passive queue entries help, but external delivery reliability is not yet strong enough for high-stakes alerts.

Implementation work:

- Persist critical notification events before dispatch.
- Expose delivery status and suppressed/misconfigured channel reasons.
- Treat missing required credentials as a reportable delivery failure for configured channels.

Verification gate:

- Tests for lagged broadcast, missing SMTP env var, disabled external channel, retry exhaustion, and passive queue surfacing.
- Dashboard/API test showing delivery status for recent critical notifications.

### 14. Dreaming Status Is Not Included In General Daemon Status

Evidence:

- `crates/memoryd/src/handlers/mod.rs:1127-1139` puts `dreams: Default::default()` in general `StatusResponse`.
- Dream status exists separately through `dream_status_response` at `crates/memoryd/src/handlers/mod.rs:1142-1146`.
- Web status maps dreaming fields to synthetic defaults in `crates/memoryd-web/src/routes/status.rs:162-166`.

Impact:

Operators looking at ordinary daemon or dashboard status may not see real dream scheduler health, last run, next run, lease state, or disabled sentinel state.

Implementation work:

- Include a compact dream status summary in general daemon status.
- Keep the detailed DreamStatus route for full diagnostics.
- Wire web status dreaming fields to the compact summary.

Verification gate:

- Enable/disable dreaming, run a dream pass, and assert ordinary status plus detailed status agree.

### 15. Eval Harness Has Deferred And Mock-Only Semantic Coverage

Evidence:

- `crates/memorum-eval/src/orchestrator.rs:226-387` declares 20 tests; tests 17 and 18 are marked `deferred: true`.
- `crates/memorum-eval/src/harness_runner.rs:298-323` makes mock tests 13 and 15 semantic skips, not passes.
- `crates/memorum-eval/src/harness_runner.rs:303-310` skips test 19 when the `stream-i-deps` feature is disabled.
- `crates/memorum-eval/tests/honesty.rs:7-19` asserts mock semantic tests skip rather than pass.

Impact:

The eval harness is honest about partial coverage, but the project still lacks always-on automated coverage for lease contention, encrypted-tier key rotation, and some real-harness behaviors.

Implementation work:

- Implement tests 17 and 18.
- Make test 19 run in default CI or add a separate required feature-enabled job.
- Define which real-harness tests block release candidates.

Verification gate:

- `cargo run -p memorum-eval -- --harness mock --output json` reports no deferred tests for required release set.
- Feature-enabled CI runs peer-update framing.
- Real harness CI fails on missing required semantic coverage.

### 16. Eval CI Defaults To Mock Harness

Evidence:

- `.github/workflows/stream-h-eval.yml:10-18` defaults `workflow_dispatch` harness mode to `mock`.
- `.github/workflows/stream-h-eval.yml:34-39` always runs mock.
- `.github/workflows/stream-h-eval.yml:43-56` runs real harnesses only on tags or non-mock manual dispatch.
- `.github/workflows/stream-h-eval.yml:93-104` makes partial RC runs fail only for matching RC tags.

Impact:

Normal pushes can pass without real Claude/Codex harness coverage. This is acceptable for fast feedback but not enough as the only quality signal before operational use.

Implementation work:

- Add a required scheduled or protected-branch real-harness lane when credentials are available.
- Make missing credentials produce a visible neutral/check-warning state rather than quietly relying on mock results.
- Require full eval for release branches/tags.

Verification gate:

- GitHub Actions matrix proves mock and real harness paths separately.
- RC tag without credentials fails, and protected release path requires full eval artifacts.

### 17. Bench Regression Gate Still Has Placeholder Baseline Bootstrap

Evidence:

- `scripts/bench-regression-check.sh:16` asks for a placeholder baseline if the baseline file is missing.
- `scripts/bench-regression-check.sh:24-28` treats `base.runs == 0` as bootstrap, writes a `.proposed` baseline, warns, and exits 0.

Impact:

The first-release bootstrap path can allow a performance gate to pass without an established baseline. That is useful during initial setup but should not remain a release-quality gate.

Implementation work:

- Replace placeholder baselines with committed measured baselines per supported profile.
- Make placeholder-baseline mode opt-in and disallowed in release CI.
- Archive proposed baselines as artifacts for review.

Verification gate:

- CI fails on placeholder baselines for release/profile gates.
- Bench job passes only against committed nonzero-run baselines.

### 18. Install Script Starts Daemon But Scheduler Remains Optional

Evidence:

- `scripts/install-memorum.sh:240-270` starts `memoryd` with `nohup`, waits for readiness, and writes a PID file.
- `scripts/install-memorum.sh:295-312` prints separate commands for daemon auto-restart and scheduled dream job.
- `scripts/install-memorum.sh:321-323` warns that dreams stay inactive when no supported harness CLI is detected.
- `scripts/install-memorum.sh:325-327` installs launchd only when `--with-scheduler` is set.

Impact:

A successful install can leave lifecycle durability and scheduled dreaming unconfigured unless the operator follows the printed next steps or passes scheduler options.

Implementation work:

- Add explicit install summary fields: daemon running, launchd installed, scheduler installed, harness available, dreams active.
- Consider an interactive or flag-driven "full install" mode that installs daemon launchd and scheduler together.
- Add a doctor check for missing launchd/scheduler when dreaming is enabled.

Verification gate:

- Dry-run and live install tests assert summary output for all lifecycle states.
- `memoryd doctor` flags enabled dreaming without scheduler or harness.

### 19. Web/TUI Reality Check Progress Is Incomplete

Evidence:

- `crates/memoryd-web/src/routes/reality_check.rs:179-188` fixture-mode response always returns `remaining: 0, deferred: 0`.
- `crates/memoryd-tui/src/client.rs:97-118` maps non-pending daemon responses to default TUI state and sets reviewed/deferred fields through defaults.

Impact:

Reality Check surfaces can under-report progress or collapse unexpected daemon responses into empty/default UI state, making operator progress less trustworthy.

Implementation work:

- Return exact reviewed, deferred, remaining, skipped, forgotten, and corrected counts from the daemon.
- Treat unexpected response variants as visible UI errors rather than default empty state.

Verification gate:

- Run a mixed Reality Check session and assert web and TUI progress agree with daemon state.
- Inject unexpected daemon responses and assert UI shows an error state.

### 20. Source Grounding In Eval Uses Temporary File Fixtures

Evidence:

- `crates/memorum-eval/src/simulator.rs:248-268` turns non-file `agent_primary` source refs into `file:` refs under the system temp directory.

Impact:

The simulator can test grounding mechanics, but it does not exercise real web capture, artifact storage, redirects, MIME handling, or privacy-sensitive source capture.

Implementation work:

- Add eval scenarios that call `memory_capture_source` and then write memories using produced source refs.
- Include web capture and privacy refusal/encryption branches in the eval catalog.

Verification gate:

- Eval test for web source grounding uses a local HTTP fixture server and asserts memory write cites captured source refs.

## Verification Status

This file itself is a documentation artifact. I did not run the full Rust/frontend gate suite after writing it. The evidence above was verified by direct file inspection and targeted `rg`/`nl` reads on 2026-05-19.

Recommended next gate:

```bash
git diff -- GAPS.md
```

Then, before using this as an implementation tracker, run at least:

```bash
scripts/check-fast.sh
```
