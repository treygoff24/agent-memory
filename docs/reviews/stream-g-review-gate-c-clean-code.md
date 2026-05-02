# Stream G Review Gate C — Clean-code / Correctness Review

### Verdict

Changes requested

### Intended outcome

Tasks 8-12 appear intended to finish the Stream G notification dispatcher, Stream E pending-attention hook, initial `memoryd-tui` panel/keymap/socket framework, and server-side trust artifact DTO/widget integration. The business outcome is an always-on observability surface that alerts users without leaking memory content, keeps UI reads behind the daemon boundary, and renders audit/trust details from the normative Stream G §7.1 data sources rather than invented or stale fields.

### Executive summary

The focused test gates are green, and the implementation gets several important boundaries right: notification payload summaries avoid titles/bodies/entity names, `memoryd-tui` depends on `memoryd` rather than `memory-substrate`, socket-unreachable rendering hides cached live data, and recall stats are read from `events_log`. However, I found one blocking correctness issue: the production passive notification queue is created as an unreachable local inside `spawn_notification_dispatcher`, so the “always-on” channel cannot be surfaced by `memoryd status` or drained by recall assembly as the spec requires. I also found trust-artifact data-source violations: policy decisions and privacy scans are assembled from frontmatter extras/defaults rather than the specified event-log/provenance and classifier sources, which can fabricate audit fields and hide missing data.

### Findings

[High] [Correctness] Production passive queue is private to the dispatcher and cannot be surfaced

- Evidence: `crates/memoryd/src/server.rs:93-96` constructs `NotificationDispatcher::production(PassiveQueue::new(), NotificationConfig::default())` with a fresh queue that is not stored anywhere. `HandlerState` stores only the broadcast sender, not the passive queue (`crates/memoryd/src/handlers.rs:80-90`). `StatusResponse` has no pending-notification field (`crates/memoryd/src/protocol.rs:290-297`), and `status_response` only returns state/guidance/recall/dream counters (`crates/memoryd/src/handlers.rs:260-266`). The Stream G spec says passive notifications surface in `memoryd status`, are added to next-session `<pending-attention>`, and are drained by status/recall assembly (`docs/specs/stream-g-observability-v0.1.md:1196-1199`, `docs/specs/stream-g-observability-v0.1.md:1253-1258`). Gate C explicitly asks that the passive queue be truly always-on (`docs/plans/2026-05-01-stream-g-observability.md:1011-1013`).
- Why it matters: The passive channel is the safety net for every notification, including OS/external-disabled cases and external delivery failures. In production today, events are appended to an in-memory queue that no daemon surface can read, so users can miss Reality Check due/overdue, secret-write attempts, review-queue threshold alerts, and external-delivery-failure notes unless another channel happens to be enabled and succeeds.
- Reasoning: The dispatcher-level unit tests pass because they inject and inspect the same `PassiveQueue` instance. Production startup creates a new queue and drops the only handle after spawning the task. Since that handle is not part of `HandlerState`, `memoryd status`, or recall rendering, the “always append” behavior is not operationally visible and does not satisfy passive always-on semantics.
- Recommendation: Make `PassiveQueue` owned by daemon state, e.g. add it to `HandlerState`, pass a clone into `NotificationDispatcher::production`, and expose a read/drain path through `StatusResponse` and the recall pending-attention assembly. Store timestamps with entries so the 7-day drain-time expiry required by §6.3 can be implemented and tested. Add an integration test that fires a production notification through `HandlerState`, then verifies `RequestPayload::Status` and/or startup recall pending-attention can observe it.
- Confidence: High

[Medium] [API Contract] Trust artifact policy decisions are not sourced from provenance events and can be fabricated

- Evidence: Stream G §7.1 requires policy decisions to come from provenance-chain events of kind `GovernanceDecision` carrying the governance fields (`docs/specs/stream-g-observability-v0.1.md:1323-1325`). The implementation instead sets `policy_decisions` from `frontmatter.extras` (`crates/memoryd/src/trust_artifact.rs:181-184`) and `parse_policy_decisions` falls back to constructing a `PolicyDecision` from `frontmatter.write_policy.policy_applied` with every other field set to `not recorded` (`crates/memoryd/src/trust_artifact.rs:338-353`).
- Why it matters: The trust artifact is meant to be the audit trail users rely on when deciding whether memory state is trustworthy. Emitting a policy decision from current frontmatter rather than the historical event that actually recorded the governance decision can show decisions that never occurred, miss decisions that did occur, or conflate current policy metadata with audit history.
- Reasoning: This violates the Gate C “no fabricated fields” review focus. The current tests only cover a fixture that includes `governance_decision` in frontmatter extras, so they do not catch a memory with governance events but no extras, nor a memory with neither governance events nor extras where the code still returns a synthetic policy row.
- Recommendation: Derive `policy_decisions` from the same event-log/provenance scan used for `provenance_chain`, filtering governance-decision events and mapping their payload fields. If no such event exists, render an explicit empty/unknown section rather than manufacturing a decision. Keep frontmatter extras only as a backward-compatible source if the spec is intentionally amended to allow it; otherwise remove that fallback.
- Confidence: High

[Medium] [Correctness] Privacy scan fallback fabricates “none/plaintext” instead of running the classifier

- Evidence: Stream G §7.1 says privacy scan results come from `frontmatter.privacy_scan` if present, or from real-time `DeterministicPrivacyClassifier::classify(body)` for pre-Stream-D memories (`docs/specs/stream-g-observability-v0.1.md:1325`). The implementation calls `parse_privacy_scan(&frontmatter.extras, encrypted)` (`crates/memoryd/src/trust_artifact.rs:181-184`), and when no `privacy_scan` extra exists it returns `labels_detected: ["none"]` with `storage_action` set to `plaintext` or `encrypted` (`crates/memoryd/src/trust_artifact.rs:375-380`).
- Why it matters: For older plaintext memories, the trust artifact can incorrectly report that no privacy labels were detected even when the body contains PII or other sensitive patterns. That weakens the audit surface and can mislead users reviewing whether a memory should have been encrypted or quarantined.
- Reasoning: This is a concrete data-source mismatch and another fabricated-field path. It is especially risky because the fallback produces a confident-looking result rather than an “unknown/not scanned” state. The encrypted path does redact title/body correctly, but plaintext pre-Stream-D memories are not reclassified as specified.
- Recommendation: When `frontmatter.privacy_scan` is missing and the memory body is plaintext, invoke the Stream D deterministic classifier and map its labels/storage action into `PrivacyScan`. For encrypted or metadata-only memories where the body cannot be classified, render a safe “not available without reveal”/index-only state rather than `none`. Add tests for a plaintext memory without `privacy_scan` but with classifier-detectable content, and for encrypted missing-scan behavior.
- Confidence: High

### Non-blocking simplifications

- The notification dispatcher/passive queue would be simpler if the queue were treated as a daemon service owned by `HandlerState` from the start, instead of injecting anonymous queues in tests and creating an inaccessible production queue. That change also closes the blocking passive-channel issue.

### Test gaps

- No production-path test proves a notification emitted through `HandlerState` is visible through `memoryd status` or recall pending-attention. The existing dispatcher tests only assert a manually injected `PassiveQueue` receives events.
- No trust-artifact test covers policy decisions sourced from event-log `GovernanceDecision` events, or the absence of such events. Current coverage allows frontmatter extras/fallbacks to satisfy the assertion.
- No trust-artifact test covers a plaintext memory without `privacy_scan` extras that should be classified in real time, nor an encrypted missing-scan case that must not fabricate `labels_detected = none`.

### Questions / uncertainties

- I did not run full workspace clippy because Gate C allowed focused clippy and the workspace is large; I ran package-level clippy for `memoryd` and `memoryd-tui` instead.
- I did not review web dashboard or CLI surfaces; they are explicitly out of scope for Gate C.

### Positives

- Focused requested tests are green for dispatcher, trust artifact, and TUI panel/keymap/socket/resize behavior.
- `recall_count_30d` and `last_recalled_at` are derived from `events_log` queries, not a `memories` column (`crates/memoryd/src/trust_artifact.rs:206-229`).
- `memoryd-tui` keeps the substrate boundary clean: its crate depends on `memoryd`, not `memory-substrate`, and `DaemonClient` polls through `memoryd::client` over the socket (`crates/memoryd-tui/Cargo.toml:8-17`, `crates/memoryd-tui/src/client.rs:20-34`).

### Commands

- `cargo test -p memoryd --test dispatcher --test trust_artifact` — passed: 10 dispatcher tests, 6 trust-artifact tests.
- `cargo test -p memoryd-tui --test panel_render --test keymap --test socket_unreachable --test resize` — passed: 10 keymap tests, 10 panel-render tests, 2 resize tests, 2 socket-unreachable tests.
- `cargo clippy -p memoryd -p memoryd-tui --all-targets --all-features -- -D warnings` — passed.
