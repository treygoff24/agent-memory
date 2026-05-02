Verdict: Changes requested

### Intended outcome

Stream I Gate D appears intended to finish the user-visible coordination path after Tasks 14-19: recall assembly should render `CoordinationInsertion` as `<peer-update>` / `<peer-presence>` XML with correct budget/privacy behavior, daemon recall handlers should enforce Level 1/2/3 boundaries, and `memoryd peer` admin commands should expose RAM-only presence/claim-lock/audit state without adding a new persisted coordination layer.

### Executive summary

The renderer and the peer CLI pieces are generally clean and the focused validation commands pass, but the main Stream I business outcome is not wired through the delta recall path. `build_delta_response` still always calls `render_delta_frame(..., None)`, and the daemon handler simply delegates to that builder, so Level 2 peer updates and Level 3 presence cannot appear in actual `memoryd recall delta-block` / daemon delta responses. The test suite currently exercises the renderer in isolation and CLI state helpers, but the required `coordination_integration.rs` file is absent, so this regression is not covered end-to-end. I did not find evidence of Stream I presence or claim-lock state being persisted to canonical memory frontmatter; the new registries are in `HandlerState`/`DashMap` RAM only, with the authorized `ClaimLockContention` event write remaining best-effort event-log output.

### Findings

[High] Correctness Delta recall never receives `CoordinationInsertion`

- Evidence: `crates/memoryd/src/recall/delta.rs:7-20` defines `build_delta_response` and renders every delta with `render_delta_frame(&items, budget_tokens, None)`. `crates/memoryd/src/handlers.rs:1057-1065` calls that builder directly from the daemon delta handler. There is no other production call site that constructs a delta `CoordinationInsertion`; `rg CoordinationInsertion` only shows renderer tests and startup wiring for production delta paths.
- Why it matters: This prevents the default Level 2 feature from shipping. Peer writes, candidates, notes, and observations may be scored by the coordination crate in isolation, but users will not see `<peer-update>` entries in normal per-turn recall. Level 3 `<peer-presence>` also cannot be delivered in delta blocks, which is the primary surface for presence.
- Reasoning: Task 14/17 require the daemon/recall assembler to compute and pass optional coordination context into the delta block builder. The renderer supports `Some(&CoordinationInsertion)`, but the only public delta builder always supplies `None`, and the handler has no stateful coordination assembly step before calling it. Passing tests do not prove the feature works because they call `render_delta_frame` directly with hand-built fixture insertions.
- Recommendation: Add a daemon-level delta coordination assembly path before rendering: resolve effective level from project binding/config, short-circuit Level 1, construct/update a `SessionContext` for the requesting session, gather eligible peer-write candidates from the shared index/event/fragments per the recency policy, attach active claim-lock metadata, include Level 3 presence entries from `PresenceRegistry`, then call a delta builder/render path with `Some(insertion)` only when entries/caps exist. Add an end-to-end daemon test that issues a delta request and asserts actual response XML includes/omits coordination by level.
- Confidence: High

[Medium] Tests Missing Gate D integration test coverage lets the core wiring gap pass

- Evidence: `crates/memoryd/tests/coordination_integration.rs` is absent. `crates/memoryd/tests/coordination_recall_render.rs:20-141` validates renderer behavior with fixture `CoordinationInsertion` values, and `crates/memoryd/tests/peer_cli.rs:27-99` validates peer status/activity/release-lock handlers against manually seeded RAM state, but neither verifies that daemon delta/startup flows compute and deliver coordination insertions from real substrate/session state.
- Why it matters: The focused gate passes while the primary product path is still disconnected. That creates a false release signal for Stream I: XML rendering is correct when manually invoked, but users do not get peer-update/presence behavior through the daemon.
- Reasoning: The plan explicitly listed `coordination_integration.rs` as the full daemon integration test file and Review Gate D includes integration coverage. Its absence is visible in the repo, and the current tests do not fail on `build_delta_response(... None)` because they do not exercise that path with peer-state setup.
- Recommendation: Add `crates/memoryd/tests/coordination_integration.rs` covering at least: Level 1 delta has no coordination attribute/entries; Level 2 delta surfaces a relevant peer update within the recency window and excludes stale/low-score/self writes; Level 3 delta includes presence before updates; claim locks on surfaced peer updates render `claim_locked`; coordination bytes count against the delta budget; no coordination state is written to canonical memory frontmatter.
- Confidence: High

[Medium] Correctness Peer delivery activity is not recorded by production delivery paths

- Evidence: `HandlerState::record_peer_delivery` exists at `crates/memoryd/src/handlers.rs:151-153`, and peer activity reads the in-memory audit at `crates/memoryd/src/handlers.rs:495-520`. The only inspected uses are test seeding in `crates/memoryd/tests/peer_cli.rs:66-67`; production recall/delta/startup code does not call `record_peer_delivery` when a `<peer-update>` is rendered.
- Why it matters: `memoryd peer activity` can appear implemented while showing no real delivery history in normal use. That undermines the admin/operator visibility promised by Task 19 and makes it harder to diagnose whether peer updates were delivered, capped, or skipped.
- Reasoning: Because coordination insertion is not wired for delta, there is currently no natural production point that records delivery audit entries. Even after wiring insertion, the renderer is pure string rendering and should not mutate audit state; the assembler/handler needs to explicitly record deliveries when entries are selected for a target session.
- Recommendation: When the daemon assembles a non-empty coordination insertion for a delta/startup recipient, record one bounded `PeerDeliveryAuditEntry` per rendered peer update in `HandlerState` with `from_*`, `to_*`, memory id, relevance, and privacy-safe summary. Keep this RAM-only; do not persist the audit to disk.
- Confidence: Medium

### Non-blocking simplifications

- Consider extracting the coordination rendering helpers in `crates/memoryd/src/recall/render.rs:306-409` into a small `coordination_render` submodule once the wiring lands. The current render file is still readable, but the peer-update/presence/pending-attention helpers are now a distinct responsibility from baseline Stream E memory entry rendering.
- `CoordinationInsertion::has_entries` in `crates/memorum-coordination/src/protocol.rs:49-56` could be used by render/startup code instead of repeated peer-update/presence emptiness checks. This is minor, but it would reduce duplicated shape knowledge.

### Test gaps

- No `coordination_integration.rs` is present, despite the plan/spec naming it as the full daemon integration file.
- No daemon-level delta test verifies Level 2 peer-update insertion from actual substrate/index candidates.
- No daemon-level delta test verifies Level 3 presence insertion before peer updates.
- No test verifies peer delivery audit entries are populated from real coordination delivery rather than manual test seeding.
- No end-to-end test verifies coordination state remains RAM-only while only authorized canonical events/memory surfaces are written.

### Questions / uncertainties

- I could not validate candidate/note/`memory_observe` Level 2 behavior because the production delta coordination assembly path is not present yet.
- `crates/memoryd/tests/coordination_integration.rs` is absent; if another branch or subagent has it unmerged, this review is against the current working tree only.
- I did not perform a full repository-wide persistence audit beyond targeted `rg` inspection of the Stream I/recall/handler paths.

### Positives

- The XML renderer is cautious about escaping attributes/text, privacy-filters peer summaries, caps presence display, and accounts rendered coordination bytes through the same `estimated_tokens` helper used by normal delta items.
- Claim-lock acquire/contention/release handling is scoped by effective coordination level, uses RAM-only registry state, and has focused tests for Level 1/2 behavior plus contention event emission.
- Peer admin commands are not exposed through MCP, and the parser/handler tests cover status, activity, and forced release-lock behavior.

### Validation run

- `cargo test -p memoryd --test coordination_recall_render --test peer_cli --test claim_lock_supersede` — passed (14 + 8 + 10 tests).
- `cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings` — passed.
