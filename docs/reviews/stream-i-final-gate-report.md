# Stream I Final Gate Report

Status: Passed
Date: 2026-05-02

## Scope

Stream I ships cross-session coordination: coordination config, session derivation, peer-write relevance, presence heartbeat/ACKs, claim-lock contention, daemon startup/delta insertion, MCP/protocol/API docs, framing regressions, and latency evidence.

## Review loop status

All final review lanes are closed:

- Clean-code rerun: `docs/reviews/stream-i-final-clean-code-review-rerun.md` — Approved.
- Security review: `docs/reviews/stream-i-final-security-review.md` — Approved.
- Performance rerun: `docs/reviews/stream-i-final-performance-review-rerun.md` — Approved.
- Test rerun: `docs/reviews/stream-i-final-test-review-rerun.md` — Approved.
- API contract rerun 2: `docs/reviews/stream-i-final-api-contract-review-rerun-2.md` — Approved.

## Material closeout fixes

- Wired coordination configuration through daemon startup/delta paths and added `crates/memoryd/tests/coordination_config.rs`.
- Ensured Tier 3/presence-only rows short-circuit before peer-update scoring.
- Enforced deterministic presence cap ordering and bounded active-peer public projection.
- Added heartbeat claim-lock conflict detection/renewal and runtime stale-session cleanup for expired claim locks.
- Removed startup peer-update relevance fallback/mutation risk: project rows no longer make themselves relevant by mutating the receiving `SessionContext`, and empty relevance-gate output produces no peer-update insertion.
- Reconciled `PeerHeartbeatAck` across spec, API docs, DTOs, handler, and tests, including `active_peers`.
- Added framing and benchmark artifacts: `crates/memorum-coordination/src/framing_tests.rs`, `crates/memorum-coordination/src/bin/peer_relevance_bench.rs`, `docs/reviews/stream-i-bench-evidence.md`, and `bench/stream-i-cross-session-results.darwin-arm64.json`.
- Hardened release test harness behavior for dream echo override and fake CLI PATH handling exposed by the final full gate.

## Validation

Final all-in gate:

- `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` — passed.

Focused Stream I gates run during closeout:

- `cargo test -p memory-substrate --test recall_index_row_indexed_at` — passed.
- `cargo test -p memorum-coordination --test gate_unit --test session_derivation --test presence_unit --test claim_lock_unit` — passed.
- `cargo test -p memorum-coordination --lib framing_tests` — passed.
- `cargo test -p memoryd --test project_binding_concurrent_mode --test heartbeat_protocol --test stale_session_cleanup --test claim_lock_supersede --test coordination_recall_render --test coordination_integration --test peer_cli` — passed.
- `cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json` — passed.
- `cargo test -p memoryd --test startup_recall_mcp` — passed.
- `cargo test -p memorum-coordination --test presence_unit` — passed.
- `cargo test -p memoryd --test dream_cli --release -- --nocapture` — passed.
- `cargo test -p memorum-eval --test harness_runner_detection --release -- --nocapture` — passed.
- `cargo test -p memoryd --test handler_contract --release -- --nocapture` — passed.
- `cargo clippy -p memorum-coordination -p memoryd --all-targets --all-features -- -D warnings` — passed.
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings` — passed.
- `cargo clippy -p memorum-eval --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.

## Performance evidence

`bench/stream-i-cross-session-results.darwin-arm64.json` passed the peer relevance budget:

- Peer relevance gate latency: p50 0.005927 ms/candidate, p95 0.006917 ms/candidate, p99 0.007670 ms/candidate.
- Budget: <= 5.0 ms/candidate.
- Fixture: 100 fixed peer-write candidates, 301 samples, embedding worker wait excluded.

The final workspace release bench also passed regression checks in `bench/results.darwin-arm64.json`.

## Residual risks

- Stream I coordination is locally and fixture-tested; live multi-machine behavior still depends on real daemon peers, clocks, git sync timing, and operator auth/CLI setup.
- The final API rerun noted one non-blocking future hardening test: a cross-device unrelated-project negative startup case. The reviewed code path now shares the same no-mutation/no-fallback relevance gate and is not considered a blocker.
- Active peer projection is intentionally bounded and privacy-filtered; it is not a rich collaboration roster.
