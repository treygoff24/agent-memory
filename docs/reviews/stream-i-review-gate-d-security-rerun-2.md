Verdict: Changes requested

# Stream I Gate D Security Rerun 2

**Scope:** Review-only security/privacy rerun after fixes for `docs/reviews/stream-i-review-gate-d-security-rerun.md`. I focused on Stream I peer-update / peer-presence XML, delta delivery audit, RAM-only coordination state, and Level 1 short-circuit behavior. Production code was not edited.

## Blocking findings

### S2 - Startup peer-update XML still leaks `source_device` through the `session` attribute

**Status of named delta blocker:** The delta-specific attribution leak is fixed. Delta peer-update candidates now resolve attribution from canonical memory frontmatter (`source.session_id` / `author.session_id`) instead of using `RecallIndexRow::source_device`, and the delivery audit records that resolved session id (`crates/memoryd/src/recall/delta.rs:244-253`, `crates/memoryd/src/recall/delta.rs:268-277`, `crates/memoryd/src/recall/delta.rs:290-294`, `crates/memoryd/src/recall/delta.rs:384-397`). The new integration test asserts the XML does not contain the device-id prefix and the audit uses `sess_peer_writer` (`crates/memoryd/tests/coordination_integration.rs:117-144`).

**Remaining issue:** The startup peer-update path still builds `PeerUpdateEntry.session_id` from `RecallIndexRow::source_device`. `source_device` is documented as the authoring device id, not a session id (`crates/memory-substrate/src/model.rs:1260-1263`). Startup then passes that value to the shared `<peer-update>` renderer, which emits it in the XML `session="..."` attribute (`crates/memoryd/src/recall/startup.rs:315-323`, `crates/memoryd/src/recall/startup.rs:329-330`, `crates/memoryd/src/recall/render.rs:350-355`).

**Exploitability:** Any same-device or cross-device startup peer-update whose recall index row has `source_device = Some("dev_...")` will render a stable device-id prefix as the peer `session` attribute. The existing startup fixtures create memories with both a real `source.session_id = Some("sess_peer_writer")` and `source.device = Some("dev_...")`, but the startup assembler ignores the real session id and uses the device id instead (`crates/memoryd/tests/startup_recall_mcp.rs:466-508`). Existing tests assert only `device="other"` / no same-device `device=` attribute and do not assert that `session` avoids `dev_` (`crates/memoryd/tests/startup_recall_mcp.rs:104-120`, `crates/memoryd/tests/startup_recall_mcp.rs:136-146`).

**Impact:** The same privacy/framing bug fixed for delta remains in startup XML: a stable device identifier is exposed as a session identifier, and the peer attribution is false. This can leak local or remote device identity prefixes to receiving agents during startup recall.

**Minimal remediation:** Mirror the delta fix in startup assembly: resolve peer harness/session from canonical memory frontmatter or event metadata, use the real peer session id in `PeerWriteCandidate.session_id`, and fall back to a non-identifying placeholder such as `unknown` for legacy rows. Add startup regression tests for same-device and cross-device peer-updates asserting `dev_*` does not appear in `<peer-update session="...">` and `sess_peer_writer` or the expected non-identifying placeholder is used.

## Rerun checklist

### Prior blocker 1: Level 3 delta presence must require salient entity/path overlap

**Fixed for delta XML.** `build_delta_coordination` now passes the live `SessionContext` into `active_peer_presence` when Level 3 is active (`crates/memoryd/src/recall/delta.rs:117-131`). `active_peer_presence` still starts with namespace/self/stale filtering, then retains only records satisfying `presence_record_overlaps_session` (`crates/memoryd/src/recall/delta.rs:329-342`). The overlap helper requires either a normalized salient-entity intersection or an exact salient-path intersection (`crates/memoryd/src/recall/delta.rs:360-368`). The integration test covers one overlapping peer and one unrelated same-namespace peer, and asserts only the overlapping peer appears in `<peer-presence>` (`crates/memoryd/tests/coordination_integration.rs:73-97`).

### Prior blocker 2: Delta peer-update attribution must not leak `source_device` via XML `session` or delivery audit

**Fixed for delta XML and delta delivery audit.** Delta still uses `source_device` only for same-device candidate filtering (`crates/memoryd/src/recall/delta.rs:150-166`). It resolves rendered/audited identity from the full memory's `source.session_id` / `author.session_id` instead (`crates/memoryd/src/recall/delta.rs:237-258`, `crates/memoryd/src/recall/delta.rs:261-294`). The shared renderer emits `PeerUpdateEntry.session_id` as the XML `session` attribute (`crates/memoryd/src/recall/render.rs:350-355`), and delta delivery audit copies the same resolved session id into `from_session_id` (`crates/memoryd/src/recall/delta.rs:378-397`, `crates/memoryd/src/handlers.rs:187-198`). The regression test asserts the delta XML uses `sess_peer_writer`, does not contain the device-id prefix, and records `from_session_id == "sess_peer_writer"` (`crates/memoryd/tests/coordination_integration.rs:117-144`).

### No raw memory body in peer XML or delivery audit

**Pass for the reviewed delta path.** The relevance gate copies `RecallIndexRow.summary` into `PeerUpdateEntry.summary`, not the memory body (`crates/memorum-coordination/src/gate.rs:167-178`). XML rendering passes that summary through `safe_peer_summary` / `safe_plaintext_fragment` before insertion (`crates/memoryd/src/recall/render.rs:370-375`, `crates/memoryd/src/recall/render.rs:393-400`). Delta delivery audit stores only `safe_audit_summary(&update.summary)`, which also passes through `safe_plaintext_fragment` and bounds the output (`crates/memoryd/src/recall/delta.rs:384-397`, `crates/memoryd/src/recall/delta.rs:427-434`). The integration fixture contains `raw body secret that must stay out of peer XML`, and the delta test asserts it is absent (`crates/memoryd/tests/coordination_integration.rs:47-52`, `crates/memoryd/tests/coordination_integration.rs:291-358`).

### Cooldown state remains RAM-only

**Pass.** `HandlerState` owns `peer_update_cooldowns: Arc<PeerUpdateCooldowns>` in process state (`crates/memoryd/src/handlers.rs:91-123`). `PeerUpdateCooldowns` is a `StdMutex<BTreeMap<PeerUpdateCooldownKey, BTreeSet<String>>>` with read/record methods only; I found no file, SQLite, or canonical-memory persistence path for it (`crates/memoryd/src/handlers.rs:217-227`, `crates/memoryd/src/handlers.rs:361-392`). The integration test confirms cooldown is scoped to the receiving session in daemon RAM (`crates/memoryd/tests/coordination_integration.rs:146-166`).

### Delivery audit remains RAM-only

**Pass.** `HandlerState` owns `peer_deliveries: Arc<PeerDeliveryAudit>` (`crates/memoryd/src/handlers.rs:91-123`). `PeerDeliveryAudit` is a `StdMutex<VecDeque<PeerDeliveryAuditEntry>>` with bounded `record` and in-memory `snapshot` methods only (`crates/memoryd/src/handlers.rs:212-215`, `crates/memoryd/src/handlers.rs:343-358`).

### Level 1 short-circuit remains intact

**Pass.** Delta coordination returns before querying peer candidates, attaching locks, or reading presence when the effective level is below 2 (`crates/memoryd/src/recall/delta.rs:104-115`). The integration test asserts daemon Level 1 delta has no `coordination=`, `<peer-update>`, or `<peer-presence>` (`crates/memoryd/tests/coordination_integration.rs:17-34`). Supersede claim-lock acquisition also returns inactive before calling the claim-lock registry at Level 1 (`crates/memoryd/src/handlers.rs:1766-1777`).

### Peer admin/MCP boundary spot-check

**Pass.** Peer/admin payloads remain rejected by the MCP forwarding choke point before socket I/O (`crates/memoryd/src/mcp.rs:223-242`).

## Focused validation

Passed:

```bash
cargo test -p memoryd --test coordination_integration --test coordination_recall_render
cargo test -p memorum-coordination --test presence_unit
```

Result: 7 `coordination_integration` tests, 14 `coordination_recall_render` tests, and 23 `presence_unit` tests passed.

Passed, but current assertions do not catch the startup `source_device` leak noted above:

```bash
cargo test -p memoryd --test startup_recall_mcp
```

Result: 12 tests passed.

## Residual risk

- I did not run the full workspace gate; this was a focused security/privacy rerun around the Gate D Stream I coordination surfaces.
- Delta blocker remediation is confirmed, but the same attribution pattern remains in startup peer-update XML and should be fixed before approving Gate D.
- Presence overlap filtering now excludes zero-overlap same-namespace delta XML entries. If broad project identifiers are allowed in `salient_entities`, a future hardening pass may need to distinguish true work-item/entity overlap from namespace-level overlap.

Confidence: high for the delta blocker verification and high for the startup XML attribution finding.
