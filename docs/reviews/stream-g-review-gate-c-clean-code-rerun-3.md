Verdict: Approved

### Intended outcome

This rerun reviews the narrow remediation for the Gate C rerun-2 Medium bug: opening `Modal::MemoryDetail` from a context that cannot resolve a valid selected memory id must not display a previously cached trust artifact. The intended product outcome is that the TUI memory-detail modal remains trustworthy as an audit surface: it either queues and renders the artifact for the selected memory, or shows an empty state when no valid selected memory id exists.

### Executive summary

No material issues found. The rerun-2 stale-cache finding is closed: `App::open_modal` now clears `snapshot.trust_artifact` unconditionally for every memory-detail open, regardless of whether `selected_memory_id()` returns `Some` or `None`. The added keymap and render regressions cover both state invalidation and the user-visible modal output. The prior DTO/protocol blocker also remains closed: the TUI still requests daemon-owned trust artifacts through the daemon protocol and renders the daemon `memoryd::trust_artifact::TrustArtifact` DTO directly rather than a duplicate local production DTO.

### Findings

None.

### Prior finding verification

- Rerun-2 Medium stale-cache finding is closed: `crates/memoryd-tui/src/app.rs:474-480` resets the memory-detail scroll state, computes the selected memory id, and clears `self.snapshot.trust_artifact = None` on every `Modal::MemoryDetail` open. This covers the no-id path because `selected_memory_id()` still returns `None` for `PanelId::Overview | PanelId::Namespace` at `crates/memoryd-tui/src/app.rs:516-527`, while the renderer only receives the current `snapshot.trust_artifact.as_ref()` at `crates/memoryd-tui/src/app.rs:530-538`.
- The empty-state behavior is explicit: `render_memory_detail_modal` renders `No trust artifact loaded.` when the artifact option is `None` at `crates/memoryd-tui/src/app.rs:677-680`.
- Regression coverage was added at the right behavioral level: `crates/memoryd-tui/tests/keymap.rs:114-125` proves opening memory detail from `PanelId::Namespace` clears a preloaded sample artifact and leaves no pending artifact id; `crates/memoryd-tui/tests/panel_render.rs:143-154` proves the rendered modal shows the empty state and does not leak the prior sample artifact id/source.
- Prior DTO/protocol blocker remains closed: `RequestPayload::TrustArtifact { id }` exists at `crates/memoryd/src/protocol.rs:45-61`, `ResponsePayload::TrustArtifact(Box<crate::trust_artifact::TrustArtifact>)` exists at `crates/memoryd/src/protocol.rs:198-205`, dispatch routes trust-artifact requests to the daemon handler at `crates/memoryd/src/handlers.rs:223-236`, and the handler builds the artifact through `TrustArtifactBuilder` at `crates/memoryd/src/handlers.rs:280-286`.
- The TUI production fetch/render path still uses the daemon DTO: `DaemonClient::trust_artifact` sends `RequestPayload::TrustArtifact` and unwraps `ResponsePayload::TrustArtifact` at `crates/memoryd-tui/src/client.rs:36-54`, while `crates/memoryd-tui/src/widgets/trust_artifact.rs:3-5` imports/re-exports `memoryd::trust_artifact::TrustArtifact` for the widget.
- MCP exclusion remains closed: `RequestPayload::TrustArtifact { .. }` is rejected by `forward_payload_to_daemon` before socket forwarding with `method_not_allowed_on_mcp` at `crates/memoryd/src/mcp.rs:223-238`.

### Non-blocking simplifications

- `App::open_modal` is still small enough, but if memory-detail behavior grows further, extracting an `open_memory_detail_modal()` helper would keep the invariant sequence obvious: reset scroll, compute selected id, clear cached artifact, open modal.

### Test gaps

None for this remediation. The new tests cover the rerun-2 failure mode at both state-transition and rendered-output levels. I did not rerun the parent-provided cargo gates in this read-only review; I treated the listed evidence as parent-run verification.

### Questions / uncertainties

- I did not validate the full untracked Stream G crate state beyond the requested files and the protocol/handler/MCP context needed to verify the prior blocker. This review is intentionally scoped to the latest stale-artifact remediation.

### Positives

- The fix is minimal and directly targets the modal state invariant instead of adding conditional rendering complexity elsewhere.
- The tests are behavior-focused and would fail on the exact stale cached artifact regression from rerun-2.
- The daemon-owned DTO path remains clean: protocol, client, and widget all continue to share `memoryd::trust_artifact::TrustArtifact`.
