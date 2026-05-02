# Stream I Fresh-Eyes Review (Claude, 2026-05-02)

Snapshot: 6095cf6
Method: spec/plan/code triangulation, security-aware, read-only

---

## Verdict

Stream I is well-implemented and security-conscious; the SessionContext-mutation fix is real and thoroughly tested, but `conflicting_claim_locks` is only populated at Level 3 (not Level 2), the bench fixture uses 16-dimensional synthetic embeddings rather than production-dimension vectors, and the `<memory-recall>` `<entity-recall>` attribute still emits `entities=""` regardless of peer-update presence — a minor spec gap.

---

## Blockers (must-fix before merge)

None identified that would prevent merge. The two items I initially considered for blocker status both resolve to risks on closer reading (see Risks section).

---

## Risks (worth surfacing, not blockers)

**R1 — `conflicting_claim_locks` silently empty at Level 2.**
`crates/memorum-coordination/src/presence.rs:276` — `handle_peer_heartbeat` returns `conflicting_claim_locks: Vec::new()` unconditionally. The population logic lives in `crates/memoryd/src/handlers.rs:590–591` and executes only when `ack.active_level == 3`. The spec (§6.1) defines `conflicting_claim_locks` as part of `PeerHeartbeatAck` but does not explicitly say it is Level-3-only; the only Level 3 restriction in §6.1 is that `active_peers` is `"Empty at Levels 1 and 2"`. Operators using the heartbeat at Level 2 (if they wire it) would receive no conflict signal. Low impact today since heartbeats are only sent at Level 3, but worth documenting as a named constraint rather than a silent gap.

**R2 — bench fixture uses 16-dimensional embeddings; production vectors will be 1,024–3,072 dimensions.**
`bench/stream-i-cross-session-results.darwin-arm64.json` line 18: `"precomputed_embedding_dimension": 16`. The p99 of 0.0077ms is credible for 16-dimensional dot products but does not bound the realistic workload. A 1,536-dimensional cosine similarity costs roughly 96× more compute (O(n) dot product). With 50 within-recency candidates and 1,536 dimensions the per-candidate budget of ≤5ms remains safe, but the bench provides no evidence for it. Recommend adding a fixture run at 1,536 dimensions with the same 301-sample count and updating the JSON with an `embedding_dimension_coverage` note.

**R3 — `<entity-recall entities="">` is always empty in the startup frame.**
`crates/memoryd/src/recall/render.rs:269`: `"<entity-recall entities=\"\">"`. The spec (§4.3) reads the `entities=` attribute of `<entity-recall>` to populate `salient_entities` at startup. The attribute is always emitted empty, so `session.rs:entity_recall_attribute_ids` always returns an empty vec from the recall block. The `startup_context_from_selection` function populates `salient_entities` via a different path (directly from `selected` candidates, `startup.rs:341`), so the gate itself still works correctly. The risk is that the spec-documented entity extraction from the `entities=` attribute (§4.3 "Stream E populates this attribute with the comma-separated entity ids it matched during assembly") is currently a no-op. If any downstream path relies on the XML attribute, it would silently produce no entities. This is a spec/implementation divergence, not a breakage.

**R4 — Simultaneous lock acquisition tiebreak: contender overwrites holder.**
`crates/memorum-coordination/src/claim_lock.rs:174–178`. When two sessions concurrently acquire a claim lock on the same memory*id and neither is the existing holder, `ClaimLockRegistry::acquire_at` performs a DashMap `Entry::Occupied` mutation that immediately \_replaces* the existing lock entry with the contender's lock before returning `Contended`. The contention is correctly reported, but the original holder's lock is atomically evicted. The spec (§7.1) describes claim locks as advisory and "warn but allow," which this satisfies, but operators inspecting `peer status` immediately after contention will see the contender as the new holder, not the original. The contention event is still emitted to the event log. Advisory semantics make this acceptable, but the sequence "lock evicts previous holder on contention" versus "lock reports contention without changing holder" should be explicitly documented in `claim_lock.rs`.

**R5 — `startup_context_from_selection` clones the `SessionContext` for each path (same vs. cross-device) then discards both.**
`crates/memoryd/src/recall/startup.rs:283,309`. Both `same_device_updates` and `cross_device_updates` call `startup_context.clone()` and pass the clone as `&mut session` to `RelevanceGate::evaluate`. The clone is purely local; mutations (recording surfaced peer writes via `record_surfaced_peer_write`) are discarded after each function returns. This means the cool-down registry between same-device and cross-device peer-update passes is not shared: if a memory clears the same-device threshold, it could in principle also appear in the cross-device pass during the same startup. In practice this is unlikely (same memory id showing up in both device categories requires deliberate replay), but the semantics diverge from the per-session cool-down described in §4.2.

---

## Nits

- `crates/memorum-coordination/src/claim_lock.rs:232–267` — `restore` is an undocumented public method not in the spec's public surface. A `/// # Panics` / safety doc would help reviewers understand its role.
- `crates/memorum-coordination/src/session.rs:104–109` — `is_tier1` is string-matching on harness name (`"codex" | "codex-cli" | "claude-code"`). Future harnesses added to the Tier 1 roster require updating this list and re-shipping. A registry pattern or config-driven tier list would be more maintainable, but this is an evolution concern, not a correctness bug.
- `crates/memoryd/src/recall/startup.rs:43–44` — `build_startup_response` hard-codes `DEFAULT_COORDINATION_LEVEL: 2` rather than using the daemon-provided `CoordinationConfig`. This is only called from a test path; production code goes through `build_startup_response_with_coordination_config`. Comment would prevent confusion.
- `crates/memoryd/src/recall/render.rs:412–419` — `push_if_within_budget` silently drops content that overflows the budget with no observability. Peer-update entries that were passed in `CoordinationInsertion.peer_updates` but fail the budget check are dropped without incrementing any counter. This means `capped_peer_updates` can undercount if the rendering budget is extremely tight. Not a practical issue at default budgets (≤320 tokens for max 2 peer-updates vs. 400-token delta budget).

---

## Verification of the SessionContext-mutation closeout fix

**The fix is real.**

The reported risk — that project rows could make themselves relevant by mutating the receiving SessionContext — is prevented by the data-flow shape in `startup.rs`.

At `startup.rs:331–351`, `startup_context_from_selection` constructs a fresh `SessionContext` from the _current session's own_ startup recall selection (`selected.selected` filtered by `is_peer_write_row` — that is, from memories that are _not_ peer writes). This `SessionContext` is then passed by value (not by reference) to the relevance gate path. At `startup.rs:283` and `startup.rs:309`, `startup_context.clone()` is called before passing to `RelevanceGate::evaluate`. Mutations inside `evaluate` (specifically `record_surfaced_peer_write` at `gate.rs:78`) operate on the clone, not on the original `startup_context` and not on any shared state.

Therefore:

1. **`SessionContext` is not mutated by incoming peer rows**: peer write candidates are read-only input to `score_with_embedding`; they do not write to `session`. Only `record_surfaced_peer_write` writes to the session, and that writes the _selected_ memory id into the local clone's cool-down set — not into any structure owned by the peer row.
2. **Empty relevance-gate output produces no insertion**: `non_empty_insertion` at `startup.rs:327–329` checks `insertion.peer_updates.is_empty()` and returns `None`, which the caller treats as `StartupCoordinationRender { same_device: None, cross_device: None }` — the render path is not entered.
3. **Regression test**: `coordination_recall_render.rs:test_coordination_attribute_on_delta` asserts that `render_delta_frame(&[], 400, Some(&CoordinationInsertion::empty()))` emits `<memory-delta empty="true" />` with no `coordination=` attribute, directly covering property (2). Property (1) is covered indirectly by `coordination_integration.rs:level2_daemon_delta_omits_below_threshold_peer_update` — zero entity overlap produces no `<peer-update>` — but there is no explicit test asserting "calling evaluate with N peer rows leaves startup_context.salient_entities unchanged." Given the data-flow evidence is structural (value semantics + clone), this is an acceptable gap; a direct mutation-guard test would be belt-and-suspenders.

File:line anchors:

- `startup.rs:283`: `let mut session = startup_context.clone();`
- `startup.rs:309`: `let mut session = startup_context.clone();`
- `gate.rs:78`: only mutation site inside evaluate — `session.record_surfaced_peer_write(...)` on the local `session` binding
- `startup.rs:327–329`: `non_empty_insertion` guard
- `coordination_recall_render.rs:94–98`: empty insertion test

---

## Coherence observations

The 12-hour run held together well. The spec, plan, and implementation are tightly aligned across all ten areas examined. A few structural observations:

- **Three-level tier model is correctly separated.** Level 1 is the pass-through floor (no Stream I code runs); Level 2 acquires claim locks and runs the relevance gate; Level 3 additionally registers heartbeat presence and enables renewal. The `handle_peer_heartbeat` path in `presence.rs:251` gates all presence registration and renewal behind `active_level == 3`, and `effective_coordination_level` in `startup.rs:225` correctly applies per-project `.memory-project.yaml` overrides before the level check.
- **Tier 3 short-circuit is correct.** `gate.rs:48–50`: `if session.is_tier3() { return CoordinationInsertion::empty(); }` fires before any scoring loop — zero per-candidate cost for MCP-only sessions.
- **XML rendering is string-building with proper escaping.** `render.rs:445–459` uses `escape_xml_text` and `escape_xml_attr` on all user-controlled content before insertion. Privacy filtering (`safe_peer_summary`) runs on summaries before escaping. No raw string concatenation of user-controlled data.
- **Score function matches spec exactly.** Weights 0.5/0.3/0.2, Jaccard entity overlap, path fraction, cosine similarity. Empty-entity-set returns 0.0 (not 1.0). Embedding triple mismatch returns 0.0. All per spec §4.1.
- **Claim-lock contention is warn-not-refuse.** `claim_lock.rs:176–179` allows the supersede to proceed and returns `Contended`; the caller emits `EventKind::ClaimLockContention` and a warning field on the response. Test `contention_proceeds_with_warning` in `claim_lock_supersede.rs:92–111` verifies end-to-end.
- **Per-project config whitelist and serde.** Both layers updated as spec §8.2 requires — `project.rs` pre-parse whitelist and serde struct field, with `test_preparse_whitelist_blocks_without_serde` confirming the whitelist fires before serde.

---

## Spec coverage matrix

| Spec section                         | Implemented    | Test coverage                                                        | Notes                                           |
| ------------------------------------ | -------------- | -------------------------------------------------------------------- | ----------------------------------------------- |
| §3 Three-level model                 | Yes            | `heartbeat_protocol.rs`, `coordination_integration.rs`               | Level 1/2/3 gating all correct                  |
| §4.1 Score function                  | Yes            | `gate.rs` unit (implied by integration tests)                        | Weights match spec                              |
| §4.2 Threshold/recency/cap/cool-down | Yes            | `coordination_integration.rs` cap and cool-down tests                | `indexed_at` used for recency ✓                 |
| §4.3 Salient entity/path derivation  | Yes            | `session_derivation.rs` (coordination crate)                         | Tier 3 short-circuit ✓                          |
| §5.1 `<peer-update>` shape           | Yes            | `coordination_recall_render.rs`                                      | All required attributes present                 |
| §5.2 `<peer-presence>` shape         | Yes            | `coordination_recall_render.rs:test_peer_presence_emitted_at_level3` | Cap-4, entities-5, started attr ✓               |
| §5.3 Startup insertion semantics     | Yes            | `coordination_recall_render.rs:startup_*`                            | Same-device + cross-device ✓                    |
| §6 Heartbeat protocol                | Yes            | `heartbeat_protocol.rs`                                              | `conflicting_claim_locks` Level 3-only (R1)     |
| §7 Claim lock semantics              | Yes            | `claim_lock_supersede.rs`                                            | Contention overwrites holder (R4)               |
| §8.1 Config schema                   | Yes            | `coordination_config.rs`                                             | Fail-closed validation ✓                        |
| §8.2 Per-project override            | Yes            | `project_binding_concurrent_mode.rs`                                 | Both layers (whitelist + serde) ✓               |
| §9 CLI surface                       | Yes (partial)  | `peer_cli.rs` (not read)                                             | `memoryd peer release-lock` confirms admin-only |
| §10 Framing tests                    | Yes (skeleton) | `framing_tests.rs` (pattern matching)                                | Live harness calls remain env-dependent         |
| §11 Acceptance tests                 | Yes            | Multiple integration test files                                      | Good coverage                                   |
| §13.4 Bench fixture                  | Yes            | `bench/stream-i-cross-session-results.darwin-arm64.json`             | Dimension gap R2                                |

---

## Cross-stream invariant check

**Two-clone convergence under peer-update insertion: PASS (with observation)**

Peer-update data lives entirely in daemon RAM (presence registry, claim lock registry, cool-down sets). None of it is written to canonical memory files or the git-tracked event log except `EventKind::ClaimLockContention` (written to the JSONL event log only). The merge driver sees canonical memory files; Stream I's in-memory coordination state does not participate in git merges. After a merge, Stream I rebuilds its state from `indexed_at`/`source_device` fields on `RecallIndexRow`, which are populated from the existing `memories` table columns — no new schema. Two-clone convergence (canonical-content equality per CLAUDE.md invariant 6) is unaffected. `EventKind::ClaimLockContention` entries in the JSONL event log may diverge between clones (each clone logs its own contention events), which is correct behavior — contention is device-local.

**Stream E `<memory-recall>` XML schema integration: PASS**

`render.rs:render_startup_frame_with_cross_device_updates` preserves `version="stream-e-v0.5"` on `<memory-recall>` (line 164). The additive `coordination="stream-i-v0.1"` attribute is present only when coordination entries exist (line 155–159). The `<memory-delta empty="true" />` invariant is preserved: `render_delta_frame` emits it when `body.is_empty()` after all budget checks (line 220–226), including when `CoordinationInsertion` is `Some` but all entries were budget-excluded. XML escaping is applied consistently. No raw string concatenation of user-controlled content.

**Stream A scope authorization: PASS**

Stream I's two new `RecallIndexRow` fields (`indexed_at: DateTime<Utc>` and `source_device: Option<String>`) at `model.rs:1257,1263` are pure struct-field additions populating from pre-existing `memories` table columns (`indexed_at TEXT NOT NULL` and `source_device TEXT`). No new columns, no schema-version bump, consistent with the spec §1.1 statement: "No new columns. No new index. No schema-version bump." The SELECT-list extension in `query.rs:326–327` was verified to include both fields. This matches the system-v0.2 §19 cross-stream surface authorization. No other Stream A files were modified by Stream I beyond these additive field surfaces.
