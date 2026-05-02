# Stream G Review Gate C — Clean-code / Correctness Rerun

### Verdict

Changes requested

### Intended outcome

This rerun verifies the remediation for the prior Gate C clean-code/correctness findings across the notification dispatcher, Stream E/TUI notification surfaces, and trust artifact data assembly. The intended product outcome is that Stream G's observability surfaces expose passive notifications reliably, render trust artifacts from normative daemon-side data sources without fabricated audit fields, and preserve the UI/daemon/substrate boundaries.

### Executive summary

The three prior findings are remediated: passive notifications are now owned by `HandlerState` and exposed in `StatusResponse`, trust artifact policy decisions come from `events_log` governance decision rows and are empty when absent, and missing privacy scans now run the deterministic classifier for plaintext while encrypted missing-scan artifacts do not fabricate `none/plaintext`. The requested tests, clippy, and fmt gates all pass. However, I found a remaining Gate C integration issue: the TUI memory-detail modal is not wired to the daemon-assembled `memoryd::trust_artifact::TrustArtifact` DTO at all. In the live app it only polls daemon status, keeps `snapshot.trust_artifact` as `None`, and the only populated artifact path is sample fixture data, so users opening a memory detail get "No trust artifact loaded" instead of the server-side trust artifact required by Task 12/spec §7.

### Findings

[Medium] [API Contract] TUI trust-artifact modal is not wired to the daemon-assembled TrustArtifact DTO

- Evidence: `docs/plans/2026-05-01-stream-g-observability.md:948` requires trust artifact data to be fetched server-side, with the widget receiving a pre-assembled `TrustArtifact` DTO, and `docs/specs/stream-g-observability-v0.1.md:1312-1329` requires every memory detail view to show the full trust artifact from daemon/substrate/event-log data sources. The daemon has a real builder (`crates/memoryd/src/trust_artifact.rs:167-190`), but the protocol has no request/response variant for fetching an artifact (`crates/memoryd/src/protocol.rs:45-80`, `crates/memoryd/src/protocol.rs:303-318`). The TUI client only sends `RequestPayload::Status` (`crates/memoryd-tui/src/client.rs:20-33`), and `App::poll_daemon` only copies status counters into the snapshot (`crates/memoryd-tui/src/app.rs:300-307`). The modal renders `snapshot.trust_artifact` or falls back to `No trust artifact loaded.` (`crates/memoryd-tui/src/app.rs:635-638`); normal snapshots initialize that field to `None` (`crates/memoryd-tui/src/app.rs:730-742`), while the only populated path uses `TrustArtifact::sample()` (`crates/memoryd-tui/src/app.rs:746-758`). The TUI also defines its own separate, non-serialized DTO in `crates/memoryd-tui/src/widgets/trust_artifact.rs:88-108`, rather than consuming the daemon DTO.
- Why it matters: Gate C is supposed to validate the trust artifact widget as an observability surface, not only the in-memory renderer. As implemented, a user opening a memory detail in the actual TUI cannot see the daemon-assembled provenance, policy, privacy, recall, supersession, or sync state. This misses the "no black boxes" business goal and leaves the tests mostly proving sample rendering rather than the live daemon/TUI contract.
- Reasoning: The remediated daemon-side builder now uses the correct data sources, but no live TUI path can request or receive its output. Because the actual TUI polling loop only asks for daemon status, the `TrustArtifactBuilder` output is unreachable from the modal. The separate TUI DTO also creates schema drift risk: future fields or redaction semantics can change in `memoryd::trust_artifact` without compiler pressure on the TUI renderer.
- Recommendation: Add a daemon protocol request/response for trust artifact lookup, e.g. `RequestPayload::TrustArtifact { id }` / `ResponsePayload::TrustArtifact(memoryd::trust_artifact::TrustArtifact)`, handled by `TrustArtifactBuilder` inside `memoryd`. Make the TUI fetch the selected memory id before opening/rendering the memory-detail modal, and either reuse the daemon DTO directly in the TUI widget or add a narrow `From<memoryd::trust_artifact::TrustArtifact>` adapter covered by a contract test. Add a TUI/client integration test proving a selected memory id results in a daemon request and the returned artifact renders, rather than relying on `TrustArtifact::sample()`.
- Confidence: High

### Non-blocking simplifications

- Collapse the duplicate TUI trust-artifact DTO into the daemon DTO or a single adapter layer. This would reduce drift and make redaction/data-source changes compile-visible across the server and UI boundary.

### Test gaps

- Missing TUI/client integration coverage for opening a selected memory detail and fetching the daemon-assembled trust artifact over the socket/protocol.
- Existing TUI trust artifact rendering tests exercise sample/local DTO data only; they would still pass if the live daemon fetch path is absent.
- No protocol contract test currently protects a trust-artifact request/response shape.

### Questions / uncertainties

- It is possible the team intended live trust-artifact fetch wiring to land in a later task, but Task 12's invariant explicitly says daemon-side assembly feeds the widget, and Gate C's review focus includes trust artifact data-source boundaries. On that contract, I treated the missing live protocol/client path as in-scope.
- I did not review the web dashboard or CLI because Gate C marks those out of scope.

### Positives

- Prior passive notification finding is fixed: `HandlerState` now owns a shared `PassiveQueue`, `spawn_notification_dispatcher` passes that shared queue into production dispatch, and `StatusResponse` exposes queued passive notifications (`crates/memoryd/src/handlers.rs:86-127`, `crates/memoryd/src/server.rs:93-96`, `crates/memoryd/src/handlers.rs:415-427`).
- Prior trust artifact data-source findings are fixed: policy decisions are queried from `events_log` rows of kind `governance_decision` and missing rows produce an empty list (`crates/memoryd/src/trust_artifact.rs:344-365`), while missing plaintext privacy scans call `DeterministicPrivacyClassifier::classify` and missing encrypted scans render a non-fabricated unavailable/encrypted state (`crates/memoryd/src/trust_artifact.rs:386-416`).
- Notification payload tests and TUI socket-unreachable tests are green, including the added socket failure behavior that prevents cached/sample content from being shown as live.

### Commands

- `cargo test -p memoryd --test dispatcher --test trust_artifact --test handler_contract --test protocol_contract` — passed: dispatcher 12, handler_contract 14, protocol_contract 12, trust_artifact 8.
- `cargo test -p memoryd-tui --test panel_render --test keymap --test socket_unreachable --test resize` — passed: keymap 10, panel_render 10, resize 2, socket_unreachable 3.
- `cargo clippy -p memoryd -p memoryd-tui --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memoryd -p memoryd-tui -- --check` — passed.
