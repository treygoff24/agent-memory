# Stream I Review Gate C — Full Clean Code Review

### Verdict

Changes requested

### Intended outcome

Stream I Review Gate C is intended to validate Tasks 9-13 as an integrated, RAM-only cross-session coordination slice: Level 3 harnesses can heartbeat into memoryd, presence stays bounded and stale sessions are cleaned up without blocking the daemon, claim locks remain advisory and concurrency-safe, Level 2+ supersede acquires/releases locks and emits contention warnings/events, and existing Stream C governance behavior remains intact. I treated the plan/spec as the active contract and focused on the specified presence, claim-lock, `memoryd` protocol/handler, and supersede paths.

### Executive summary

The underlying `memorum-coordination` presence and claim-lock registries are substantially cleaner after the prior Gate C reruns: DashMap mutation is atomic enough for the reviewed cases, holder identity includes harness plus session id, heartbeat renewal exists in the crate, stale cleanup uses predicate re-checks, and the Task 13 supersede path preserves governance while warning/logging contention. The requested tests, clippy, and fmt gates all pass. However, the full Gate C contract is not actually wired through memoryd for presence: `RequestPayload`/`ResponsePayload` do not expose `PeerHeartbeat`, `dispatch` has no heartbeat handler, `HandlerState` has no `PresenceRegistry`, and the daemon/server/workers do not spawn the stale-session cleanup task. As a result, Level 3 presence and heartbeat-driven claim-lock renewal cannot work through the daemon, so this should not advance as a full Tasks 9-13 Gate C approval yet.

### Findings

[High] [API Contract] Level 3 heartbeat and stale-presence cleanup are implemented only in the coordination crate, not in memoryd

- Evidence: `docs/plans/2026-05-01-stream-i-cross-session.md` Task 10 requires `crates/memoryd/src/protocol.rs` to add `RequestPayload::PeerHeartbeat` and `ResponsePayload::PeerHeartbeat(PeerHeartbeatAck)`, and Task 11 requires memoryd server/worker cleanup wiring. In the current code, `crates/memoryd/src/protocol.rs:45-115` has no `PeerHeartbeat` request variant, and `crates/memoryd/src/protocol.rs:184-203` has no heartbeat response variant. `crates/memoryd/src/handlers.rs:155-190` has no dispatch arm for peer heartbeat, and `HandlerState` stores claim locks but no presence registry (`crates/memoryd/src/handlers.rs:73-95`). The reusable primitives exist in `crates/memorum-coordination/src/presence.rs:187-245` and renewal exists at `crates/memorum-coordination/src/presence.rs:343-356`, but `crates/memoryd/src/server.rs:52-87` only spawns notification/reality-check tasks, and `crates/memoryd/src/workers.rs:7-13` has no presence/stale cleanup worker. There are also no `crates/memoryd/tests/heartbeat_protocol.rs` or `crates/memoryd/tests/stale_session_cleanup.rs` files in the current tree.
- Why it matters: Level 3 presence is a daemon protocol feature, not just a library data structure. A real harness sending the speced heartbeat cannot deserialize through memoryd, so no session becomes present, held claim locks are not renewed by heartbeats, active-peer counts are unavailable, and stale sessions/locks are not cleaned up by the running daemon. This misses the central business outcome of Gate C: cross-session coordination must work in the daemon path that agents actually use.
- Reasoning: The coordination crate tests prove isolated behavior if an in-process caller directly invokes `handle_peer_heartbeat` and `spawn_stale_session_cleanup_task`. But memoryd is the trust and integration boundary for Tier 1 harnesses. Since `RequestPayload` lacks a heartbeat variant, the socket protocol cannot represent the request; since `dispatch` lacks a handler and `HandlerState` lacks presence state, there is nowhere for memoryd to store or renew presence even if the DTO existed; since server/worker startup never calls the cleanup spawner, stale cleanup will never run in production. The requested supersede tests cover claim-lock acquisition/release on `memory_supersede`, but they do not exercise heartbeat renewal or stale presence integration.
- Recommendation: Complete Task 10/11 in memoryd before rerunning this full gate: add heartbeat request/response DTOs or re-export the coordination DTOs through `memoryd::protocol`, add a `HandlerState`-owned `PresenceRegistry`, dispatch `PeerHeartbeat` to the coordination handler with claim-lock renewal options, spawn the stale-session cleanup task from daemon startup using the same shutdown signal, and add the planned `heartbeat_protocol` and `stale_session_cleanup` integration tests. Then include those tests in the Gate C command set, not only the coordination-crate unit tests.
- Confidence: High

### Non-blocking simplifications

- Once memoryd heartbeat wiring is added, consider keeping the crate-level DTOs and memoryd protocol DTOs as one shared type path rather than copying structurally identical request/ack structs. That would reduce drift between the library behavior tests and the daemon protocol contract.

### Test gaps

- `memoryd` has no heartbeat protocol integration tests verifying serde roundtrip, validation, Level 3 presence upsert, first non-`None` `started_at` retention through the daemon, or active-peer ack shape.
- `memoryd` has no stale-session cleanup integration tests proving the production daemon spawns the cleanup loop, removes stale presence records, releases stale sessions' claim locks, and shuts down cleanly.
- The existing `claim_lock_supersede` tests cover successful release and contention warning/event paths, but they do not prove a Level 3 heartbeat can renew a lock through the daemon protocol because that protocol path is absent.

### Questions / uncertainties

- The user-requested validation command omitted the plan's earlier `heartbeat_protocol` and `stale_session_cleanup` memoryd tests and instead ran governance suites. I followed the user-requested commands, but the plan/spec still make heartbeat and stale cleanup part of full Gate C, so I treated the absent memoryd wiring as blocking.
- I did not inspect recall assembler, XML rendering, peer status/activity CLI, or later Stream I tasks because they are explicitly out of scope for Gate C.
- The worktree contains broad pre-existing uncommitted Stream G/H/I changes outside this review file; I did not attempt to separate authorship beyond preserving them and only writing this review artifact.

### Validations

- `cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit` — passed: 15 `claim_lock_unit` tests and 23 `presence_unit` tests.
- `cargo test -p memoryd --test claim_lock_supersede --test governance_e2e --test governance_matrix_e2e` — passed: 5 `claim_lock_supersede` tests, 9 `governance_e2e` tests, and 3 `governance_matrix_e2e` tests.
- `cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-coordination -p memoryd -- --check` — passed.

### Positives

- The claim-lock registry now uses entry/predicate-based DashMap operations for the important acquire/release/sweep paths, which closes the prior concurrency holes without adding broad locks.
- The supersede wiring is placed after governance refusal paths and before the write, and release happens after successful write commit, so existing governance behavior is preserved in the covered tests.
- The contention response/event shape is behavior-tested against both JSONL and SQLite mirror surfaces, which is the right level of integration coverage for Task 13.
