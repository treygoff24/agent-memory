# Stream I Review Gate C — Clean Code / Concurrency Review

### Verdict

Changes requested

### Intended outcome

Stream I Tasks 9-12 appear intended to land the RAM-only coordination substrate for presence and advisory claim locks: monotonic `PresenceRegistry` state, Level 3 heartbeat ingestion with validation, non-blocking stale-session cleanup, and a `ClaimLockRegistry` that supports acquire/renew/release/contention without adding persistence or depending on Stream C governance. The reviewed scope is `crates/memorum-coordination` plus the Stream I plan/spec contracts for presence and claim locks.

### Executive summary

The implementation is cleanly factored, uses `Instant` for stale/TTL decisions, keeps state in process RAM, avoids governance coupling, and passes the requested `memorum-coordination` gates. I am not comfortable approving Gate C yet because the claim-lock lifecycle is incomplete at the contract boundary: heartbeat `claim_locks_held` is validated/stored but never renews recognized locks, and contended acquire returns a warning without creating/refreshing a lock for the contending operation despite the plan's Task 12 implementation contract saying acquire always inserts. These are behavioral contract gaps rather than style issues; they can make Level 3 renewal ineffective and leave stale/misleading lock state after an advisory contended supersede proceeds.

### Findings

[Medium] [Correctness] Heartbeat `claim_locks_held` does not renew locks

- Evidence: `docs/specs/stream-i-cross-session-v0.1.md:587-596` says heartbeat renewal is driven by `claim_locks_held`; `docs/plans/2026-05-01-stream-i-cross-session.md:658-659` defers claim-lock renewal from Task 10 to Task 12. In `crates/memorum-coordination/src/presence.rs:215-248`, `handle_peer_heartbeat` validates and stores heartbeat data but has no `ClaimLockRegistry`/TTL input, performs no `renew`, and always returns `conflicting_claim_locks: Vec::new()`.
- Why it matters: Level 3 sessions can believe they are renewing active work through heartbeats, but locks will still expire on the original TTL. A long-running supersede/review workflow can silently lose its advisory lock while the session remains present and healthy.
- Reasoning: `ValidatedHeartbeat` preserves `claim_locks_held` into `PresenceRecord` (`presence.rs:251-306`), but storing the list in presence is not equivalent to renewing the authoritative lock registry. Since `ClaimLockRegistry::renew_at` exists and enforces holder/session/expiry semantics, the missing connection is observable behavior, not just missing plumbing.
- Recommendation: Extend the heartbeat handling boundary, or the memoryd wrapper around it, to accept a claim-lock registry and claim-lock TTL, call `renew` for recognized `claim_locks_held`, ignore unrecognized/expired locks per spec, and add a test that a Level 3 heartbeat extends the lock expiry from heartbeat time.
- Confidence: High

[Medium] [API Contract] Contended acquire warns but does not create or refresh the contender's advisory lock

- Evidence: `docs/plans/2026-05-01-stream-i-cross-session.md:765` says acquire succeeds regardless of existing locks and `docs/plans/2026-05-01-stream-i-cross-session.md:796-799` says Task 12 `acquire` "always inserts" and returns contention when a previous entry exists. In `crates/memorum-coordination/src/claim_lock.rs:153-170`, `acquire_at` returns `ClaimLockAcquireResult::Contended(...)` immediately when another live holder exists and only inserts at lines 168-170 for the no-live-lock path. The unit test codifies the current behavior by asserting the registry still reports `sess_a` after `sess_b` contends (`crates/memorum-coordination/tests/claim_lock_unit.rs:48-67`).
- Why it matters: Advisory contention is supposed to preserve liveness: the contending supersede proceeds, then the caller's completion path can release its lock and the system should reflect the currently active holder/workflow. With the current registry behavior, the contender never owns a lock, cannot renew it, and a post-write `release(memory_id, sess_b)` is a no-op while the old holder remains visible until TTL/stale cleanup.
- Reasoning: The implementation satisfies "warn, not hard-refuse" at the return-value level, but it diverges from the explicit Task 12 registry contract and creates misleading state for follow-on recall/status surfaces. If the intended design is "first holder remains authoritative until release/expiry," the plan/spec and tests should say that explicitly; otherwise the registry should insert/replace with the contender while returning the previous holder as contention metadata.
- Recommendation: Decide and encode the intended contention ownership semantics. If following the current plan, change `acquire_at` to insert/update the contender's lock while returning previous-holder warning metadata, and update tests to assert the registry holder becomes the contender. If first-holder-wins is intended, update the Task 12 contract and add daemon-level tests proving contended supersede completion does not leave misleading stale lock visibility.
- Confidence: Medium

### Non-blocking simplifications

- Consider centralizing heartbeat limits in a small `HeartbeatLimits` value if memoryd needs to share the same validation in another layer. The current constants are clear enough for this crate, so this is not blocking.

### Test gaps

- Missing behavior test that a Level 3 heartbeat with `claim_locks_held` renews the corresponding `ClaimLockRegistry` entry from heartbeat time and ignores unrecognized/expired lock ids.
- Missing contention lifecycle test that covers the full intended sequence: A acquires, B contends and proceeds, B's completion/release behavior leaves the registry in the expected state.
- Missing test for `conflicting_claim_locks` being populated, or an explicit contract update stating this field is deferred and should remain empty for Tasks 9-12.

### Validations

- `cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit` — passed (19 presence tests, 11 claim-lock tests).
- `cargo test -p memorum-coordination` — passed (claim_lock_unit, gate_unit, presence_unit, session_derivation, doc-tests).
- `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-coordination -- --check` — passed.

### Questions / uncertainties

- The user request says after Tasks 9-12, while the plan's Gate C is blocked by Tasks 9-13. I reviewed the requested Tasks 9-12/package scope and did not run the memoryd Gate C commands for heartbeat protocol, stale cleanup, or supersede wiring.
- There is a spec/plan ambiguity on contention ownership: the spec emphasizes "warn but allow" and the plan explicitly says registry `acquire` always inserts. The code follows a first-holder-wins registry model while still warning the contender.

### Positives

- Presence is RAM-only and monotonic: `PresenceRecord::last_heartbeat_at` uses `Instant`, cleanup uses `cleanup_stale_at`, and no file or governance dependency exists in the coordination crate.
- Stale cleanup avoids DashMap iterator mutation hazards by collecting session ids first and using `remove_if` before releasing claim locks.
- The code is generally small and readable, with deterministic `*_at` methods that make stale and TTL behavior straightforward to test.
