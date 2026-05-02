# Stream I Review Gate C Security/Concurrency Review

**Date:** 2026-05-01  
**Scope:** Stream I Tasks 9-12 in `crates/memorum-coordination`: presence heartbeat/state, stale-session cleanup, claim-lock registry, protocol DTOs, and unit tests.  
**Verdict:** Changes requested

## Findings

### High — `ClaimLockRegistry::acquire_at` is check-then-insert and can miss same-memory double-acquire contention

**Files:** `crates/memorum-coordination/src/claim_lock.rs:153-171`, `crates/memorum-coordination/src/claim_lock.rs:246-249`  
**Related contract:** `docs/plans/2026-05-01-stream-i-cross-session.md:758-765`, `docs/plans/2026-05-01-stream-i-cross-session.md:794-803`, `docs/plans/2026-05-01-stream-i-cross-session.md:895-922`

`acquire_at` first calls `live_lock_at`, which clones any existing live entry under a short-lived DashMap read guard, then inserts a new entry later if no live entry was observed. Two handler tasks can therefore both observe no live lock for the same `memory_id`, both return `Acquired`, and the later `insert` silently overwrites the earlier lock. That loses the required contention warning/audit signal for one of the two simultaneous supersede attempts.

**Exploitability:** A local peer/harness can trigger this by issuing two concurrent `memory_supersede` calls for the same memory from different sessions. No special privileges are needed beyond access to the daemon protocol path that will call this registry.

**Impact:** The advisory lock stops providing reliable coordination under the exact contention case it is meant to surface. Downstream Task 13 would have no `ClaimLockContention` warning/event for one racing session, and the final visible holder can be whichever insert wins the race.

**Minimal remediation:** Make acquire atomic per `memory_id` with `DashMap::entry`/occupied-entry mutation under the shard lock. Re-check liveness while holding the entry guard, replace only expired entries, and return `Contended` for a live non-holder. Add a behavior test with two threads/tasks simultaneously acquiring the same empty `memory_id`; exactly one should be `Acquired`, the other should be `Contended` or the chosen contract-equivalent, never two `Acquired` results.

### Medium — Claim-lock ownership checks authorize by `session_id` only, not by the full holder identity

**Files:** `crates/memorum-coordination/src/claim_lock.rs:31-60`, `crates/memorum-coordination/src/claim_lock.rs:153-188`, `crates/memorum-coordination/src/claim_lock.rs:191-210`  
**Related contract:** `docs/specs/stream-i-cross-session-v0.1.md:485-490`, `docs/specs/stream-i-cross-session-v0.1.md:619-634`

`ClaimLockInfo` stores both `holder_harness` and `holder_session_id`, and contention payloads frame holders as `harness:session_id`. But `AlreadyHeld`, `renew`, `release`, and `release_all_held_by` compare only `session_id`. A session from a different harness with the same/spoofed `session_id` can be treated as the holder for renew/release purposes.

**Exploitability:** This is most plausible when session ids are short, user-controllable in tests/admin paths, or not globally unique across harnesses. Even if current harness ids are usually high entropy, the protocol type does not enforce that uniqueness.

**Impact:** A non-holder can renew or release another harness's lock if it can collide/spoof `session_id`. Stale-session cleanup can also release locks across harnesses if two active records share a session id.

**Minimal remediation:** Carry `harness` through `ClaimLockRenewRequest`, `release`, and stale-release identity, or introduce a single `PeerSessionId { harness, session_id }` key type. Compare both fields for holder checks. Add tests proving same `session_id` with different `harness` cannot renew, release, or be treated as `AlreadyHeld`.

### Medium — Claim-lock removal paths can delete a newly acquired live lock after an expired/released entry race

**Files:** `crates/memorum-coordination/src/claim_lock.rs:191-227`  
**Related contract:** `docs/specs/stream-i-cross-session-v0.1.md:598-605`

`release` validates ownership with `get`, drops the read guard, then removes by key without re-checking that the same holder/entry is still present. `sweep_expired_at` collects expired memory ids, then removes by key later without re-checking that the entry is still expired. Because `acquire_at` allows an expired lock to be replaced, these windows can remove a newly acquired live lock for the same `memory_id`.

One concrete interleaving: sweeper observes expired `mem_x` and queues it; before removal, session B acquires `mem_x` and inserts a fresh live lock; sweeper then removes by key and deletes B's lock.

**Exploitability:** This can occur naturally under heartbeat/sweeper timing and concurrent supersede attempts around TTL expiry.

**Impact:** Fresh locks can disappear, causing missed claim-lock visibility and stale-contention handling. This is liveness/coordination correctness rather than disk corruption, but it undermines the concurrency invariant Gate C is meant to validate.

**Minimal remediation:** Use `remove_if`/entry APIs with predicate re-checks. For expiry, remove only if the current entry is still expired at the sweep instant. For release, remove only if the current entry is still held by the releasing holder identity.

### Medium — Heartbeat/active-peer payloads expose and store more peer metadata than the speced presence surface, with incomplete bounds

**Files:** `crates/memorum-coordination/src/protocol.rs:21-44`, `crates/memorum-coordination/src/presence.rs:215-248`, `crates/memorum-coordination/src/presence.rs:264-290`, `crates/memorum-coordination/src/presence.rs:293-323`, `crates/memorum-coordination/src/presence.rs:336-377`  
**Related contract:** `docs/specs/stream-i-cross-session-v0.1.md:368-390`, `docs/specs/stream-i-cross-session-v0.1.md:451-498`

The speced `<peer-presence>` surface is intentionally small: harness, truncated id, bounded entities, and started time. The heartbeat ack in code returns `active_peers`, and each `ActivePeer` includes full `session_id`, `device_id`, full `project_binding` including cwd, full `salient_paths`, `capabilities`, and `claim_locks_held`. `capabilities` has no count or per-entry byte bound, and `claim_locks_held` has only a count bound, not per-id length/pattern validation or daemon-registry cross-check.

**Exploitability:** Any Level 3 heartbeat caller in a namespace can receive the full peer records for other live sessions in that namespace. A malformed heartbeat can also store oversized capability strings or lock ids in daemon RAM and echo them to peers.

**Impact:** This leaks more local metadata than the Stream I presence XML contract requires, including absolute cwd/project-binding details and full path lists. The unbounded fields are also a small local DoS/amplification vector.

**Minimal remediation:** Keep rich presence records internal to the daemon; return only the speced/capped peer-presence projection, or remove `active_peers` from the ack if not contractually needed. Add explicit bounds for `capabilities` and per-entry bounds/pattern checks for `claim_locks_held`. When claim-lock renewal is wired, derive/echo only locks the daemon registry recognizes rather than trusting heartbeat-supplied lock ids.

## Positive checks

- Presence stale cleanup uses `remove_if` with a stale re-check before deletion, which avoids the stale-peer cleanup race that the claim-lock removal paths still have (`crates/memorum-coordination/src/presence.rs:134-149`).
- Stale presence cleanup calls the claim-lock releaser after removing stale sessions, matching the spec intent to release locks for stale sessions (`crates/memorum-coordination/src/presence.rs:152-166`; `docs/specs/stream-i-cross-session-v0.1.md:533-548`).
- Presence peer lookup filters exact namespace, excludes the current session id, and filters stale records (`crates/memorum-coordination/src/presence.rs:120-128`).
- `started_at` retention is implemented and tested: the first non-`None` value is retained across later heartbeats (`crates/memorum-coordination/src/presence.rs:95-105`; `crates/memorum-coordination/tests/presence_unit.rs:198-218`, `crates/memorum-coordination/tests/presence_unit.rs:294-340`).
- Existing claim-lock tests cover sequential acquisition, contention, renew, release, TTL expiry, and stale-session release (`crates/memorum-coordination/tests/claim_lock_unit.rs:21-258`). The missing coverage is concurrent acquire/remove interleavings.
- No direct filesystem, process-command, event-log, or governance writes were found in `crates/memorum-coordination/src` during the targeted no-governance scan. The crate is RAM-only at this layer.

## Validations run

```bash
cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit
```

Result: passed — 11 `claim_lock_unit` tests and 19 `presence_unit` tests.

```bash
cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings
```

Result: passed.

```bash
cargo fmt --package memorum-coordination -- --check
```

Result: passed.

```bash
rg -n "std::fs|tokio::fs|File::|OpenOptions|create_dir|write\(|remove_file|rename\(|canonical|governance|EventKind|events_log|memory_substrate::|memory_privacy::|unsafe|Command::|process::Command" crates/memorum-coordination
```

Result: no direct disk/governance/process execution paths in coordination code; only type imports from `memory_substrate` and the crate-level `unsafe_op_in_unsafe_fn` lint appeared.

```bash
cargo tree -p memorum-coordination -e normal
```

Result: normal dependency tree inspected. Note: `memory-privacy` is a normal dependency in `crates/memorum-coordination/Cargo.toml:8-14` but is not used by the Task 9-12 source inspected here; this is not a Gate C blocker if it is intentionally staged for Task 14 XML/privacy rendering, but it should be removed if Task 14 does not consume it.

## Residual risk and confidence

Residual risk is concentrated in integration not yet in this scope: Task 13 `handle_supersede` must preserve the `effective_level >= 2` gate, emit `ClaimLockContention`, and release locks after successful writes without changing Stream C governance behavior. This review did not validate memoryd handler wiring because the requested scope was Tasks 9-12 and `memorum-coordination`.

Confidence: high for the claim-lock registry findings and presence payload/bounds findings, based on direct line-level inspection and passing targeted gates. Confidence is medium for exploit likelihood because daemon protocol trust boundaries depend on the later memoryd wiring and harness authentication assumptions.
