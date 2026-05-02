# Stream I Review Gate C — Full Clean Code Rerun

### Verdict

Approve

### Intended outcome

This rerun validates that the prior full Gate C clean-code/security blockers for Stream I Tasks 9-13 are closed after the memoryd heartbeat/stale cleanup fixes. The intended outcome is an operational daemon path for Level 3 peer heartbeat/presence, daemon lifecycle stale-session cleanup, safe advisory claim-lock identity handling, safe post-acquire cleanup, and best-effort contention event emission without regressing Stream C governance.

### Executive summary

The implementation now satisfies the Gate C business outcome in the reviewed scope. `memoryd` exposes and dispatches `PeerHeartbeat`, stores `PresenceRegistry` in `HandlerState`, renews claim locks through the heartbeat path, spawns stale-session cleanup from the substrate-backed server lifecycle, validates claim-lock identity fields before event/warning emission, restores or releases locks on post-acquire failure, and keeps contention event persistence best-effort. The requested unit/integration tests, clippy, and fmt gates all pass. No material issues found.

### Findings

No material issues found.

### Non-blocking simplifications

- The stale-session cleanup test directly exercises `spawn_coordination_cleanup_for_state`, while `serve_substrate`/`serve_substrate_with` call that helper in production startup. That is acceptable for this gate; if this lifecycle grows more complex later, a socket-level smoke test around `serve_substrate_with` could give stronger confidence without changing the current design.

### Test gaps

- No blocking test gaps found for the scoped rerun. The new coverage verifies heartbeat serde/dispatch behavior, Level 3 presence updates, Level 1/2 heartbeat no-op behavior, stale cleanup lock release, identity validation before contention logging, and post-acquire failure restoration.

### Questions / uncertainties

- The worktree is broadly dirty with Stream G/H/I changes outside this review scope. I reviewed the current dirty tree and did not attempt to attribute unrelated changes.
- I did not perform destructive fault injection beyond the existing write-failure test coverage for claim-lock rollback.

### Positives

- The previous heartbeat integration blocker is closed cleanly: `RequestPayload::PeerHeartbeat` is part of the daemon protocol, `dispatch` routes it to `peer_heartbeat_response`, and the handler passes `PresenceRegistry`, stale threshold, and claim-lock renewal into the coordination crate (`crates/memoryd/src/protocol.rs:91-93`, `crates/memoryd/src/handlers.rs:80-129`, `crates/memoryd/src/handlers.rs:213-251`).
- Stale cleanup is now in the server lifecycle rather than only the library: both `serve_substrate` and `serve_substrate_with` spawn `spawn_coordination_cleanup_for_state`, which wires the state-owned presence and claim-lock registries to the coordination cleanup task (`crates/memoryd/src/server.rs:53-90`, `crates/memoryd/src/server.rs:99-105`).
- The claim-lock security/concurrency fixes are appropriately small and owner-minded: identity screening happens during governance meta parsing before acquire/event emission, contention event append is best-effort, and the `SupersedeClaimLock` drop guard releases or restores locks on error paths (`crates/memoryd/src/handlers.rs:1364-1404`, `crates/memoryd/src/handlers.rs:1407-1520`, `crates/memoryd/src/handlers.rs:2325-2336`).

### Validations

- `cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit` — passed: 17 `claim_lock_unit` tests and 23 `presence_unit` tests.
- `cargo test -p memoryd --test claim_lock_supersede --test heartbeat_protocol --test stale_session_cleanup --test governance_e2e --test governance_matrix_e2e` — passed: 8 `claim_lock_supersede` tests, 7 `heartbeat_protocol` tests, 2 `stale_session_cleanup` tests, 9 `governance_e2e` tests, and 3 `governance_matrix_e2e` tests.
- `cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-coordination -p memoryd -- --check` — passed.

### Evidence checked

- Previous full clean-code and security reviews: `docs/reviews/stream-i-review-gate-c-full-clean-code.md`, `docs/reviews/stream-i-review-gate-c-full-security.md`.
- Heartbeat protocol and handler wiring: `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/heartbeat_protocol.rs`.
- Stale cleanup lifecycle: `crates/memoryd/src/server.rs`, `crates/memoryd/tests/stale_session_cleanup.rs`, `crates/memorum-coordination/src/presence.rs`.
- Claim-lock identity, contention, rollback, and tests: `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/claim_lock_supersede.rs`, `crates/memorum-coordination/src/claim_lock.rs`.
