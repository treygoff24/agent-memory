Verdict: Changes requested

# Stream I Gate D Security Rerun After Delta Coordination Wiring Fix

**Scope:** Review-only security/privacy rerun after new production delta wiring now assembles `<peer-update>` / `<peer-presence>` entries and records peer-update delivery audit entries. Focused files: `crates/memoryd/src/recall/delta.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/protocol.rs`, and relevant `crates/memorum-coordination` presence/gate/claim-lock paths.

## Blocking findings

### S1 - Level 3 delta presence leaks unrelated same-namespace sessions

**Exploitability:** Any Level 3 same-device session in the same namespace can be surfaced to another session's delta even when it shares no salient entity or salient path with the receiving turn. A peer heartbeat can therefore reveal harness, truncated session id, start time, and up to five salient entity ids for unrelated work in the same project.

**Evidence:**

- Delta builds a `SessionContext` with the current message/project salience, but the Level 3 presence path discards it and calls `active_peer_presence` with only `session_binding`, the registry, and stale threshold: `crates/memoryd/src/recall/delta.rs:107-115`.
- `active_peer_presence` queries `PresenceRegistry::active_peers` by namespace/current-session/staleness only, then maps every returned record into `PeerPresenceEntry`: `crates/memoryd/src/recall/delta.rs:267-293`.
- `PresenceRegistry::active_peers` filters only namespace, `own_session_id`, and stale records; it does not check entity/path overlap: `crates/memorum-coordination/src/presence.rs:133-140`.
- The renderer then emits the peer's harness, six-character session prefix, up to five salient entity ids, and start time in XML: `crates/memoryd/src/recall/render.rs:378-390`.

**Impact:** This violates the Stream I presence minimization boundary. Presence should be a narrow collaboration hint, not a same-project activity directory. In practice, unrelated sensitive entity ids such as acquisition, legal, incident, or personnel entities can leak to another Level 3 session simply because both sessions share a project namespace.

**Minimal remediation:** Pass the current `SessionContext` or its salient entity/path sets into the presence selection path. Include only presence records with at least one entity intersection or one path intersection with the current session. Preserve existing self/stale filtering and the cap. Add an integration test with two same-namespace peers: one overlapping and one unrelated; assert only the overlapping peer appears in `<peer-presence>`.

### S2 - Delta peer-update attribution leaks `source_device` through the `session` attribute and delivery audit

**Exploitability:** Any delta peer-update rendered from the recall index uses the row's stable `source_device` value as the peer `session_id`. That value is emitted in the XML `session` attribute and copied into peer delivery audit as `from_session_id`.

**Evidence:**

- `RecallIndexRow::source_device` is documented as the authoring device id, not a session id: `crates/memory-substrate/src/model.rs:1260-1263`.
- Delta candidate assembly sets `PeerWriteCandidate.session_id` from `peer_session_id(row)`: `crates/memoryd/src/recall/delta.rs:221-232`.
- `peer_session_id` returns `row.source_device` when present and falls back to the synthetic string `local-device`: `crates/memoryd/src/recall/delta.rs:235-237`.
- The XML renderer places `entry.session_id` in the `<peer-update session="...">` attribute after only truncating it to eight characters: `crates/memoryd/src/recall/render.rs:350-357`.
- Delivery audit is derived from the same `update.session_id` and recorded unchanged as `from_session_id`: `crates/memoryd/src/recall/delta.rs:304-323`; `crates/memoryd/src/handlers.rs:185-197`.

**Impact:** Same-device peer XML was supposed to avoid exposing raw device identity; the `device` attribute remains absent, but the stable device id is reintroduced through `session`. This is both a privacy leak and a framing problem: the load-bearing `session` attribute no longer identifies the peer session. Audit records inherit the same false/stable identifier.

**Minimal remediation:** Do not derive peer session ids from `source_device`. Project the real source session id/harness from canonical frontmatter or event metadata into the recall-index candidate path, then use that for `from`/`session` and audit. If real source session id is unavailable for legacy rows, use a non-identifying explicit placeholder and avoid leaking `source_device`; add tests asserting a local device id such as `dev_deltaaudit` never appears in `<peer-update session=...>` or `PeerDeliveryAuditEntry.from_session_id`.

## Focus checklist

- **No raw memory body in peer XML:** Pass for the reviewed delta path. Delta peer-update entries are built from `RecallIndexRow.summary`, not memory body (`crates/memorum-coordination/src/gate.rs:167-178`), and the integration fixture's raw body canary is asserted absent from the rendered delta (`crates/memoryd/tests/coordination_integration.rs:47-52`, `crates/memoryd/tests/coordination_integration.rs:240-257`).
- **Safe summaries used in XML:** Pass. `render_peer_update_element` emits `<summary>` from `safe_peer_summary`, and `safe_peer_summary` calls `safe_plaintext_fragment` with the deterministic classifier before truncation/placeholder emission (`crates/memoryd/src/recall/render.rs:350-376`, `crates/memoryd/src/recall/render.rs:393-401`).
- **Safe summaries used in delivery audit:** Pass for summary content. `rendered_peer_deliveries` stores `summary: safe_audit_summary(&update.summary)`, and `safe_audit_summary` calls `safe_plaintext_fragment` before storing either a truncated summary or the privacy placeholder (`crates/memoryd/src/recall/delta.rs:304-323`, `crates/memoryd/src/recall/delta.rs:341-349`). Finding S2 is about the audit's `from_session_id`, not summary/body leakage.
- **Presence excludes current session:** Pass. Delta presence queries pass `own_session_id: Some(&session_binding.session_id)` (`crates/memoryd/src/recall/delta.rs:272-277`), and the registry excludes that session id (`crates/memorum-coordination/src/presence.rs:133-140`). The missing overlap filter is the blocker above.
- **Delivery audit remains RAM-only:** Pass. `HandlerState` owns `peer_deliveries: Arc<PeerDeliveryAudit>` initialized in memory (`crates/memoryd/src/handlers.rs:91-120`); `PeerDeliveryAudit` is only a `StdMutex<VecDeque<PeerDeliveryAuditEntry>>` with record/snapshot methods (`crates/memoryd/src/handlers.rs:200-202`, `crates/memoryd/src/handlers.rs:319-333`). I found no file/database persistence path for `peer_deliveries`.
- **Level 1 short-circuits delta coordination:** Pass. `build_delta_coordination` returns before querying candidates, attaching locks, or reading presence when effective level is below 2 (`crates/memoryd/src/recall/delta.rs:94-103`). The focused integration test confirms no `coordination=`, `<peer-update>`, or `<peer-presence>` at daemon Level 1 (`crates/memoryd/tests/coordination_integration.rs:17-34`).
- **Level 1 skips claim-lock acquisition:** Pass. Supersede claim-lock acquisition returns inactive when the effective coordination level is below 2 (`crates/memoryd/src/handlers.rs:1707-1715`).
- **Claim-lock metadata remains advisory/RAM-safe:** Pass on the reviewed path. Claim locks live in `ClaimLockRegistry`'s `DashMap` (`crates/memorum-coordination/src/claim_lock.rs:148-152`), contention proceeds with an advisory warning/event instead of refusal (`crates/memoryd/src/handlers.rs:1723-1747`), and active locks are projected into peer updates via the in-memory lookup in `attach_claim_locks` (`crates/memoryd/src/recall/delta.rs:256-260`).
- **Peer admin surfaces remain MCP-rejected:** Pass. `PeerHeartbeat`, `PeerStatus`, `PeerActivity`, and `PeerReleaseLock` are rejected by the MCP forwarding choke point before socket I/O (`crates/memoryd/src/mcp.rs:223-242`).

## Focused commands run

```bash
cargo test -p memoryd --test coordination_integration --test coordination_recall_render
```

Result: passed. 4 `coordination_integration` tests and 14 `coordination_recall_render` tests passed. These tests do not cover the zero-overlap presence leak or the source-device-as-session leak.

```bash
cargo test -p memorum-coordination --test presence_unit
```

Result: passed. 23 tests passed. Existing tests cover self/stale exclusion and field bounding, but not entity/path overlap filtering.

```bash
rg -n "safe_plaintext_fragment|safe_peer_summary|safe_audit_summary|peer_session_id|render_peer_presence_element|active_peer_presence|record_delta_peer_delivery|PeerDeliveryAudit|StdMutex<VecDeque|build_delta_response_with_coordination" crates/memoryd/src/recall crates/memoryd/src/handlers.rs crates/memorum-coordination/src -S
```

Result: used to trace the delta summary/privacy/audit/presence paths cited above.

## Residual risk

- I did not run the full workspace gate; this rerun was intentionally focused on the changed delta wiring and adjacent render/audit paths.
- The two blockers are privacy/framing boundary issues that current focused tests do not catch; adding regression tests should be part of the fix.
- I did not review unrelated Stream G/H changes in the dirty tree except where they intersected Stream I delivery/audit surfaces.

Confidence: high for the requested security/privacy checklist and the two blocking findings.
