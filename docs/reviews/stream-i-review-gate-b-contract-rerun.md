# Stream I Review Gate B Contract Rerun

Verdict: Approve

Scope: read-only rerun after fixes for Stream I Gate B in `crates/memorum-coordination`, checked against `docs/reviews/stream-i-review-gate-b-contract.md`, `docs/reviews/stream-i-review-gate-b-clean-code.md`, `docs/plans/2026-05-01-stream-i-cross-session.md` Gate B / Tasks 4-8, and `docs/specs/stream-i-cross-session-v0.1.md`.

## Findings

### Severity 1

None.

### Severity 2

None. The previous S2 contract findings are closed.

### Severity 3

None. The previous S3 contract/clean-code findings in Gate B scope are closed or have targeted regression coverage.

## Previous finding closure

- Closed: `PeerUpdateEntry.reference` now uses the stable memory id rather than the namespace path. Evidence: `crates/memorum-coordination/src/gate.rs:167-178` sets `reference: candidate.memory_id.to_string()` and leaves `namespace` separate; `crates/memorum-coordination/tests/gate_unit.rs:113-125` asserts the reference equals the `mem_...` id and is not the path.
- Closed: surfaced peer writes are now recorded by the gate. Evidence: `RelevanceGate::evaluate` takes `&mut SessionContext`, filters prior surfaced ids, and records selected ids before DTO construction at `crates/memorum-coordination/src/gate.rs:28-65`; `crates/memorum-coordination/tests/gate_unit.rs:99-111` calls `evaluate` twice and asserts the second result is empty.
- Closed: same-device peer-update DTOs no longer expose raw `source_device`. Evidence: `crates/memorum-coordination/src/gate.rs:167-178` sets `device: None`; `crates/memorum-coordination/tests/gate_unit.rs:127-135` asserts the same-device gate output leaves `device` unset.
- Closed: project-binding mode vocabulary now matches `minimal | default | collaborative`. Evidence: `crates/memorum-coordination/src/session.rs:6-30` defines `ConcurrentSessionMode::Default` and string mapping for `default`; `crates/memorum-coordination/tests/session_derivation.rs:125-131` covers the project vocabulary.
- Closed: `ref` attribute parsing no longer matches substring attributes like `data-ref` or `xref`. Evidence: `crates/memorum-coordination/src/session.rs:192-239` scans attribute names before comparing to the requested name; `crates/memorum-coordination/tests/session_derivation.rs:105-123` asserts only the real `ref` value is included.

## Contract checks

- `CoordinationConfig` defaults: pass. Defaults match Gate B/spec defaults: level 2, threshold 0.6, recency 1800 seconds, cap 2, cross-device startup window 86400 seconds, cross-device threshold 0.7, heartbeat 60 seconds, stale-after 300 seconds, claim-lock TTL 300 seconds (`crates/memorum-coordination/src/config.rs:12-80`).
- `CoordinationInsertion` / DTO shape: pass for Gate B. `CoordinationInsertion` has the four Stream E insertion fields and `PeerUpdateEntry` keeps separate `reference`, `namespace`, `claim_locked`, and `device` fields (`crates/memorum-coordination/src/protocol.rs:3-34`).
- Score weights and component semantics: pass. Weights are 0.5 / 0.3 / 0.2; empty-empty entity sets return 0.0; path matching is exact string; missing or mismatched embeddings return 0.0; cosine is clamped (`crates/memorum-coordination/src/gate.rs:9-164`).
- Threshold, recency, cap, sorting, and overflow count: pass. `evaluate` filters by `local_observed_at`, threshold `>=`, cap, capped count, descending score, descending `updated_at`, ascending memory id (`crates/memorum-coordination/src/gate.rs:38-72`).
- Tier 3 short-circuit: pass. `evaluate` returns `CoordinationInsertion::empty()` before embedding lookup/scoring when `session.is_tier3()` (`crates/memorum-coordination/src/gate.rs:28-36`), with regression coverage in both gate and session tests.
- Cool-down contract: pass. Selected ids are recorded into `SessionContext.surfaced_peer_writes` during evaluation and excluded on later calls (`crates/memorum-coordination/src/gate.rs:43-65`; `crates/memorum-coordination/src/session.rs:151-157`).
- Entity/path derivation: pass for Gate B. Tier 1 startup entities plus last-3-turn FTS5 ids are merged; Tier 3 derives only binding identifiers; startup recall paths are extracted only from `entity-recall` and `project-state`; Tier 1 session paths can be added; Tier 3 `add_session_paths` is ignored (`crates/memorum-coordination/src/session.rs:73-174`, `246-318`).
- Recent-query embedding cache: pass for Gate B. Cache keys include `(session_id, message_hash)`, lookup is synchronous/non-blocking, cache miss returns no session embedding, and triple mismatches score as zero topic similarity (`crates/memorum-coordination/src/session.rs:124-149`; `crates/memorum-coordination/src/gate.rs:116-131`).
- `memory-governance` dependency: pass. `crates/memorum-coordination/Cargo.toml` depends on `chrono`, `dashmap`, `memory-privacy`, `memory-substrate`, and `serde`, and the requested `cargo tree` grep returned no matches (`crates/memorum-coordination/Cargo.toml:8-13`).

## Validations

- `cargo test -p memorum-coordination --test gate_unit --test session_derivation` — passed: 17 gate tests, 9 session derivation tests.
- `cargo test -p memorum-coordination` — passed: crate unit tests, placeholder presence/claim-lock tests, 17 gate tests, 9 session tests, doc-tests.
- `cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings` — passed.
- `cargo tree -p memorum-coordination | rg "memory-governance"` — no matches; command exited 1 as expected for an empty grep result.
- Extra format sanity check: `cargo fmt --package memorum-coordination -- --check` — passed.

## Residual risks / notes

- `tests/presence_unit.rs` and `tests/claim_lock_unit.rs` remain placeholders. I do not treat this as a Gate B blocker because the active Gate B scope is Tasks 4-8, and plan Tasks 9+ own presence/claim-lock behavior.
- This rerun did not inspect memoryd integration or XML rendering beyond the coordination DTO contract; those are later gates.
- The repository has a large dirty tree outside this review file and the coordination crate. I did not modify or revert any unrelated files.
