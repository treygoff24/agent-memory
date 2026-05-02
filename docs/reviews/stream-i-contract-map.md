# Stream I Cross-Session Coordination Contract Map

Date: 2026-05-01
Scope: Task 1 from `docs/plans/2026-05-01-stream-i-cross-session.md`
Normative spec: `docs/specs/stream-i-cross-session-v0.1.md`

## Worktree baseline

Captured before this file was created:

```text
$ git status --short
 M CLAUDE.md
?? docs/plans/2026-05-01-stream-g-observability.md
?? docs/plans/2026-05-01-stream-h-eval-harness.md
?? docs/plans/2026-05-01-stream-i-cross-session.md
?? docs/reviews/stream-g-spec-review.md
?? docs/reviews/stream-ghi-combined-plan-review-pass-2.md
?? docs/reviews/stream-ghi-combined-plan-review.md
?? docs/reviews/stream-h-spec-review.md
?? docs/reviews/stream-i-plan-review.md
?? docs/reviews/stream-i-spec-review.md
?? docs/reviews/system-v0.2-spec-review.md
?? docs/specs/stream-g-observability-v0.1.md
?? docs/specs/stream-h-eval-harness-v0.1.md
?? docs/specs/stream-i-cross-session-v0.1.md
?? docs/specs/system-v0.2.md
?? thoughts/shared/handoffs/dd477f42/
```

Task 1 edits are intentionally limited to this file. The active plan is not edited because the checked plan already records the patched blockers and inter-stream boundaries required by the spec.

## Vertical TDD trace for this docs slice

Behavior gate: the Stream I contract map must exist and must not contain unresolved blocker markers.

RED:

```text
$ test -s docs/reviews/stream-i-contract-map.md; printf 'exit:%s\n' "$?"
exit:1
```

Reason: `docs/reviews/stream-i-contract-map.md` did not exist yet.

GREEN command to rerun after this file is written:

```bash
<blocker-marker scan>
```

Expected GREEN result: no output, exit code 1 from `rg` because none of those unresolved markers exist.

## Patched blockers to preserve during implementation

1. **Tier 3 no-op short-circuit:** `RelevanceGate::evaluate` must check Tier 3 at entry and immediately return `CoordinationInsertion::empty()` with zero candidate scoring. No Tier 3 threshold or degraded scoring path exists for v1.
2. **Heartbeat `started_at` shape:** `PeerHeartbeat.started_at` is `Option<DateTime<Utc>>`. The daemon retains the first non-`None` value per `session_id`; a later heartbeat must not overwrite it.
3. **Two-layer project parser update:** `concurrent_session_mode` requires both the pre-parse whitelist in `crates/memoryd/src/recall/project.rs` and the serde `deny_unknown_fields` target to change in the same implementation slice.
4. **Claim-lock scope clarification:** claim locks are skipped at Level 1, acquired at Levels 2 and 3 on supersede, and renew only through Level 3 heartbeat. They remain daemon-RAM advisory state and are never canonical frontmatter.
5. **`local_observed_at` recency window:** peer-update recency uses the local index-ingest timestamp surfaced as `RecallIndexRow::indexed_at`, not the peer-authored `updated_at` timestamp.

## Inter-stream boundary

- **Stream G owns** the schema-version bump, `EventKind` additions including `RecallHit` and `ClaimLockContention`, and the `events_log` covering index / SQLite mirror work. Stream I must not edit `crates/memory-substrate/src/events/log.rs` for those variants.
- **Stream I owns** the `RecallIndexRow::indexed_at` and `RecallIndexRow::source_device` Rust surface and `query_recall_index` hydration only. No migration, no new column, and no schema-version bump are authorized for this surface because `memories.indexed_at TEXT NOT NULL` and `memories.source_device TEXT` already exist in the shipped schema.
- Presence and claim-lock state live in daemon RAM only. Stream I must not write either state to disk or canonical memory frontmatter.

## Evidence commands recorded

### Spec-term coverage

Command:

```bash
rg -n "CoordinationInsertion|RelevanceGate|SessionContext|PresenceRegistry|ClaimLockRegistry|PeerHeartbeat|peer_update|peer_presence|indexed_at|local_observed_at|concurrent_session_mode|framing_tests" docs/specs/stream-i-cross-session-v0.1.md
```

Evidence highlights:

- `CoordinationInsertion`: DTO shape and insertion contract at lines 51-73; Level 1/2/3 generation behavior at lines 170-192; budget ownership at line 1011.
- `RecallIndexRow::indexed_at` and `source_device`: Stream A additive surface at lines 93-103; `local_observed_at` semantic recency language at lines 266, 422, and 658.
- `SessionContext` / `RelevanceGate`: score input at line 221; session fields at lines 273-320; async embedding cache at line 326; Tier 3 short-circuit at line 320.
- `PresenceRegistry` / `PeerHeartbeat`: protocol shape at lines 454-473; RAM-only registry at lines 502-523; restart semantics at line 550.
- `concurrent_session_mode`: project override and parser two-layer warning at lines 695-735.
- `framing_tests`: assertion surface and misattribution pattern list at lines 869-888.
- §11 acceptance bullets: gate unit tests at lines 911-924, session derivation at lines 926-929, presence at lines 931-936, claim locks at lines 938-945, integration tests at lines 949-960.

### Current code choke points

Command:

```bash
rg -n "RequestPayload|ResponsePayload|RecallIndexRow|query_recall_index|recall.*project|project.*recall" crates
```

Evidence highlights:

- Daemon protocol choke point: `crates/memoryd/src/protocol.rs:39` owns `RequestPayload`; `crates/memoryd/src/protocol.rs:157` owns `ResponsePayload`.
- Daemon handler dispatch: `crates/memoryd/src/handlers.rs:88-123` matches `RequestPayload` variants and returns `ResponsePayload` variants.
- MCP forwarding boundary: `crates/memoryd/src/mcp.rs:167-175` maps the frozen nine tools to existing daemon `RequestPayload` variants; peer CLI commands must remain outside this path.
- Recall candidate query seam: `crates/memoryd/src/recall/candidates.rs:51-87` abstracts `query_recall_index` into recall candidate collection.
- Stream A recall row and query seam: `crates/memory-substrate/src/model.rs:1219` defines `RecallIndexRow`; `crates/memory-substrate/src/index/query.rs:286` defines `query_recall_index`; `crates/memory-substrate/src/index/query.rs:871` hydrates rows.
- Project parser seam: `crates/memoryd/src/recall/project.rs` currently accepts only `canonical_id | alias` in its pre-parse whitelist; it is the first layer that must accept `concurrent_session_mode`.

Additional project-parser check:

```text
$ rg -n "recall.*project|project.*recall" crates/memoryd/src crates/memoryd/tests | sed -n '1,80p'
crates/memoryd/src/recall/binding.rs:5:use crate::recall::project::resolve_project_binding;
crates/memoryd/src/recall/binding.rs:76:fn namespaces_for(project: Option<&crate::recall::types::ProjectBinding>) -> Vec<String> {
crates/memoryd/src/recall/startup.rs:119:        guidance: "Stream E passive recall assembled from read-only Stream A index projections.".to_owned(),
crates/memoryd/tests/startup_recall_privacy.rs:30:    assert!(startup.recall_block.contains("safe project fact"));
crates/memoryd/tests/startup_recall_privacy.rs:87:async fn startup_recall_escapes_identity_and_project_text_fields() {
crates/memoryd/tests/startup_recall_privacy.rs:130:    assert!(startup.recall_block.contains("project&lt;/project-state&gt;&lt;script&gt;&amp;"));
```

Current parser and Stream A surface excerpts:

```text
crates/memoryd/src/recall/project.rs: pre-parse whitelist currently matches only "canonical_id" | "alias".
crates/memory-substrate/src/model.rs:1219: pub struct RecallIndexRow currently lacks indexed_at/source_device fields.
crates/memory-substrate/src/index/query.rs:286: query_recall_index SELECT list currently lacks indexed_at/source_device.
```

## §11.1 unit-test acceptance map

### `crates/memorum-coordination/tests/gate_unit.rs`

| Acceptance bullet                                                                                                | Implementation task(s)                                                                           | Owned files                                                                                                                                                                                          | Narrow gate(s)                                                                                                                                         |
| ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `test_score_entity_overlap_only` verifies score with only entity overlap.                                        | Task 5 implements score helpers and weighted score.                                              | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_score_path_overlap_only` verifies score with only path overlap.                                            | Task 5 implements exact path fraction scoring.                                                   | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_score_all_components` verifies all non-zero components and `f64::EPSILON` tolerance.                       | Task 5 implements full weighted score `(0.5, 0.3, 0.2)`.                                         | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_threshold_boundary` verifies exactly `0.6` surfaces and `0.5999` does not.                                 | Task 5 implements threshold handling.                                                            | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_per_turn_cap` verifies top 2 selection and `capped_peer_updates = 3`.                                      | Task 5 implements cap/sort/capped count.                                                         | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_cool_down` verifies surfaced peer-write ids do not repeat for the same session.                            | Task 5 implements cooldown filtering using `surfaced_peer_writes` from session context.          | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_recency_window_uses_local_observed_at` verifies recency uses local observation, not authored `updated_at`. | Task 5 starts gate coverage; Task 15 enforces `RecallIndexRow::indexed_at` in integration.       | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                      | `cargo test -p memorum-coordination --test gate_unit test_recency_window`; `cargo test -p memoryd --test coordination_integration test_recency_window` |
| `test_tier3_returns_empty` verifies Tier 3 returns empty insertion regardless of available signals.              | Task 5 implements entry short-circuit; Task 18 hardens no-scoring spy coverage.                  | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit test_tier3`; `cargo test -p memorum-coordination --test gate_unit`                                |
| `test_entity_overlap_required_property` verifies E=0, P=1, T=1 scores 0.5 and does not surface.                  | Task 5 implements the named design-property test.                                                | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_empty_entity_sets` verifies empty-empty entity sets score `0.0`.                                           | Task 5 implements entity Jaccard empty-set semantics.                                            | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/gate_unit.rs`                                                                                                          | `cargo test -p memorum-coordination --test gate_unit`                                                                                                  |
| `test_embedding_triple_mismatch` verifies mismatched triples yield topic similarity `0.0` without error.         | Task 5 covers helper semantics; Task 8 wires embedding cache/triple matching through `evaluate`. | `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/src/session.rs`; `crates/memorum-coordination/tests/gate_unit.rs`; `crates/memorum-coordination/tests/session_derivation.rs` | `cargo test -p memorum-coordination --test gate_unit`; `cargo test -p memorum-coordination --test session_derivation`                                  |

### `crates/memorum-coordination/tests/session_derivation.rs`

| Acceptance bullet                                                                                          | Implementation task(s)                                                          | Owned files                                                                                                                                                                                          | Narrow gate(s)                                                                                                                   |
| ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `test_salient_entities_from_startup_recall` populates salient entities from mocked recall explanation.     | Task 6 implements Tier 1 salient-entity derivation.                             | `crates/memorum-coordination/src/session.rs`; `crates/memorum-coordination/tests/session_derivation.rs`                                                                                              | `cargo test -p memorum-coordination --test session_derivation`                                                                   |
| `test_salient_paths_from_selected_ids` populates salient paths from selected memory ids resolved to paths. | Task 7 implements salient-path derivation.                                      | `crates/memorum-coordination/src/session.rs`; `crates/memorum-coordination/tests/session_derivation.rs`                                                                                              | `cargo test -p memorum-coordination --test session_derivation`                                                                   |
| `test_relevance_gate_skipped_for_tier3` verifies `RelevanceGate::evaluate` short-circuits before scoring.  | Task 6 adds session-side tier context; Task 18 hardens gate-side short-circuit. | `crates/memorum-coordination/src/session.rs`; `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/tests/session_derivation.rs`; `crates/memorum-coordination/tests/gate_unit.rs` | `cargo test -p memorum-coordination --test session_derivation`; `cargo test -p memorum-coordination --test gate_unit test_tier3` |

### `crates/memorum-coordination/tests/presence_unit.rs`

| Acceptance bullet                                                           | Implementation task(s)                                                     | Owned files                                                                                                                                                                                                           | Narrow gate(s)                                                                                                  |
| --------------------------------------------------------------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `test_upsert_and_snapshot` returns records only for the matching namespace. | Task 9 implements `PresenceRegistry::upsert` and `snapshot_for_namespace`. | `crates/memorum-coordination/src/presence.rs`; `crates/memorum-coordination/tests/presence_unit.rs`                                                                                                                   | `cargo test -p memorum-coordination --test presence_unit`                                                       |
| `test_stale_removal` removes records older than `stale_after_seconds`.      | Task 9 implements registry cleanup; Task 11 wires daemon sweeper.          | `crates/memorum-coordination/src/presence.rs`; `crates/memorum-coordination/tests/presence_unit.rs`; `crates/memoryd/src/server.rs`; `crates/memoryd/src/workers.rs`; `crates/memoryd/tests/stale_session_cleanup.rs` | `cargo test -p memorum-coordination --test presence_unit`; `cargo test -p memoryd --test stale_session_cleanup` |
| `test_fresh_not_removed` preserves recent heartbeat records.                | Task 9 implements monotonic `Instant` stale comparison.                    | `crates/memorum-coordination/src/presence.rs`; `crates/memorum-coordination/tests/presence_unit.rs`                                                                                                                   | `cargo test -p memorum-coordination --test presence_unit`                                                       |
| `test_concurrent_upsert` gives one last-write-wins record per `session_id`. | Task 9 implements DashMap-backed concurrent upsert.                        | `crates/memorum-coordination/src/presence.rs`; `crates/memorum-coordination/tests/presence_unit.rs`                                                                                                                   | `cargo test -p memorum-coordination --test presence_unit`                                                       |

### `crates/memorum-coordination/tests/claim_lock_unit.rs`

| Acceptance bullet                                                          | Implementation task(s)                                                                       | Owned files                                                                                                                                                                                                               | Narrow gate(s)                                                                                                    |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `test_acquire_success` acquires an unlocked memory id.                     | Task 12 implements `ClaimLockRegistry::acquire`.                                             | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`                                                                                                                   | `cargo test -p memorum-coordination --test claim_lock_unit`                                                       |
| `test_renew_extends_ttl` extends TTL from the renew time.                  | Task 12 implements `renew`; Task 10 provides heartbeat DTO; Task 11/12 connect renewal path. | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`; `crates/memoryd/src/protocol.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/heartbeat_protocol.rs` | `cargo test -p memorum-coordination --test claim_lock_unit`; `cargo test -p memoryd --test heartbeat_protocol`    |
| `test_release_clears_lock` releases a holder lock.                         | Task 12 implements `release`.                                                                | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`                                                                                                                   | `cargo test -p memorum-coordination --test claim_lock_unit`                                                       |
| `test_ttl_expiry` removes expired locks.                                   | Task 12 implements expiry sweep; Task 11 invokes it from daemon cleanup.                     | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`; `crates/memoryd/src/server.rs`; `crates/memoryd/src/workers.rs`; `crates/memoryd/tests/stale_session_cleanup.rs` | `cargo test -p memorum-coordination --test claim_lock_unit`; `cargo test -p memoryd --test stale_session_cleanup` |
| `test_contention_warn_not_refuse` proceeds with warning on contention.     | Task 12 models advisory contention; Task 13 wires warning response and event emission.       | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/claim_lock_supersede.rs`                                 | `cargo test -p memorum-coordination --test claim_lock_unit`; `cargo test -p memoryd --test claim_lock_supersede`  |
| `test_stale_session_releases_lock` releases locks when session goes stale. | Task 12 implements `release_all_held_by`; Task 11 calls it from stale-session cleanup.       | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`; `crates/memoryd/src/server.rs`; `crates/memoryd/src/workers.rs`; `crates/memoryd/tests/stale_session_cleanup.rs` | `cargo test -p memorum-coordination --test claim_lock_unit`; `cargo test -p memoryd --test stale_session_cleanup` |

## §11.2 integration-test acceptance map

### `crates/memoryd/tests/coordination_integration.rs`

| Acceptance bullet                                                                                                      | Implementation task(s)                                                                              | Owned files                                                                                                                                                                                                                                                                                                      | Narrow gate(s)                                                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `test_level2_peer_update_inserted` inserts one `<peer-update>` with correct `from`, `session`, `ref`, and `namespace`. | Task 14 renders coordination entries; Task 15 wires live recency; Task 17 resolves Level 2.         | `crates/memoryd/src/recall/render.rs`; `crates/memoryd/src/recall/mod.rs`; `crates/memoryd/src/recall/types.rs`; `crates/memorum-coordination/src/gate.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/coordination_recall_render.rs`; `crates/memoryd/tests/coordination_integration.rs`           | `cargo test -p memoryd --test coordination_recall_render`; `cargo test -p memoryd --test coordination_integration`                                                        |
| `test_level2_no_insert_below_threshold` omits peer-update below threshold.                                             | Task 5 implements threshold; Task 14/17 ensure empty coordination produces no XML or attribute.     | `crates/memorum-coordination/src/gate.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                                                                                           | `cargo test -p memorum-coordination --test gate_unit`; `cargo test -p memoryd --test coordination_integration`                                                            |
| `test_level2_cool_down_suppresses_repeat` suppresses repeat surfacing for the same memory.                             | Task 5 implements cooldown registry semantics; Task 15/17 wire through daemon integration.          | `crates/memorum-coordination/src/gate.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                                                                                                                                  | `cargo test -p memorum-coordination --test gate_unit`; `cargo test -p memoryd --test coordination_integration`                                                            |
| `test_level2_cap_two_entries` emits two entries and counts the overflow in `<pending-attention>`.                      | Task 5 implements cap count; Task 14 renders pending-attention addition.                            | `crates/memorum-coordination/src/gate.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/coordination_recall_render.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                                                                       | `cargo test -p memorum-coordination --test gate_unit`; `cargo test -p memoryd --test coordination_recall_render`; `cargo test -p memoryd --test coordination_integration` |
| `test_level1_no_peer_update` confirms `concurrent_session_mode: minimal` disables peer-update XML.                     | Task 3 parses `concurrent_session_mode`; Task 17 applies Level 1 short-circuit.                     | `crates/memoryd/src/recall/project.rs`; `crates/memoryd/tests/project_binding_concurrent_mode.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                                                                          | `cargo test -p memoryd --test project_binding_concurrent_mode`; `cargo test -p memoryd --test coordination_integration test_level`                                        |
| `test_level3_presence_in_delta` emits `<peer-presence>` for collaborative project peer heartbeat.                      | Task 9 implements registry; Task 10 heartbeat; Task 14 rendering; Task 17 Level 3 enforcement.      | `crates/memorum-coordination/src/presence.rs`; `crates/memoryd/src/protocol.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/heartbeat_protocol.rs`; `crates/memoryd/tests/coordination_recall_render.rs`; `crates/memoryd/tests/coordination_integration.rs` | `cargo test -p memorum-coordination --test presence_unit`; `cargo test -p memoryd --test heartbeat_protocol`; `cargo test -p memoryd --test coordination_integration`     |
| `test_level3_no_presence_unrelated_namespace` filters presence to the same namespace.                                  | Task 9 implements exact namespace snapshots; Task 14 renders only selected presence entries.        | `crates/memorum-coordination/src/presence.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                                                                                                                         | `cargo test -p memorum-coordination --test presence_unit`; `cargo test -p memoryd --test coordination_integration`                                                        |
| `test_claim_lock_in_peer_update` renders `claim_locked="claude-code:sess_A"` on matching peer-update.                  | Task 12 exposes active lock lookup; Task 13 wires supersede acquisition; Task 14 renders attribute. | `crates/memorum-coordination/src/claim_lock.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/claim_lock_supersede.rs`; `crates/memoryd/tests/coordination_recall_render.rs`; `crates/memoryd/tests/coordination_integration.rs`                               | `cargo test -p memorum-coordination --test claim_lock_unit`; `cargo test -p memoryd --test claim_lock_supersede`; `cargo test -p memoryd --test coordination_integration` |
| `test_cross_device_startup_peer_update` emits `<cross-device-updates>` inside `<entity-recall>`.                       | Task 2 surfaces `source_device`; Task 16 implements cross-device startup split/rendering.           | `crates/memory-substrate/src/model.rs`; `crates/memory-substrate/src/index/query.rs`; `crates/memory-substrate/tests/recall_index_row_source_device.rs`; `crates/memoryd/src/recall/render.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/coordination_integration.rs`                             | `cargo test -p memory-substrate --test recall_index_row_source_device`; `cargo test -p memoryd --test coordination_integration test_cross_device`                         |
| `test_startup_no_cross_device_outside_window` excludes cross-device peer writes outside the startup window.            | Task 2 surfaces `indexed_at`; Task 16 applies cross-device startup window.                          | `crates/memory-substrate/src/model.rs`; `crates/memory-substrate/src/index/query.rs`; `crates/memory-substrate/tests/recall_index_row_indexed_at.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                       | `cargo test -p memory-substrate --test recall_index_row_indexed_at`; `cargo test -p memoryd --test coordination_integration test_startup`                                 |
| `test_coordination_attribute_on_delta` emits `coordination="stream-i-v0.1"` only when peer entries are present.        | Task 14 implements conditional coordination attribute; Task 17 verifies Level 1 absence.            | `crates/memoryd/src/recall/render.rs`; `crates/memoryd/tests/coordination_recall_render.rs`; `crates/memoryd/tests/coordination_integration.rs`                                                                                                                                                                  | `cargo test -p memoryd --test coordination_recall_render`; `cargo test -p memoryd --test coordination_integration`                                                        |

## Supporting implementation tasks outside the §11 bullets

These tasks are prerequisites or release gates for the §11 matrix even where they do not map one-to-one to a §11 acceptance bullet:

| Task    | Contract purpose                                                                          | Owned files                                                                                                                                                                                                                                                | Narrow gate(s)                                                                                                                                                      |
| ------- | ----------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ------------- | ----------------------- | ------------------- | ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Task 2  | Stream A `RecallIndexRow::indexed_at` and `source_device` surface with no migration.      | `crates/memory-substrate/src/model.rs`; `crates/memory-substrate/src/index/query.rs`; `crates/memory-substrate/tests/recall_index_row_indexed_at.rs`; `crates/memory-substrate/tests/recall_index_row_source_device.rs`; `docs/api/stream-a-public-api.md` | `cargo test -p memory-substrate --test recall_index_row_indexed_at`; `cargo test -p memory-substrate --test recall_index_row_source_device`                         |
| Task 3  | Stream E parser two-layer `concurrent_session_mode` acceptance/rejection.                 | `crates/memoryd/src/recall/project.rs`; `crates/memoryd/tests/project_binding_concurrent_mode.rs`                                                                                                                                                          | `cargo test -p memoryd --test project_binding_concurrent_mode`                                                                                                      |
| Task 4  | Workspace skeleton and public DTO/module layout for `memorum-coordination`.               | Workspace `Cargo.toml`; `crates/memorum-coordination/**`; placeholder test files.                                                                                                                                                                          | `cargo build -p memorum-coordination`; `cargo test -p memorum-coordination`                                                                                         |
| Task 10 | Heartbeat protocol and handler surface, including `started_at: Option<DateTime<Utc>>`.    | `crates/memoryd/src/protocol.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/heartbeat_protocol.rs`                                                                                                                                           | `cargo test -p memoryd --test heartbeat_protocol`                                                                                                                   |
| Task 11 | Non-blocking stale-session sweeper and RAM-only cleanup.                                  | `crates/memoryd/src/server.rs`; `crates/memoryd/src/workers.rs`; `crates/memoryd/tests/stale_session_cleanup.rs`                                                                                                                                           | `cargo test -p memoryd --test stale_session_cleanup`                                                                                                                |
| Task 13 | Claim-lock wiring into supersede with Stream G-owned event variant consumed after rebase. | `crates/memoryd/src/handlers.rs`; `crates/memoryd/tests/claim_lock_supersede.rs`                                                                                                                                                                           | `cargo test -p memoryd --test claim_lock_supersede`; `cargo test -p memoryd --test governance_e2e`                                                                  |
| Task 19 | Admin-only `memoryd peer` CLI, activity ring buffer, and explicit non-MCP exposure.       | `crates/memoryd/src/cli.rs`; `crates/memoryd/src/handlers.rs`; `crates/memoryd/src/protocol.rs`; `crates/memoryd/tests/peer_cli.rs`                                                                                                                        | `cargo test -p memoryd --test peer_cli`; `rg -n "peer_status\|peer_activity\|peer_release" crates/memoryd/src/mcp.rs` with no matches                               |
| Task 20 | Stream H framing fixture and assertion helper owned by Stream I.                          | `crates/memorum-eval/fixtures/prompts/t19_peer_update_framing.md`; `crates/memorum-coordination/src/framing_tests.rs`                                                                                                                                      | `cargo test -p memorum-coordination --lib framing_tests`                                                                                                            |
| Task 21 | Performance bench for p95 <= 5ms relevance gate budget.                                   | `crates/memorum-coordination/src/bin/peer_relevance_bench.rs`; `bench/stream-i-cross-session-results.darwin-arm64.json`; `docs/reviews/stream-i-bench-evidence.md`; `crates/memorum-coordination/Cargo.toml`                                               | `cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json` |
| Task 22 | Public API and architecture docs for Stream I.                                            | `docs/api/stream-i-cross-session-api.md`; `docs/dev/stream-i-architecture.md`; `CLAUDE.md`                                                                                                                                                                 | `rg -n "CoordinationInsertion                                                                                                                                       | peer-update | peer-presence | concurrent_session_mode | memoryd peer status | coordination.\*stream-i-v0.1 | local_observed_at" docs/api/stream-i-cross-session-api.md docs/dev/stream-i-architecture.md`; `git diff --check docs/api docs/dev CLAUDE.md` |

## Review gates to preserve

- **Review Gate A:** after Tasks 1-3, inspect Stream A `indexed_at/source_device` hydration and Stream E parser seam.
- **Review Gate B:** after Tasks 4-8, inspect score function, Tier 3 short-circuit, session derivation, and embedding cache behavior.
- **Review Gate C:** after Tasks 9-13, inspect presence, claim locks, concurrency, heartbeat `started_at`, and advisory contention.
- **Review Gate D:** after Tasks 14-19, inspect recall XML rendering, budget accounting, privacy filtering, Level 1 short-circuit, cross-device framing, and CLI non-MCP exposure.
- **Final Review Gate E:** after Tasks 20-22, run independent clean-code, security, performance, test, and API-contract review before final release gate.

## Unresolved blockers

None.
