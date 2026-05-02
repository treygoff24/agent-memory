Verdict: Changes requested

### Intended outcome

Stream I Gate D is intended to wire production daemon delta recall to Stream I coordination so Level 2 sessions receive relevant `<peer-update>` entries, Level 3 sessions also receive `<peer-presence>`, claim-lock metadata is visible on peer updates, delivery audit remains RAM-only, and Level 1/minimal projects preserve baseline Stream E behavior with no `coordination=` insertion.

### Executive summary

The original blocker is fixed: the daemon delta handler now constructs a `DeltaCoordinationContext` and calls `build_delta_response_with_coordination`, and the delta renderer receives `delta_coordination.insertion.as_ref()` instead of hard-coded `None`. The new integration test proves the happy-path XML insertion and RAM-only delivery audit path. However, the production peer-update candidate mapping loses the actual peer session identity and hard-codes the harness, and the per-session cool-down state is rebuilt from scratch on every delta request. Those are not polish issues: the spec makes `from`/`session` load-bearing framing, and it requires repeat suppression for the same receiving session. Gate D should not advance until those are fixed and covered by integration tests.

### Findings

[High] Correctness Peer-update attribution uses device id, not the peer session that wrote the memory

- Evidence: `crates/memoryd/src/recall/delta.rs:221-237` builds every `PeerWriteCandidate` with `harness: "codex"` and `session_id: peer_session_id(row)`, where `peer_session_id` returns `row.source_device` or `"local-device"`. The Stream I spec requires the `<peer-update from=... session=...>` attributes to identify the peer harness and peer `session_id` (`docs/specs/stream-i-cross-session-v0.1.md:347-350`), and the Gate D acceptance test requires the correct `from`, `session`, `ref`, and `namespace` (`docs/specs/stream-i-cross-session-v0.1.md:951`).
- Why it matters: Agents rely on these attributes to distinguish peer context from user input. Mislabeling a writer session as a device id weakens the highest-stakes framing contract, makes `memoryd peer activity --session ...` misleading, and prevents reliable filtering/auditing by actual session.
- Reasoning: The recall index row exposed to this path carries `source_device`, not source session or source harness. The fix maps that device field into `PeerUpdateEntry.session_id`, and `rendered_peer_deliveries` then records the same incorrect value as `from_session_id` via `crates/memoryd/src/recall/delta.rs:314-318`. A same-device write from `sess_peer_writer` will render/audit as something like `dev_deltalevel2`, not `sess_peer_writer`; non-Codex peer harnesses will also render as `from="codex"`.
- Recommendation: Preserve actual source harness/session in the data used for Stream I candidate assembly. Either expose `source_harness` and `source_session_id` on `RecallIndexRow` or hydrate the selected memories/frontmatter before creating `PeerWriteCandidate`. Then add an integration assertion that a memory authored by `harness="claude-code", session_id="sess_peer_writer"` renders with that harness/session and records the same source in peer activity.
- Confidence: High

[High] Correctness Per-session peer-update cool-down cannot work because session context is transient

- Evidence: `crates/memoryd/src/recall/delta.rs:94-109` creates a new `SessionContext` inside every `build_delta_coordination` call. `delta_session_context` initializes that context with `..SessionContext::default()` at `crates/memoryd/src/recall/delta.rs:169-175`, so `surfaced_peer_writes` starts empty for every request. The only cool-down state is in `SessionContext::surfaced_peer_writes` (`crates/memorum-coordination/src/session.rs:62-73`), and `RelevanceGate` only checks/records against that in-memory set for the current context (`crates/memorum-coordination/src/gate.rs:45-65`). The spec explicitly requires `test_level2_cool_down_suppresses_repeat` (`docs/specs/stream-i-cross-session-v0.1.md:953`).
- Why it matters: A receiving session can be shown the same peer update on every turn while the write remains inside the recency window. That creates noisy, misleading recall and undermines the product goal of surfacing new peer activity without repeatedly re-injecting stale context.
- Reasoning: `record_surfaced_peer_write` mutates only the local `session` variable created for this one delta assembly. Nothing stores that set back into `HandlerState` or another per-session registry, and `rg surfaced_peer_writes` shows no memoryd production use outside the transient delta/startup builders. The new `coordination_integration.rs` covers the first delivery but does not issue a second delta for the same receiving session.
- Recommendation: Add a RAM-only per-session coordination state in `HandlerState` keyed by receiving session id/harness (or extend the existing in-memory presence/session registry) and seed `SessionContext.surfaced_peer_writes` from it before evaluation; after rendering selected updates, persist the surfaced ids back to that RAM state. Add the missing integration test: session A writes memory M, session B receives M on turn 1, session B's next delta omits M while still within the recency window.
- Confidence: High

### Non-blocking simplifications

- After the correctness fixes, consider extracting candidate hydration/attribution into a small helper module. `delta.rs` now mixes request validation, index querying, scoring-context derivation, XML-delivery audit detection, privacy summary filtering, and presence assembly; splitting only the attribution/hydration seam would make the load-bearing source identity logic easier to test without broad refactoring.

### Test gaps

- `crates/memoryd/tests/coordination_integration.rs` now exists and covers Level 1 absence, Level 2 happy path, Level 3 presence ordering, claim-lock attribute rendering, and production audit population.
- Missing: integration coverage that `<peer-update from=... session=...>` reflects the actual author harness/session rather than hard-coded `codex` plus device id.
- Missing: `test_level2_cool_down_suppresses_repeat` from the spec.
- Missing: integration coverage for below-threshold omission and the two-entry cap / pending-attention count.
- Missing: integration coverage proving self/current-session writes are not presented as peer updates once actual session attribution is available.

### Questions / uncertainties

- I did not find a current `RecallIndexRow` field for source session id. If the intended implementation is to avoid adding another row field, the assembler likely needs to hydrate selected memories from the substrate before constructing `PeerWriteCandidate`.
- I did not rerun the full clippy/fmt gate because the orchestrator already reported those as passing. I ran the focused new integration test below.

### Positives

- The original `CoordinationInsertion` wiring blocker is fixed: `crates/memoryd/src/handlers.rs:1078-1085` passes live coordination context, and `crates/memoryd/src/recall/delta.rs:66-80` builds coordination, renders it, and records delivered peer updates.
- Delivery audit state remains RAM-only in `HandlerState`/`PeerDeliveryAudit` (`crates/memoryd/src/handlers.rs:97-120`, `crates/memoryd/src/handlers.rs:201-202`, `crates/memoryd/src/handlers.rs:319-332`), with no new canonical persistence path found in the reviewed code.
- The new integration fixture is behavior-oriented and exercises the daemon request path rather than only the pure renderer.

### Validation run

- `cargo test -p memoryd --test coordination_integration` — passed, 4 tests.
