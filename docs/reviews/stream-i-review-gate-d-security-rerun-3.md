Verdict: Approved

# Stream I Gate D Security Rerun 3

**Scope:** Review-only security/privacy rerun after source identity, startup attribution, and Level 3 presence path-overlap fixes. I focused on Stream I peer-update / peer-presence XML, delta delivery audit, RAM-only cooldown/audit state, and Level 1 short-circuit behavior. Production code was not edited.

## Findings by severity

No blocking security, privacy, auth/session, injection, secret-exposure, or authorization-boundary findings in the requested Gate D rerun scope.

## Prior blocker verification

### Prior blocker: Level 3 delta presence must require entity OR path overlap

**Status: fixed.** Delta coordination constructs the live `SessionContext` from the receiving session/message, then passes it into `active_peer_presence` only when effective coordination level is at least 3 (`crates/memoryd/src/recall/delta.rs:116-130`). `active_peer_presence` still starts from the namespace/self/staleness filtered registry query, but then retains only records for which `presence_record_overlaps_session` returns true (`crates/memoryd/src/recall/delta.rs:288-316`). The helper requires either normalized salient-entity intersection or exact salient-path intersection (`crates/memoryd/src/recall/delta.rs:319-327`).

**Regression coverage:** `level3_presence_requires_salient_entity_or_path_overlap` asserts an overlapping same-namespace peer is rendered and a no-overlap same-namespace peer is excluded (`crates/memoryd/tests/coordination_integration.rs:73-97`). `level3_presence_renders_for_salient_path_overlap_without_entity_overlap` separately proves path-only overlap is enough and an unrelated path peer is excluded (`crates/memoryd/tests/coordination_integration.rs:99-123`).

**Exploitability after fix:** Low for the prior leak. A same-namespace peer heartbeat alone is no longer enough to surface presence in delta XML; the peer must overlap the receiving turn's salient entities or paths.

### Prior blocker: Delta XML/audit must not leak `source_device` as `session`

**Status: fixed.** `RecallIndexRow::source_device` remains documented as a device id (`crates/memory-substrate/src/model.rs:1254-1263`) and is now used in the delta path only for same-device candidate filtering (`crates/memoryd/src/recall/delta.rs:145-166`). Delta candidate assembly hydrates source identity via `peer_source_identity`, skips self/current-session writes by that resolved identity, and passes the resolved harness/session into `PeerWriteCandidate` (`crates/memoryd/src/recall/delta.rs:236-257`). `peer_source_identity` reads canonical memory frontmatter and uses `source.harness/session_id` or `author.harness/session_id`, falling back to non-identifying `unknown` on read failure or missing fields (`crates/memoryd/src/recall/source_identity.rs:11-39`).

The XML renderer emits `PeerUpdateEntry.session_id` as the truncated `session` attribute (`crates/memoryd/src/recall/render.rs:350-357`), and delta audit records the same resolved `update.session_id` into `from_session_id` (`crates/memoryd/src/recall/delta.rs:337-357`; `crates/memoryd/src/protocol.rs:393-403`).

**Regression coverage:** `peer_update_uses_actual_writer_attribution_in_delta_and_audit` writes a memory with `source.device = dev_deltaattribution` but `source.session_id = sess_peer_writer`, then asserts delta XML renders `from="claude-code" session="sess_pee"`, excludes the `dev_` session prefix, and stores audit `from_session_id == sess_peer_writer` (`crates/memoryd/tests/coordination_integration.rs:143-170`).

**Exploitability after fix:** Low for the prior delta leak. A raw device id is not projected into the delta XML `session` attribute or delta delivery audit session field on the reviewed path.

### Prior blocker: Startup peer-update XML must not leak `source_device` as `session`

**Status: fixed.** Startup now mirrors the delta identity path: same-device and cross-device startup candidate assembly call `peer_write_candidates`, which resolves `peer_source_identity` from canonical memory frontmatter and stores the resolved harness/session in `PeerWriteCandidate` (`crates/memoryd/src/recall/startup.rs:245-290`, `crates/memoryd/src/recall/startup.rs:326-348`). `source_device` is still used only to partition same-device versus cross-device startup rows (`crates/memoryd/src/recall/startup.rs:198-205`), not as rendered session identity. The shared peer-update renderer then emits the resolved `PeerUpdateEntry.session_id` (`crates/memoryd/src/recall/render.rs:350-357`).

**Regression coverage:** `test_startup_peer_update_uses_writer_attribution_not_source_device` writes a startup fixture with `source_device = dev_startup` and `source.session_id = sess_peer_writer`, then asserts the peer-update opening contains `from="claude-code"`, contains `session="sess_pee"`, and excludes the `dev_sta`/`dev_startup` session value (`crates/memoryd/tests/startup_recall_mcp.rs:149-174`).

**Exploitability after fix:** Low for the prior startup leak. A startup peer-update can still carry `device="other"` for cross-device framing, but the `session` attribute is no longer sourced from the stable device id.

## Requested security checklist

### No raw memory body in peer XML/audit

**Pass.** Relevance gate entries copy `RecallIndexRow.summary`, not canonical memory body, into `PeerUpdateEntry.summary` (`crates/memorum-coordination/src/gate.rs:167-178`). Delta and startup peer-update XML both flow through the same renderer, which calls `safe_peer_summary` before emitting `<summary>` (`crates/memoryd/src/recall/render.rs:350-376`, `crates/memoryd/src/recall/render.rs:393-401`). Delta delivery audit stores only `safe_audit_summary(&update.summary)`, which uses `safe_plaintext_fragment` and a byte cap (`crates/memoryd/src/recall/delta.rs:337-357`, `crates/memoryd/src/recall/delta.rs:386-394`). The delta fixture includes a raw-body canary and the integration test asserts it is not in the rendered peer XML (`crates/memoryd/tests/coordination_integration.rs:47-52`, `crates/memoryd/tests/coordination_integration.rs:317-385`).

### Cooldown state remains RAM-only

**Pass.** `HandlerState` owns `peer_update_cooldowns: Arc<PeerUpdateCooldowns>` initialized in process memory (`crates/memoryd/src/handlers.rs:91-127`). `PeerUpdateCooldowns` is a `StdMutex<BTreeMap<PeerUpdateCooldownKey, BTreeSet<String>>>` with read/record methods only (`crates/memoryd/src/handlers.rs:217-227`, `crates/memoryd/src/handlers.rs:361-392`). The delta handler injects the in-memory state through `DeltaPeerCooldownStore` and records only rendered peer deliveries back into that RAM store (`crates/memoryd/src/handlers.rs:202-210`, `crates/memoryd/src/handlers.rs:1131-1144`; `crates/memoryd/src/recall/delta.rs:81-89`, `crates/memoryd/src/recall/delta.rs:369-379`). I found no file, SQLite, or canonical-memory persistence path for peer-update cooldowns.

**Regression coverage:** `peer_update_cooldown_is_per_receiving_session_in_daemon_ram` proves a first delta renders a peer update, a second delta for the same receiver suppresses it, and a different receiver can still see it (`crates/memoryd/tests/coordination_integration.rs:172-192`).

### Delivery audit remains RAM-only and summary-safe

**Pass.** `HandlerState` owns `peer_deliveries: Arc<PeerDeliveryAudit>` initialized in memory (`crates/memoryd/src/handlers.rs:91-127`). `PeerDeliveryAudit` is a bounded `StdMutex<VecDeque<PeerDeliveryAuditEntry>>` with in-memory `record` and `snapshot` methods only (`crates/memoryd/src/handlers.rs:212-215`, `crates/memoryd/src/handlers.rs:343-358`). The admin activity surface reads the in-memory snapshot (`crates/memoryd/src/handlers.rs:569-594`). I found no persistence path for peer delivery audit state.

### Level 1 short-circuit intact

**Pass.** Delta coordination returns before peer candidate querying, relevance gating, claim-lock attachment, or presence lookup when effective coordination level is below 2 (`crates/memoryd/src/recall/delta.rs:103-133`). Startup peer-update construction likewise returns empty before querying candidate rows when effective level is below 2 (`crates/memoryd/src/recall/startup.rs:184-195`). Handler-level coordination wiring passes RAM state into delta only after request dispatch, but the delta builder enforces the short-circuit before touching Stream I peer surfaces (`crates/memoryd/src/handlers.rs:1131-1144`).

**Regression coverage:** `level1_daemon_delta_has_no_coordination_insertion` asserts no `coordination=`, `<peer-update>`, or `<peer-presence>` at daemon Level 1 (`crates/memoryd/tests/coordination_integration.rs:17-34`). `test_level1_no_peer_update_from_project_mode` asserts project `minimal` mode suppresses startup peer updates and coordination attributes (`crates/memoryd/tests/startup_recall_mcp.rs:176-192`).

## Focused validation

Passed:

```bash
cargo test -p memoryd --test coordination_integration --test startup_recall_mcp --test coordination_recall_render
```

Result: 8 `coordination_integration` tests, 13 `startup_recall_mcp` tests, and 14 `coordination_recall_render` tests passed.

Passed:

```bash
cargo test -p memorum-coordination --test gate_unit --test presence_unit
```

Result: 17 `gate_unit` tests and 23 `presence_unit` tests passed.

## Residual risk

- I did not run the full workspace gate; this rerun was intentionally focused on Stream I Gate D security/privacy surfaces and the prior blockers.
- Presence overlap currently treats any matching salient entity id as sufficient. If future heartbeat producers include broad project identifiers as salient entities for every session, a later hardening pass should distinguish namespace/project identifiers from true work-item entities or rely more heavily on path overlap.
- `peer_source_identity` falls back to `unknown` for legacy/unreadable memories. That avoids leaking `source_device`, but multiple legacy peers may collapse to the same displayed attribution until source frontmatter is present.

Confidence: high for the requested prior-blocker verification and focused security checklist.
