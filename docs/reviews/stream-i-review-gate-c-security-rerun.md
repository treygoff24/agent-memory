# Stream I Review Gate C Security/Concurrency Rerun

**Date:** 2026-05-02  
**Scope:** Read-only rerun for Stream I Gate C fixes in `crates/memorum-coordination`, focused on Tasks 9-12: presence heartbeat/state, stale-session cleanup, claim-lock registry, protocol DTOs, and unit tests. This rerun reviewed the prior Gate C security and clean-code findings and the current implementation in `presence.rs`, `claim_lock.rs`, `protocol.rs`, and related tests.  
**Verdict:** Approve for the requested Tasks 9-12 `memorum-coordination` scope.

## Findings by severity

### High / S1

None.

### Medium / S2

None.

### Low / S3

None blocking for this gate. Residual integration risks are listed below.

## Prior High/Medium finding closure

### Closed — atomic claim-lock acquire prevents same-memory double-acquire races

The prior High finding on `ClaimLockRegistry::acquire_at` check-then-insert races is closed. `acquire_at` now uses `DashMap::entry` and performs liveness checks, replacement, and contention result construction while holding the occupied/vacant entry guard (`crates/memorum-coordination/src/claim_lock.rs:163-197`). A concurrent first-acquire behavior test asserts exactly one `Acquired` and one `Contended` result for simultaneous acquisition of an empty `memory_id` (`crates/memorum-coordination/tests/claim_lock_unit.rs:73-95`).

### Closed — claim-lock holder checks use full harness + session identity

The prior Medium finding on session-id-only authorization is closed. `ClaimLockEntry::is_held_by` compares both `holder_harness` and `holder_session_id` (`crates/memorum-coordination/src/claim_lock.rs:128-130`), renew checks both fields before extending TTL (`crates/memorum-coordination/src/claim_lock.rs:204-214`), release uses the same full-identity predicate (`crates/memorum-coordination/src/claim_lock.rs:217-229`), and stale-session cleanup passes the removed record's harness and session id into the releaser (`crates/memorum-coordination/src/presence.rs:171-184`). The regression test covers same `session_id` with different `harness` for renew, release, and release-all paths (`crates/memorum-coordination/tests/claim_lock_unit.rs:177-196`).

### Closed — removal paths re-check the current entry before deletion

The prior Medium finding on release/sweep deleting newly acquired live locks is closed. `release` uses `remove_if` with a current-entry holder predicate (`crates/memorum-coordination/src/claim_lock.rs:217-219`), and `sweep_expired_at` collects candidate keys but removes only if the current entry is still expired at the sweep instant (`crates/memorum-coordination/src/claim_lock.rs:236-249`). Tests cover old-holder release not removing a new contender and expired-sweep not removing a reacquired live lock (`crates/memorum-coordination/tests/claim_lock_unit.rs:198-236`).

### Closed — heartbeat ack no longer exposes rich peer metadata and bounded fields were added

The prior Medium finding on heartbeat/active-peer metadata exposure and incomplete bounds is closed for the reviewed scope. The public `ActivePeer` DTO now contains only truncated `session_id`, `harness`, capped `salient_entities`, and `started_at` (`crates/memorum-coordination/src/protocol.rs:31-38`; `crates/memorum-coordination/src/presence.rs:332-340`). It no longer serializes `device_id`, `project_binding`/cwd, salient paths, capabilities, or heartbeat-supplied lock ids; the regression test asserts those fields are absent from the active peer JSON (`crates/memorum-coordination/tests/presence_unit.rs:261-300`). Capabilities and claim-lock ids are now explicitly bounded and claim-lock ids are pattern-validated (`crates/memorum-coordination/src/presence.rs:16-25`, `crates/memorum-coordination/src/presence.rs:290-299`, `crates/memorum-coordination/src/presence.rs:405-420`), with tests for capability overflow and invalid claim-lock ids (`crates/memorum-coordination/tests/presence_unit.rs:447-470`).

### Closed — heartbeat claim-lock renewal is wired to the authoritative registry

The prior clean-code/concurrency Medium finding on `claim_locks_held` being stored but not renewed is closed. `PeerHeartbeatOptions` now accepts optional `ClaimLockHeartbeatRenewal` inputs (`crates/memorum-coordination/src/presence.rs:65-79`), Level 3 heartbeat handling calls renewal before upserting presence (`crates/memorum-coordination/src/presence.rs:234-245`), and renewal calls `ClaimLockRegistry::renew_at` using the heartbeat holder's harness/session identity (`crates/memorum-coordination/src/presence.rs:343-356`). The test proves a recognized lock is renewed from heartbeat time and an unrecognized lock id is ignored (`crates/memorum-coordination/tests/presence_unit.rs:327-360`).

### Closed — contended acquire remains advisory and records the contender as current holder

The prior clean-code/concurrency Medium finding on contended acquire returning a warning without creating/refreshing the contender's advisory lock is closed. On a live non-holder entry, `acquire_at` captures the previous holder, inserts the contender's new entry, and returns `Contended` with the previous holder as warning metadata (`crates/memorum-coordination/src/claim_lock.rs:166-183`). The contention test asserts the active registry holder becomes the contender after the warning result (`crates/memorum-coordination/tests/claim_lock_unit.rs:36-71`).

## Positive security/concurrency checks

- Presence state remains RAM-only in the reviewed crate: no direct filesystem, process execution, event-log, or governance writes were found in the targeted scan of `crates/memorum-coordination`.
- Presence stale cleanup uses a collect-then-`remove_if` pattern and releases claim locks only for records actually removed as stale (`crates/memorum-coordination/src/presence.rs:151-184`).
- The cleanup task uses Tokio interval scheduling, `MissedTickBehavior::Delay`, and a watch-channel shutdown path without blocking the heartbeat handler (`crates/memorum-coordination/src/presence.rs:187-232`), with tests for interval behavior, clean shutdown, and handler non-blocking (`crates/memorum-coordination/tests/presence_unit.rs:77-151`).
- Heartbeats at Level 1/2 are acknowledged but do not update presence, preserving the Level 3 presence boundary (`crates/memorum-coordination/src/presence.rs:240-268`; `crates/memorum-coordination/tests/presence_unit.rs:473-495`).
- Claim-lock TTL and stale-session release behavior are covered by behavior tests rather than DashMap internals (`crates/memorum-coordination/tests/claim_lock_unit.rs:97-348`).

## Validations run

```bash
cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit
```

Result: passed — 15 `claim_lock_unit` tests and 23 `presence_unit` tests.

```bash
cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings
```

Result: passed.

```bash
cargo fmt --package memorum-coordination -- --check
```

Result: passed.

```bash
cargo test -p memorum-coordination
```

Result: passed — unit target, `claim_lock_unit`, `gate_unit`, `presence_unit`, `session_derivation`, and doctests.

```bash
rg -n "std::fs|tokio::fs|File::|OpenOptions|create_dir|write\(|remove_file|rename\(|canonical|governance|EventKind|events_log|memory_substrate::|memory_privacy::|unsafe|Command::|process::Command|secret|token|password|api[_-]?key" crates/memorum-coordination/src crates/memorum-coordination/tests
```

Result: no direct disk, governance, process execution, unsafe block, or secret-handling path found in the reviewed coordination code. Matches were limited to `memory_substrate` type imports/usages in gate/session code and tests plus the crate-level `unsafe_op_in_unsafe_fn` lint.

## Residual risk and confidence

Residual risk is primarily outside this requested rerun scope. The plan's full Gate C also references Task 13 `memoryd` wiring for `effective_level >= 2`, `handle_supersede`, `ClaimLockContention` event emission, and governance-preserving release behavior; this rerun did not inspect or run the `memoryd` Gate C tests because the request scoped the rerun to Tasks 9-12 and `memorum-coordination`.

Confidence: high for the reviewed Tasks 9-12 security/concurrency closure. The prior High/Medium findings are closed in the current code and backed by targeted behavior tests plus passing clippy/fmt gates.
