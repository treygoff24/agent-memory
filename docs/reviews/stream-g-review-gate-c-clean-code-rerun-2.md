Verdict: Changes requested

### Intended outcome

This review verifies the latest Gate C remediation for the previous blocker: the memoryd-tui memory-detail/trust-artifact modal should consume the daemon-owned `memoryd::trust_artifact::TrustArtifact` protocol DTO instead of a duplicate local DTO or sample-only production path. The intended product outcome is that users can open a memory detail in the TUI and see daemon-assembled provenance, policy, privacy, recall, supersession, and sync-state data from the substrate/event-log sources.

### Executive summary

The prior blocker is substantially closed: the daemon protocol now has a `trust_artifact` request/response, the handler builds the artifact via `TrustArtifactBuilder`, the TUI client can fetch it over the daemon socket, and the widget renders the daemon DTO directly via `memoryd::trust_artifact::TrustArtifact`. The remaining issue is a modal state bug: opening the memory-detail modal from a panel that cannot resolve a valid memory id leaves the previously loaded artifact in `snapshot.trust_artifact`, so the UI can show a stale daemon artifact for the wrong context. That is not a DTO duplication regression, but it is a correctness risk in the same trust-artifact surface and should be fixed before this Gate C remediation is accepted.

### Findings

[Medium] [Correctness] Memory-detail modal can show a stale trust artifact when no selected id is resolved

- Evidence: `crates/memoryd-tui/src/app.rs:470-478` resets the modal and sets `pending_trust_artifact_id = self.selected_memory_id()`, but only clears `self.snapshot.trust_artifact` when `pending_trust_artifact_id.is_some()`. `selected_memory_id` explicitly returns `None` for `PanelId::Namespace` at `crates/memoryd-tui/src/app.rs:514-524`, while the namespace panel still opens `Modal::MemoryDetail` on Enter/`t` (`crates/memoryd-tui/src/panels/namespace.rs:37`). The modal renderer then blindly renders `self.snapshot.trust_artifact.as_ref()` at `crates/memoryd-tui/src/app.rs:528-536`, so any artifact loaded from a previous memory remains visible. The same failure mode can occur for any panel whose text scan does not contain a valid full `MemoryId`.
- Why it matters: The trust-artifact modal is an audit surface. Showing a previously loaded artifact under a new selection/context misleads users about provenance, policy, privacy, and supersession state for the memory they think they opened. This is especially risky because the data is high-trust daemon data; the bug is not obviously distinguishable from correct content in the UI.
- Reasoning: The remediation correctly changed the production fetch path from sample/local data to daemon DTO data, but the cached DTO is stored at the snapshot level and not invalidated on every memory-detail open. If a user loads artifact A from the review queue, switches to the namespace panel, and opens memory detail, no daemon request is queued because `selected_memory_id()` is `None`; because the old artifact is not cleared, the modal still renders artifact A instead of the empty/loading state.
- Recommendation: Clear `self.snapshot.trust_artifact` unconditionally whenever `Modal::MemoryDetail` is opened, before or immediately after computing `pending_trust_artifact_id`. If no valid id is available, render the existing empty state. Also add a keymap/render regression test for: load or seed an existing `snapshot.trust_artifact`, open memory detail from a panel with no resolvable id (for example Namespace), and assert the stale artifact is not rendered. If Namespace is supposed to support memory detail, give `NamespacePanelData` a real selected memory id instead of relying on text scanning.
- Confidence: High

### Prior blocker verification

- Daemon protocol request/response exists: `RequestPayload::TrustArtifact { id }` is defined at `crates/memoryd/src/protocol.rs:45-61`, and `ResponsePayload::TrustArtifact(Box<crate::trust_artifact::TrustArtifact>)` is defined at `crates/memoryd/src/protocol.rs:198-206`.
- Handler returns daemon-built artifact: dispatch routes `RequestPayload::TrustArtifact` to `trust_artifact_response` at `crates/memoryd/src/handlers.rs:223-236`; `trust_artifact_response` validates the id, calls `crate::trust_artifact::TrustArtifactBuilder::new(substrate).build(&memory_id)`, and returns `ResponsePayload::TrustArtifact` at `crates/memoryd/src/handlers.rs:280-287`.
- TUI client can fetch it: `DaemonClient::trust_artifact` sends `RequestPayload::TrustArtifact { id }` and unwraps a `ResponsePayload::TrustArtifact` at `crates/memoryd-tui/src/client.rs:36-54`.
- Widget renders daemon DTO directly: `crates/memoryd-tui/src/widgets/trust_artifact.rs:3-5` imports/re-exports the daemon `TrustArtifact`, and `TrustArtifactWidget` renders fields from `&TrustArtifact` at `crates/memoryd-tui/src/widgets/trust_artifact.rs:30-119`.
- Sample-only rendering is no longer the normal production fetch path: production `App::new` starts from `DaemonSnapshot::loading`, which has `trust_artifact: None` through `DaemonSnapshot::empty` (`crates/memoryd-tui/src/app.rs:187-206`, `crates/memoryd-tui/src/app.rs:857-878`), and `poll_daemon` loads a pending trust artifact from the daemon after status succeeds (`crates/memoryd-tui/src/app.rs:307-332`). The remaining sample artifact is confined to `DaemonSnapshot::sample()` / test-fixture construction (`crates/memoryd-tui/src/app.rs:881-894`, `crates/memoryd-tui/src/app.rs:735-815`).
- MCP remains correctly excluded: trust-artifact lookup is treated as an admin/UI daemon payload and rejected by MCP forwarding at `crates/memoryd/src/mcp.rs:177-239`.

### Non-blocking simplifications

- `App::open_modal` would be easier to reason about if memory-detail opening were split into a small helper such as `open_memory_detail_modal()`, with the invariants local and obvious: reset scroll, clear current artifact, compute selected id, queue fetch if present, then open modal.

### Test gaps

- Missing regression coverage for stale trust-artifact invalidation when opening `Modal::MemoryDetail` without a valid selected memory id.
- Existing TUI tests prove a review-queue selection queues the selected daemon artifact id (`crates/memoryd-tui/tests/keymap.rs:101-111`) and sample DTO fields render (`crates/memoryd-tui/tests/panel_render.rs:122-138`), but they do not cover the cache invalidation failure mode above.
- I did not rerun the parent-provided cargo gates in this read-only review; I treated the listed local evidence as already run by the parent.

### Questions / uncertainties

- It is unclear whether Namespace is intended to open a memory detail for a selected memory today. The panel advertises the action, but `selected_memory_id` currently returns `None` for Namespace. If that action is intentionally disabled until later, the handler should not open a potentially stale memory-detail modal from that panel.

### Positives

- The daemon/TUI boundary is much cleaner now: the TUI widget consumes the daemon DTO directly instead of maintaining a duplicate local schema.
- The protocol and handler contract tests now protect the new trust-artifact request/response and daemon-assembled handler path (`crates/memoryd/tests/protocol_contract.rs:20-28`, `crates/memoryd/tests/protocol_contract.rs:109-122`, `crates/memoryd/tests/handler_contract.rs:538-570`).
- The production fetch path avoids showing sample content while the daemon is unreachable: `DaemonSnapshot::loading` starts empty, and failed artifact loads requeue the id and mark the socket unreachable rather than substituting fixture data (`crates/memoryd-tui/src/app.rs:307-332`, `crates/memoryd-tui/src/app.rs:857-878`).
