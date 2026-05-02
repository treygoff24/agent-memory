Verdict: Changes requested

# Stream I Gate D Test Rerun 2

## Scope

Review-only rerun after the latest Stream I Gate D fixes. I inspected `crates/memoryd/tests/coordination_integration.rs`, the requested adjacent tests, and the production delta/handler coordination code. I did not edit production code.

## Blocking finding

### S1 - Presence-overlap coverage misses the path-overlap branch required by the Stream I contract

The new presence-overlap regression proves that an entity-overlapping Level 3 peer appears and an unrelated same-namespace peer is filtered out, but it does not cover the path-only overlap behavior required by the Stream I contract.

Evidence:

- The spec requires `<peer-presence>` filtering by either entity overlap or path overlap: “at least one entity in common ... OR at least one path in common ...” (`docs/specs/stream-i-cross-session-v0.1.md:390`).
- Production implements that as a real two-branch predicate: entity overlap is checked first, then `record.salient_paths` are compared with `session.salient_paths` (`crates/memoryd/src/recall/delta.rs:360-368`).
- The new integration test only creates peer presence records with entity sets: the overlapping peer has `ent_stream_i_presence`, and the unrelated peer has `ent_unrelated_work` (`crates/memoryd/tests/coordination_integration.rs:73-96`).
- The helper used by that test always sets `salient_paths: Vec::new()` for every presence record, so no test record can exercise the path-overlap branch (`crates/memoryd/tests/coordination_integration.rs:371-388`).
- The delta message in that test is only `ent_stream_i_presence`, so the receiving session has entity salience but no path-only overlap fixture (`crates/memoryd/tests/coordination_integration.rs:90`).

Why this blocks this rerun: the task specifically asked to verify coverage for the newly fixed presence-overlap behavior. The current test catches the previous “zero-overlap same-namespace peer leaks” class when the positive case is entity overlap, but a regression that accidentally removed or broke the path-overlap half of the production `OR` would not fail.

Required remediation: add a second daemon-level regression in `coordination_integration.rs` that inserts a Level 3 peer presence record with no shared entities but a shared `salient_paths` value, calls delta with a message containing that exact path, and asserts that the path-overlapping peer appears while a no-overlap peer remains absent.

## Covered acceptance evidence

### Prior delta coordination coverage remains present

- Level 1 daemon delta still asserts no `coordination=`, no `<peer-update>`, and no `<peer-presence>` through `RequestPayload::Delta` (`crates/memoryd/tests/coordination_integration.rs:17-34`, `crates/memoryd/tests/coordination_integration.rs:201-221`). Production returns no delta coordination below level 2 (`crates/memoryd/src/recall/delta.rs:104-113`).
- Level 2 daemon delta still writes a real peer memory, calls the handler path, and asserts Stream I coordination, `<peer-update>`, `<ref>`, claim-lock metadata, and no raw body leakage (`crates/memoryd/tests/coordination_integration.rs:36-52`, `crates/memoryd/tests/coordination_integration.rs:235-358`).
- Level 3 daemon delta still asserts `<peer-presence>` precedes `<peer-update>` and excludes the current session (`crates/memoryd/tests/coordination_integration.rs:54-71`). Production attaches presence only for level 3 or higher (`crates/memoryd/src/recall/delta.rs:128-132`).
- Production delta delivery still records peer activity audit entries only after a peer update renders (`crates/memoryd/tests/coordination_integration.rs:99-115`, `crates/memoryd/src/recall/delta.rs:82-90`, `crates/memoryd/src/recall/delta.rs:378-419`).

### Newly fixed attribution coverage is present

`peer_update_uses_actual_writer_attribution_in_delta_and_audit` writes a peer memory with `harness: "claude-code"` and `session_id: "sess_peer_writer"`, calls the daemon delta path, and asserts both the rendered `<peer-update>` and peer-activity audit use that actual writer identity rather than `codex` or the local device id (`crates/memoryd/tests/coordination_integration.rs:117-144`). Production now hydrates peer source identity from canonical memory frontmatter/source before building `PeerWriteCandidate` (`crates/memoryd/src/recall/delta.rs:237-299`), and delivery audit copies the rendered update's `harness` / `session_id` (`crates/memoryd/src/recall/delta.rs:378-397`; `crates/memoryd/src/handlers.rs:187-199`).

### Newly fixed cooldown coverage is present

`peer_update_cooldown_is_per_receiving_session_in_daemon_ram` calls delta three times against the same peer write: first delivery to `sess_current` renders, the second delivery to the same receiving session is suppressed, and a different receiving session still gets the peer update (`crates/memoryd/tests/coordination_integration.rs:146-166`). Production seeds `SessionContext.surfaced_peer_writes` from the daemon RAM cooldown store before evaluation and records rendered peer writes afterward (`crates/memoryd/src/recall/delta.rs:117-125`, `crates/memoryd/src/recall/delta.rs:410-419`), while `HandlerState` keys cooldown by harness, receiving session id, and namespaces (`crates/memoryd/src/handlers.rs:202-208`, `crates/memoryd/src/handlers.rs:361-392`).

### Presence zero-overlap filtering is partially covered

`level3_presence_requires_salient_entity_or_path_overlap` verifies the Level 3 daemon path now suppresses a same-namespace peer with unrelated salient entities while still rendering an entity-overlapping peer (`crates/memoryd/tests/coordination_integration.rs:73-96`). That closes the core zero-overlap leak class for entity-overlap fixtures, but the path-only branch remains untested per S1.

### Requested adjacent coverage still passes

- Renderer tests cover no-coordination shape, peer update rendering, Level 2 absence/Level 3 presence rendering when an insertion is supplied, privacy masking, claim-lock attribute rendering, budget accounting, startup peer-update rendering, and cross-device startup rendering (`crates/memoryd/tests/coordination_recall_render.rs:9-208`).
- Peer CLI tests cover parser exposure, status/activity rendering, release-lock no-lock and success behavior, and MCP rejection (`crates/memoryd/tests/peer_cli.rs:17-117`).
- Claim-lock supersede tests cover Level 1 skip, project mode overrides, contention warning/event, governance refusal preserving refusal status, invalid identity rejection, and rollback restoring the previous holder (`crates/memoryd/tests/claim_lock_supersede.rs:12-238`).

## Checks run

```text
cargo test -p memoryd --test coordination_integration --test coordination_recall_render --test peer_cli --test claim_lock_supersede
```

Result: passed.

- `claim_lock_supersede`: 10 passed.
- `coordination_integration`: 7 passed.
- `coordination_recall_render`: 14 passed.
- `peer_cli`: 8 passed.

## Remaining risk

This was a focused Gate D test-coverage rerun, not a full Stream I security/performance review. The worktree was already broadly dirty with many Stream G/H/I changes and untracked files; this review intentionally wrote only this artifact.
