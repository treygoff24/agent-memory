# Stream I Review Gate C — Clean Code / Concurrency Review Rerun

### Verdict

Approve

### Intended outcome

Stream I Gate C is intended to validate the RAM-only presence and advisory claim-lock substrate for Tasks 9-12 after fixes: Level 3 heartbeats should maintain presence and renew held claim locks, claim-lock acquire should remain advisory while surfacing contention, ownership must be scoped to harness plus session id, cleanup/release paths must be race-safe, and heartbeat/active-peer payloads must be bounded and privacy-conscious. This rerun is read-only except for this review file and focuses on `crates/memorum-coordination` plus the prior Gate C clean-code/security findings.

### Executive summary

The previous Gate C findings are closed in the coordination crate. `ClaimLockRegistry::acquire_at` now uses DashMap entry mutation for atomic per-memory acquire semantics, contended acquisition replaces the visible holder while returning warning metadata about the previous holder, renew/release/stale cleanup compare full `harness + session_id` holder identity, and release/sweep removal paths use predicate re-checks so they do not remove newly acquired live locks. Heartbeat claim-lock renewal is wired through `PeerHeartbeatOptions::claim_lock_renewal` and covered by a behavior test, while active-peer acknowledgements now expose only the small presence projection and heartbeat inputs are bounded. No material issues found.

### Findings

No material issues found.

### Closure checks from prior Gate C findings

- Heartbeat `claim_locks_held` renewal is wired/tested.
  - Evidence: `crates/memorum-coordination/src/presence.rs:65-79` adds `ClaimLockHeartbeatRenewal` to heartbeat options; `crates/memorum-coordination/src/presence.rs:242-244` calls renewal before presence upsert for active Level 3 heartbeats; `crates/memorum-coordination/src/presence.rs:343-356` renews each listed lock via `ClaimLockRegistry::renew_at`; `crates/memorum-coordination/tests/presence_unit.rs:327-360` proves a Level 3 heartbeat extends a recognized lock from heartbeat time and ignores an unrecognized lock id.
  - Confidence: High

- Contended acquire semantics insert/update the contender with warning metadata.
  - Evidence: `crates/memorum-coordination/src/claim_lock.rs:166-183` performs occupied-entry mutation under the DashMap shard lock, captures the previous holder info, inserts the contender's new lock, and returns `ClaimLockAcquireResult::Contended`; `crates/memorum-coordination/tests/claim_lock_unit.rs:37-70` asserts the contention result includes holder and contender metadata and the active registry holder becomes the contender.
  - Confidence: High

- Atomic acquire no longer has a check-then-insert race.
  - Evidence: `crates/memorum-coordination/src/claim_lock.rs:163-197` uses `DashMap::entry` instead of a separate `get`/`insert` sequence; `crates/memorum-coordination/tests/claim_lock_unit.rs:73-95` starts two simultaneous empty-memory acquires and asserts exactly one `Acquired` plus exactly one `Contended` result.
  - Confidence: High

- Owner identity uses harness plus session id.
  - Evidence: `crates/memorum-coordination/src/claim_lock.rs:116-118` defines holder equality over both fields; `crates/memorum-coordination/src/claim_lock.rs:204-214`, `217-229` apply that identity to renew, release, and release-all; `crates/memorum-coordination/tests/claim_lock_unit.rs:177-195` proves the same `session_id` under a different harness cannot renew, release, or stale-release another harness's lock.
  - Confidence: High

- Release/sweep cannot remove newly acquired live locks.
  - Evidence: `crates/memorum-coordination/src/claim_lock.rs:217-218` uses `remove_if` with a current-holder predicate for release; `crates/memorum-coordination/src/claim_lock.rs:236-249` uses `remove_if` with a current-expired predicate for sweep; `crates/memorum-coordination/tests/claim_lock_unit.rs:198-235` covers release-after-contender and expired-sweep-after-reacquire cases.
  - Confidence: High

- Active peer payload and heartbeat bounds are constrained.
  - Evidence: `crates/memorum-coordination/src/protocol.rs:31-38` limits public `ActivePeer` to `session_id`, `harness`, `salient_entities`, and `started_at`; `crates/memorum-coordination/src/presence.rs:332-339` truncates session id and caps peer entities; `crates/memorum-coordination/src/presence.rs:16-27`, `290-299`, and `383-420` bound session/harness/entity/path/capability/claim-lock fields, including count and per-id validation for `claim_locks_held`; `crates/memorum-coordination/tests/presence_unit.rs:261-299`, `302-324`, and `421-470` verify projection and validation behavior.
  - Confidence: High

### Non-blocking simplifications

- Consider exporting `ClaimLockHeartbeatRenewal` from `crates/memorum-coordination/src/lib.rs` alongside `PeerHeartbeatOptions` if memoryd callers are expected to build renewal options through the crate root. This is not a blocker because the type is already public through `memorum_coordination::presence::ClaimLockHeartbeatRenewal` and tests use that path.

### Test gaps

- The requested coordination-crate tests cover the prior findings. Full Gate C in the plan also names Task 13 memoryd handler wiring (`heartbeat_protocol`, `stale_session_cleanup`, `claim_lock_supersede`, and the `effective_level >= 2` supersede gate), but this rerun was explicitly scoped by the prompt to `memorum-coordination` Tasks 9-12 and the requested commands did not include memoryd tests.

### Questions / uncertainties

- `docs/plans/2026-05-01-stream-i-cross-session.md` defines Review Gate C as blocked by Tasks 9-13 and includes memoryd review commands, while this rerun request names Tasks 9-12 and only asks for `memorum-coordination` gates. I treated Task 13/memoryd wiring as residual integration risk rather than a blocker for this rerun.
- The worktree contains substantial pre-existing uncommitted Stream G/H/I changes outside this review file; I did not inspect or modify those unrelated surfaces.

### Validations

- `cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit` — passed: 15 `claim_lock_unit` tests and 23 `presence_unit` tests.
- `cargo test -p memorum-coordination` — passed: crate unit tests, `claim_lock_unit`, `gate_unit`, `presence_unit`, `session_derivation`, and doc-tests.
- `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-coordination -- --check` — passed.

### Positives

- The fix keeps concurrency control local and simple: DashMap entry APIs and `remove_if` predicates express the intended atomicity without introducing broader locks or async blocking.
- The tests are behavior-first and target the actual prior failure modes rather than implementation details alone.
- The public active-peer projection is materially safer and cleaner than the earlier rich-record echo while retaining the presence information needed for coordination.
