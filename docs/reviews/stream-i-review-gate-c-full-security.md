# Stream I Review Gate C Full Security/Concurrency Review

**Date:** 2026-05-02  
**Scope:** Read-only security/concurrency review after Stream I Tasks 9-13. Reviewed the active Stream I plan/spec, previous Gate C reviews/reruns, `memorum-coordination` presence/claim-lock code/tests, and `memoryd` claim-lock supersede/governance wiring/tests.  
**Verdict:** Changes requested

## Findings by severity

### High — Task 10/11 heartbeat and stale-cleanup are still library-only, not integrated into `memoryd`

**Files:**

- `docs/plans/2026-05-01-stream-i-cross-session.md:651-714`, `docs/plans/2026-05-01-stream-i-cross-session.md:895-922`
- `crates/memorum-coordination/src/lib.rs:14-20`
- `crates/memoryd/src/protocol.rs:45-100`
- `crates/memoryd/src/handlers.rs:72-79`, `crates/memoryd/src/handlers.rs:155-195`

**Issue:** The coordination crate now exports `handle_peer_heartbeat`, `PresenceRegistry`, and `spawn_stale_session_cleanup_task`, but `memoryd` does not expose or dispatch the heartbeat protocol and does not hold/spawn the presence stale-cleanup state. `RequestPayload` has no `PeerHeartbeat` variant in the reviewed `memoryd` protocol, `dispatch` has no heartbeat arm, and `HandlerState` stores only `claim_locks`/level/TTL, not a `PresenceRegistry`. A targeted scan for `PeerHeartbeat`, `PresenceRegistry`, and `spawn_stale_session_cleanup_task` under `crates/memoryd/src` and `crates/memoryd/tests` returned no hits.

**Exploitability:** Any Level 3 client following the Stream I heartbeat contract has no daemon request path to call. Stale-session cleanup also has no daemon lifecycle hook, so a dead/stale session cannot be cleaned through the operational daemon path.

**Impact:** The full Gate C contract is not operationally integrated. Level 3 presence, heartbeat-driven claim-lock renewal, and stale-session lock release are proven only inside `memorum-coordination` unit tests, not in the daemon surface that agents actually use. This leaves concurrency behavior dependent on supersede success or passive TTL checks, rather than the spec's live heartbeat/stale sweeper model.

**Minimal remediation:** Add `PeerHeartbeat` to `memoryd::protocol::RequestPayload`/`ResponsePayload`, store `PresenceRegistry` in `HandlerState`, dispatch to the coordination heartbeat handler with `ClaimLockHeartbeatRenewal`, and spawn the stale-session cleanup task in daemon startup/shutdown wiring. Add/restore daemon-level `heartbeat_protocol` and `stale_session_cleanup` tests from the plan.

### High — Claim-lock warning/event identity fields bypass privacy and bounds validation before canonical event persistence

**Files:**

- `crates/memoryd/src/mcp.rs:89-95`, `crates/memoryd/src/mcp.rs:208-213`
- `crates/memoryd/src/handlers.rs:1987-2005`, `crates/memoryd/src/handlers.rs:2122-2136`, `crates/memoryd/src/handlers.rs:2139-2158`
- `crates/memoryd/src/handlers.rs:1296-1316`
- `crates/memory-substrate/src/events/log.rs:136-144`

**Issue:** `memory_supersede` accepts a free-form `meta` object, and `GovernanceMeta.session_id` / `GovernanceMeta.harness` are deserialized as unconstrained strings. `GovernanceWriteInput::parse` validates confidence only; the privacy scan includes body/title/summary/source refs/tags/descriptors but not `session_id` or `harness`. Task 13 then uses these unvalidated values as claim-lock holder identity and writes `holder`/`contender` strings into `EventKind::ClaimLockContention` and the returned warning.

**Exploitability:** A local MCP/daemon caller that can submit `memory_supersede` can place a secret-like value, personal identifier, control-ish string, or oversized value in `meta.session_id` or `meta.harness`. Under contention, the value is serialized into the canonical event JSONL and SQLite mirror as `holder` or `contender`, bypassing the privacy filter that would protect the memory body. A previous holder can also poison the later contender's warning/event payload because the warning intentionally echoes the holder label.

**Impact:** Secret or personal data can land in repo-backed event logs and runtime SQLite despite Stream D/handler privacy checks. This is the exact kind of metadata side channel previous security reviews closed for observe/dream paths, and it also creates a local log/amplification risk from unbounded identity strings.

**Minimal remediation:** Reuse the same kind of safe binding validation already used by `memory_observe`: trim policy, non-empty, <=128 bytes, safe identifier characters, and `is_safe_plaintext_for_indexing`/canary rejection for `harness` and `session_id` before they reach `ClaimLockAcquireRequest`. Add tests proving AWS-key/phone/token canaries and oversized values in supersede `meta.harness`/`meta.session_id` are rejected before claim-lock event append or warning serialization.

### Medium — Post-acquire failure paths can leave stale or wrong claim locks, and contention event append can turn advisory contention into a hard failure

**Files:**

- `crates/memoryd/src/handlers.rs:1256-1272`
- `crates/memoryd/src/handlers.rs:1286-1317`
- `crates/memorum-coordination/src/claim_lock.rs:166-183`
- `crates/memory-substrate/src/api.rs:1293-1302`, `crates/memory-substrate/src/api.rs:1473-1480`

**Issue:** `governance_supersede_response` acquires the claim lock before `supersede_memory`, then releases only after the write returns success. There is no guard/finally path to release or roll back when an error occurs after acquisition. In the contended case, `ClaimLockRegistry::acquire_at` replaces the previous live holder with the contender before `memoryd` attempts event append or disk write. If `record_event_best_effort` fails, `acquire_claim_lock_for_supersede` returns an error before the supersede write proceeds and before the release path runs.

**Exploitability:** This is mainly an operational-fault/concurrency exploit rather than a normal happy path: event-log append failure, filesystem/index failure during `supersede_memory`, or another post-acquire error can trigger it. A local actor who can induce an event-log write failure can convert an advisory contention warning into a failed supersede and leave the registry in the contender-held state.

**Impact:** A failed supersede can leave a stale/misleading claim lock for the failed session and can erase/replace the prior holder's advisory lock. Because the daemon stale-sweeper is not wired, cleanup is weaker than the spec assumes. This undermines the lock lifecycle around failure even though success-path release is covered.

**Minimal remediation:** Introduce a small claim-lock guard around the acquired/replaced lock and release or restore on every error path after acquisition. Decide whether `ClaimLockContention` event append is mandatory or genuinely best-effort; if advisory liveness is the priority, log event-append failures and still proceed with the supersede warning. Add tests for post-acquire event-append/write failure proving the registry is not left with a stale contender lock.

## Positive checks

- The prior `ClaimLockRegistry::acquire_at` double-acquire race is closed: current acquire uses `DashMap::entry` and updates/replaces under the entry guard (`crates/memorum-coordination/src/claim_lock.rs:163-197`), with a concurrent first-acquire test requiring exactly one `Acquired` and one `Contended` result (`crates/memorum-coordination/tests/claim_lock_unit.rs:73-95`).
- Ownership checks now use full `harness + session_id` identity in the registry (`crates/memorum-coordination/src/claim_lock.rs:128-130`, `crates/memorum-coordination/src/claim_lock.rs:204-229`), and tests cover same-session/different-harness renew/release protection (`crates/memorum-coordination/tests/claim_lock_unit.rs:177-195`).
- Release and sweep use predicate re-checks, avoiding the previous remove-after-stale-read race (`crates/memorum-coordination/src/claim_lock.rs:217-249`; tests at `crates/memorum-coordination/tests/claim_lock_unit.rs:198-248`).
- Successful Task 13 supersede flow preserves governance precedence before claim-lock acquisition (`crates/memoryd/src/handlers.rs:1160-1256`) and has a test that governance refusal is not replaced by a claim-lock warning (`crates/memoryd/tests/claim_lock_supersede.rs:94-123`).
- The Level 1/2 gate is present on acquire/release (`crates/memoryd/src/handlers.rs:110-116`, `crates/memoryd/src/handlers.rs:1292-1294`, `crates/memoryd/src/handlers.rs:1319-1322`) and covered by Level 1/Level 2 supersede tests (`crates/memoryd/tests/claim_lock_supersede.rs:12-46`).
- Successful contention warning/event shape is covered for the happy path: warning code/holder and JSONL/SQLite contention event are asserted in `crates/memoryd/tests/claim_lock_supersede.rs:48-92`.

## Validations run

```bash
cargo test -p memorum-coordination --test presence_unit --test claim_lock_unit
```

Result: passed — 15 `claim_lock_unit` tests and 23 `presence_unit` tests.

```bash
cargo test -p memoryd --test claim_lock_supersede --test governance_e2e --test governance_matrix_e2e
```

Result: passed — 5 `claim_lock_supersede` tests, 9 `governance_e2e` tests, and 3 `governance_matrix_e2e` tests.

```bash
cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings
```

Result: passed.

Additional read-only evidence:

```bash
rg -n "PeerHeartbeat|PresenceRegistry|spawn_stale_session_cleanup_task|cleanup_stale_sessions|StaleSessionClaimLockReleaser|PRESENCE_CLEANUP_INTERVAL" crates/memoryd/src crates/memoryd/tests
```

Result: no matches, confirming the heartbeat/stale-cleanup primitives are not wired into `memoryd` in the reviewed tree.

## Residual risk and confidence

Residual risk is moderate-to-high until the findings above are fixed and re-tested with daemon-level heartbeat/stale-cleanup coverage plus failure-path claim-lock tests. I did not modify code and did not attempt destructive filesystem fault injection in this read-only review. The worktree had substantial pre-existing uncommitted Stream G/H/I changes; this review reflects the current dirty tree state.

Confidence: high for the missing `memoryd` integration and identity privacy findings based on direct line-level inspection. Confidence: medium-high for the post-acquire failure lifecycle finding; the code path is clear, but exploit likelihood depends on operational IO failures or fault injection rather than the passing happy-path tests.
