# Stream I Review Gate C Full Security/Concurrency Rerun

**Date:** 2026-05-02  
**Scope:** Read-only rerun after memoryd heartbeat/stale cleanup fixes. Reviewed the prior full Gate C security and clean-code reviews, the earlier Gate C security rerun, the current `memorum-coordination` presence/claim-lock implementation, and current `memoryd` heartbeat, stale-cleanup, claim-lock supersede, and governance test coverage.  
**Verdict:** Approve

## Findings by severity

### High / S1

None.

### Medium / S2

None.

### Low / S3

None blocking for Gate C. Residual risks are listed below.

## Prior High/Medium finding closure

### Closed — Task 10/11 heartbeat and stale cleanup are integrated into `memoryd`

The prior full security and full clean-code reviews found that heartbeat and stale-cleanup behavior existed only in `memorum-coordination`, not in the daemon path. That finding is closed in the reviewed tree:

- `memoryd::protocol` now re-exports the coordination heartbeat DTOs and exposes `RequestPayload::PeerHeartbeat` plus `ResponsePayload::PeerHeartbeat` (`crates/memoryd/src/protocol.rs:10`, `crates/memoryd/src/protocol.rs:45-94`, `crates/memoryd/src/protocol.rs:190-202`).
- `HandlerState` owns `PresenceRegistry` and `ClaimLockRegistry` arcs, exposes registry accessors, and carries coordination defaults (`crates/memoryd/src/handlers.rs:80-140`).
- `dispatch` now routes `PeerHeartbeat` into `peer_heartbeat_response`, which calls `memorum_coordination::handle_peer_heartbeat` with claim-lock renewal wired to the authoritative registry (`crates/memoryd/src/handlers.rs:182-216`, `crates/memoryd/src/handlers.rs:231-252`).
- Daemon substrate startup now spawns coordination cleanup, and the exported helper passes the daemon-owned presence and claim-lock registries into `spawn_stale_session_cleanup_task` (`crates/memoryd/src/server.rs:53-60`, `crates/memoryd/src/server.rs:78-90`, `crates/memoryd/src/server.rs:99-109`).
- Daemon-level tests now cover heartbeat serde, Level 3 presence, `started_at` retention, Level 1/2 no-op presence behavior, validation failures, stale cleanup, lock release, and non-blocking reads (`crates/memoryd/tests/heartbeat_protocol.rs:8-157`, `crates/memoryd/tests/stale_session_cleanup.rs:10-77`).

### Closed — claim-lock warning/event identity is validated before event persistence

The prior full security review found that `memory_supersede` could move unbounded or secret-like `meta.session_id` / `meta.harness` into claim-lock warnings and `ClaimLockContention` events. That finding is closed:

- `GovernanceWriteInput::parse` validates `session_id` and `harness` before privacy classification, governance, claim-lock acquisition, warning serialization, or event append (`crates/memoryd/src/handlers.rs:2320-2333`).
- `validated_claim_lock_identity_field` trims, rejects empty values, enforces the 128-byte bound, restricts to safe id characters, runs the existing safe-plaintext/canary checks, and rejects secret/PII marker patterns (`crates/memoryd/src/handlers.rs:1057-1072`).
- Regression tests prove AWS-key-like session ids are rejected before any contention event is written and oversized harness values are rejected (`crates/memoryd/tests/claim_lock_supersede.rs:126-168`).

### Closed — post-acquire failure paths no longer leave stale/wrong claim locks or hard-fail advisory contention event append

The prior full security review found that post-acquire errors could leave stale contender locks and that contention event append failure could turn advisory contention into a hard failure. That finding is closed for the reviewed paths:

- Contention event append is now warning-only: `record_event_best_effort` failure is logged and the supersede proceeds with the advisory warning (`crates/memoryd/src/handlers.rs:1364-1405`).
- `SupersedeClaimLock` now acts as an RAII guard. On success it releases the current holder lock and returns the warning; on drop before success it releases an acquired contender lock or restores the previous holder after contention (`crates/memoryd/src/handlers.rs:1407-1520`).
- `ClaimLockRegistry::restore` only restores a previous holder if the old lock has remaining TTL and the current entry is vacant, expired, or already the same holder, so rollback does not overwrite an unrelated live holder (`crates/memorum-coordination/src/claim_lock.rs:232-267`).
- Regression tests cover restore-after-failed-contention, not replacing unrelated live holders, and post-acquire supersede write failure restoring the previous holder (`crates/memorum-coordination/tests/claim_lock_unit.rs:220-280`, `crates/memoryd/tests/claim_lock_supersede.rs:170-195`).

### Still closed — prior coordination-crate race and metadata findings

The earlier Gate C security and clean-code findings remain closed:

- Atomic acquire uses `DashMap::entry` so the liveness check and insert/replace happen under the occupied/vacant entry guard (`crates/memorum-coordination/src/claim_lock.rs:163-197`).
- Holder checks use full `harness + session_id` identity for renew, release, and release-all paths (`crates/memorum-coordination/src/claim_lock.rs:128-130`, `crates/memorum-coordination/src/claim_lock.rs:204-229`).
- Release, restore, and sweep paths re-check the current entry before deletion/replacement (`crates/memorum-coordination/src/claim_lock.rs:217-267`, `crates/memorum-coordination/src/claim_lock.rs:274-288`).
- Presence stale cleanup removes only records still stale at removal time and releases claim locks only for records actually removed (`crates/memorum-coordination/src/presence.rs:151-184`).
- Heartbeats renew recognized held claim locks through the daemon-wired authoritative registry (`crates/memorum-coordination/src/presence.rs:234-269`, `crates/memorum-coordination/src/presence.rs:343-356`, `crates/memoryd/src/handlers.rs:231-252`).
- Active peer acknowledgements remain bounded to the small projection rather than rich peer metadata (`crates/memorum-coordination/src/protocol.rs:23-38`, `crates/memorum-coordination/src/presence.rs:332-340`).

## Additional security/concurrency checks

- `memorum-coordination` remains RAM-only for reviewed presence and claim-lock paths; the daemon is responsible for the only reviewed contention event persistence path.
- Heartbeat validation rejects empty IDs and entity overflow at the daemon request boundary, and Level 1/2 heartbeats acknowledge without mutating presence state.
- Successful supersede still runs Stream C governance before claim-lock acquire, and the governance refusal path does not emit or return claim-lock warnings.
- Level 1 supersede skips claim-lock acquisition; Level 2 supersede acquires and releases on success.
- Contention warnings/events retain advisory semantics: they warn and proceed rather than refusing writes.

## Validations run

```bash
cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit
```

Result: passed — 17 `claim_lock_unit` tests and 23 `presence_unit` tests.

```bash
cargo test -p memoryd --test claim_lock_supersede --test heartbeat_protocol --test stale_session_cleanup --test governance_e2e --test governance_matrix_e2e
```

Result: passed — 8 `claim_lock_supersede` tests, 7 `heartbeat_protocol` tests, 2 `stale_session_cleanup` tests, 9 `governance_e2e` tests, and 3 `governance_matrix_e2e` tests.

```bash
cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings
```

Result: passed.

## Residual risk and confidence

Residual risk is low for Gate C security/concurrency after this rerun. I did not review later Stream I recall rendering, peer CLI/status/activity surfaces, or performance/doc tasks outside the requested heartbeat, stale cleanup, claim-lock, and governance scope. I also did not perform destructive fault injection beyond the existing post-acquire filesystem-failure regression test.

The worktree contains broad pre-existing uncommitted Stream G/H/I changes; this review reflects the current dirty tree and only adds this review artifact.

Confidence: high. The prior High/Medium findings are closed by direct code inspection and by the requested daemon-level and coordination-level tests passing with clippy clean.
