# Stream I Final Performance Review Rerun

Status: Approved

## Findings

None. The two prior Severity 2 performance blockers are fixed, and the remaining benchmark-scope limitation is non-blocking for Stream I v0.1 given the current plan contract, code-path changes, targeted tests, and passing canonical bench assert.

## Fix verification

### 1. Tier 3 and stale-row short-circuit before expensive candidate reads

Fixed.

- Delta recall now resolves the effective coordination level before peer-row acquisition and returns immediately for Level 1 (`crates/memoryd/src/recall/delta.rs:108-111`). It then builds the session context and returns for Tier 3 before `delta_peer_candidate_rows(...)` is called (`crates/memoryd/src/recall/delta.rs:113-122`). This closes the prior Tier 3 path that could pay recall-index/candidate-read cost before the gate returned empty.
- Delta stale-row filtering now happens before `peer_write_candidates(...)` performs per-row source-identity reads: rows are filtered on `indexed_at >= recency_cutoff` at `crates/memoryd/src/recall/delta.rs:118-125`, while `peer_write_candidates(...)` calls `peer_source_identity(...)` only after receiving that filtered slice (`crates/memoryd/src/recall/delta.rs:129-132`, `crates/memoryd/src/recall/delta.rs:246-254`). The expensive read itself is still isolated in `peer_source_identity(...)`, which calls `substrate.read_memory(...)` (`crates/memoryd/src/recall/source_identity.rs:34-38`).
- Startup recall now returns before peer-row acquisition when the effective level is below 2 or the session is Tier 3 (`crates/memoryd/src/recall/startup.rs:198-208`). Same-device and cross-device startup rows are then filtered by `indexed_at` before `peer_write_candidates(...)`/`peer_source_identity(...)` runs (`crates/memoryd/src/recall/startup.rs:281-288`, `crates/memoryd/src/recall/startup.rs:305-315`, `crates/memoryd/src/recall/startup.rs:424-432`).
- The underlying gate still has the entry-point Tier 3 guard (`crates/memorum-coordination/src/gate.rs:37-50`) and still filters candidate rows by `indexed_at` before scoring (`crates/memorum-coordination/src/gate.rs:52-64`). Regression coverage verifies indexed-at recency and zero scorer calls for Tier 3 (`crates/memorum-coordination/tests/gate_unit.rs:140-160`, `crates/memorum-coordination/tests/gate_unit.rs:162-196`), plus startup cross-device stale exclusion (`crates/memoryd/tests/startup_recall_mcp.rs:124-135`).

Residual note: the recall-index queries still fetch row sets before the in-memory `indexed_at` filter (`crates/memoryd/src/recall/delta.rs:155-180`, `crates/memoryd/src/recall/startup.rs:236-256`). I do not treat that as a release blocker because the fixed code eliminates the prior dominant per-row `read_memory` cost for stale rows and fully removes Stream I peer-row acquisition for Tier 3. It remains a reasonable v1.x improvement to push the `indexed_at` cutoff into SQLite if production namespaces grow large.

### 2. Runtime cleanup sweeping expired claim locks

Fixed.

- The stale-session cleanup abstraction now exposes `sweep_expired_at(...)`, and the concrete `ClaimLockRegistry` implementation delegates to `ClaimLockRegistry::sweep_expired_at(...)` (`crates/memorum-coordination/src/presence.rs:30-45`).
- The 60-second cleanup task now performs both stale-session cleanup and expired claim-lock sweeping on every tick (`crates/memorum-coordination/src/presence.rs:220-233`). The sweeper implementation removes expired locks by collecting keys then `remove_if`ing still-expired entries, avoiding iterator-removal races (`crates/memorum-coordination/src/claim_lock.rs:270-288`).
- memoryd production startup wires this cleanup task through `spawn_coordination_cleanup_for_state(...)` in both substrate-backed server entry points (`crates/memoryd/src/server.rs:53-90`, `crates/memoryd/src/server.rs:106-116`).
- Regression coverage now proves the daemon cleanup task sweeps an expired claim lock even while presence remains fresh (`crates/memoryd/tests/stale_session_cleanup.rs:57-80`) and still covers stale-presence lock release (`crates/memoryd/tests/stale_session_cleanup.rs:10-33`).

### 3. Remaining benchmark limitation: non-blocking

Non-blocking.

- The Stream I plan's Task 21 benchmark contract is an in-memory relevance-gate fixture with 100 candidates, 50 inside recency, 50 outside recency, precomputed embeddings, and assert mode against `bench/stream-i-cross-session-results.darwin-arm64.json` (`docs/plans/2026-05-01-stream-i-cross-session.md:1268-1305`). The current bench implementation matches that fixture shape (`crates/memorum-coordination/src/bin/peer_relevance_bench.rs:15-25`, `crates/memorum-coordination/src/bin/peer_relevance_bench.rs:147-169`) and measures `RelevanceGate::evaluate(...)` over the prebuilt candidate slice (`crates/memorum-coordination/src/bin/peer_relevance_bench.rs:172-206`).
- The canonical baseline passes: `bench/stream-i-cross-session-results.darwin-arm64.json:21-34` records p95 `0.006917 ms` against the `<= 5 ms` budget with `pass: true`.
- The rerun also passed, with p95 `0.006935 ms`, selected peer updates `2`, capped peer updates `48`, and `pass: true`.
- The bench limitation is accurately documented rather than hidden: it certifies relevance-gate computation, not daemon IPC, SQLite query time, recall XML rendering, or terminal/browser latency (`docs/reviews/stream-i-bench-evidence.md:98-102`). After the fixes above, the unbenchmarked candidate-acquisition path no longer performs stale-row `read_memory` work and the Tier 3 path returns before Stream I peer-row acquisition. That makes the remaining limitation a residual risk, not a final-gate blocker.

## Evidence reviewed

- Original blocker artifact: `docs/reviews/stream-i-final-performance-review.md`.
- Normative performance/review contract: `docs/plans/2026-05-01-stream-i-cross-session.md:1268-1305`, `docs/plans/2026-05-01-stream-i-cross-session.md:1386-1404`; `docs/specs/stream-i-cross-session-v0.1.md:262-320`, `docs/specs/stream-i-cross-session-v0.1.md:540-548`, `docs/specs/stream-i-cross-session-v0.1.md:600-605`.
- Implementation: `crates/memoryd/src/recall/delta.rs`; `crates/memoryd/src/recall/startup.rs`; `crates/memoryd/src/recall/source_identity.rs`; `crates/memorum-coordination/src/gate.rs`; `crates/memorum-coordination/src/presence.rs`; `crates/memorum-coordination/src/claim_lock.rs`; `crates/memoryd/src/server.rs`; `crates/memorum-coordination/src/bin/peer_relevance_bench.rs`.
- Tests: `crates/memorum-coordination/tests/gate_unit.rs`; `crates/memorum-coordination/tests/session_derivation.rs`; `crates/memorum-coordination/tests/presence_unit.rs`; `crates/memorum-coordination/tests/claim_lock_unit.rs`; `crates/memoryd/tests/stale_session_cleanup.rs`; `crates/memoryd/tests/startup_recall_mcp.rs`; `crates/memoryd/tests/coordination_integration.rs`.
- Bench evidence: `docs/reviews/stream-i-bench-evidence.md`; `bench/stream-i-cross-session-results.darwin-arm64.json`.

## Commands run

- `cargo test -p memorum-coordination --test gate_unit --test session_derivation --test presence_unit --test claim_lock_unit` — passed: 67 tests.
- `cargo test -p memoryd --test stale_session_cleanup` — passed: 3 tests.
- `cargo test -p memoryd --test startup_recall_mcp test_startup_no_cross_device_outside_window` — passed: 1 test.
- `cargo test -p memoryd --test startup_recall_mcp` — passed: 14 tests.
- `cargo test -p memoryd --test coordination_integration` — passed: 12 tests.
- `cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json` — passed; rerun p95 `0.006935 ms` versus `<= 5 ms` budget.
- `jq '.peer_relevance_gate, .fixture' bench/stream-i-cross-session-results.darwin-arm64.json` — confirmed canonical fixture shape and p95 `0.006917 ms`, `pass: true`.

## Residual risks

- There is still no end-to-end recall-hot-path p95 benchmark covering daemon protocol, SQLite recall-index query time, source-identity reads for live rows, XML rendering, and audit/cooldown recording together. The current Task 21 bench is sufficient for v0.1 because it matches the plan contract and the prior stale-row/Tier 3 read-amplification risks are fixed by code-path ordering, but a future release should add a full hot-path benchmark if namespaces become large.
- The `indexed_at` recency cutoff is still applied after recall-index row acquisition rather than pushed into SQLite. That is acceptable for this rerun because stale rows no longer trigger `read_memory`, but very large namespaces could still spend time materializing and filtering rows.
- The focused gates above passed; I did not rerun the full workspace clippy/test/doc stack for this review-only performance rerun.
