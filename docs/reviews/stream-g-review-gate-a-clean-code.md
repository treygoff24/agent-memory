### Verdict

Changes requested

### Intended outcome

This change appears to land Stream G Review Gate A's cross-stream substrate and recall surfaces: add the new observability event kinds, make canonical JSONL events queryable through a rebuildable SQLite `events_log` projection, add the `memory_supersession` projection and `original_confidence` field, surface `RecallIndexRow` fields needed by later streams, and emit best-effort `RecallHit` events from Stream E startup/delta recall without changing XML output. The core business outcome is to give later Stream G drift scoring accurate, queryable recall/supersession data while preserving Stream A's JSONL/frontmatter source-of-truth model and Stream E's hot-path recall contract.

### Executive summary

The implementation is not ready to ship. The new targeted tests pass, but they miss important Stream G §1.3 invariants, and the current `events_log` design loses events as soon as multiple devices have the same per-device sequence number. RecallHit emission also bypasses Stream A's event sequence allocator and does synchronous fsync + full mirror rebuild work inside the recall response path, which violates the intended best-effort/fire-and-forget hot-path behavior and is race-prone under concurrent recall. The XML shape appears preserved in the tested paths, and the model/schema additions are mostly straightforward, but the event-log mirror and emission seam need correction before downstream drift scoring can rely on them.

### Findings

[High] [Data Integrity] `events_log` collapses multi-device events with the same per-device sequence

- Evidence: `crates/memory-substrate/src/index/schema.rs:63-69` and `crates/memory-substrate/src/index/migrations.rs:129-135` define `events_log(seq INTEGER PRIMARY KEY)` with no device column; `crates/memory-substrate/src/index/query.rs:346-356` mirrors events via `INSERT OR REPLACE INTO events_log(seq, ...)`; `crates/memory-substrate/src/api.rs:1402-1408` rebuilds by reading every `events/*.jsonl` file and sorting only by `seq` then event id. Stream A's event sequence allocator is explicitly per-device (`crates/memory-substrate/src/events/sequence.rs:32-40`).
- Why it matters: In a real synced repository, each device has its own JSONL file and starts at sequence 1. Rebuilding or mirroring events from two devices with `seq = 1` overwrites one row with the other, so recall counts, last-recalled timestamps, and any later drift score based on `events_log` are silently wrong. Mirror health can also report `lag = 0` while rows are missing, because it compares only max sequence numbers.
- Reasoning: The canonical source is per-device JSONL, but the SQLite projection key is only the per-device sequence. `INSERT OR REPLACE` makes collisions destructive rather than visible. This contradicts the stated derived/rebuildable projection invariant: a rebuild from canonical JSONL must be lossless enough for Stream G queries.
- Recommendation: Include event device identity in the mirror schema and uniqueness, e.g. `device TEXT NOT NULL` plus `PRIMARY KEY(device, seq)` or a unique `event_id` key, and update mirror insertion, rebuild, health, indexes, docs, and tests. Add a regression test that creates two device logs with overlapping `seq` values and asserts both rows survive rebuild and health detects missing rows, not just max-seq lag.
- Confidence: High

[High] [Concurrency] RecallHit emission bypasses Stream A's locked event sequence allocator

- Evidence: `crates/memoryd/src/recall/render.rs:57-79` reads `local-device.yaml`, computes `seq` by scanning `substrate.events()` through `next_recall_hit_sequence`, constructs event ids/operation ids manually, and calls `memory_substrate::events::append_event` directly. This path does not call `Substrate::record_event`/`build_recorded_event` or `reserve_event_sequence` (`crates/memory-substrate/src/events/sequence.rs:32-40`). `Substrate::events()` reads only the local device's current event log (`crates/memory-substrate/src/api.rs:1164-1166`).
- Why it matters: Two concurrent startup/delta recalls can both observe the same max sequence and append duplicate sequence numbers to the same per-device JSONL file. The persisted `event-seq.json` is also not advanced, so the next normal substrate event can reuse a sequence number that recall already wrote. With the current mirror schema this can overwrite rows; even after fixing the schema, it breaks Stream A's per-device monotonic event sequence contract and makes event ordering/audit data unreliable.
- Reasoning: Stream A already has a lock-backed allocator specifically to serialize sequence reservation and recover high-water marks. Reimplementing sequence allocation in the recall renderer creates a parallel event-writing path with weaker guarantees. This is especially risky because Stream E response building is async and can be invoked concurrently.
- Recommendation: Add/use a substrate API for best-effort event recording that goes through the same event sequence allocator and mirror hook as other events. Recall should pass `EventKind::RecallHit` and let Stream A stamp `device`, `seq`, `event_id`, `operation_id`, and mirror rows. Add a concurrent recall emission test that runs multiple recalls against one substrate and asserts unique, monotonic per-device sequences and no duplicate `(device, seq)` rows.
- Confidence: High

[Medium] [Performance] RecallHit emission does blocking fsync and full mirror rebuild work in the recall hot path

- Evidence: `crates/memoryd/src/recall/render.rs:79` uses `append_event`, which fsyncs each event (`crates/memory-substrate/src/events/log.rs:211-235`), and then `crates/memoryd/src/recall/render.rs:87-90` calls `substrate.doctor_reindex_events_log()` after any append. `doctor_reindex_events_log()` reads all JSONL logs and rebuilds the full SQLite mirror (`crates/memory-substrate/src/api.rs:1168-1178`). This runs synchronously from `build_startup_response`/`build_delta_response` async functions after XML rendering.
- Why it matters: Stream G's plan calls emission fire-and-forget/best-effort and says not to block the recall hot path. A startup response with several memories now performs one fsync per memory plus an O(total events) mirror rebuild before returning. As event history grows, recall latency can degrade materially, and blocking filesystem/SQLite work inside async code can stall the Tokio worker.
- Reasoning: The substrate already added `append_event_and_mirror`, which does an incremental mirror write after the canonical append. Recall bypasses that seam and compensates by rebuilding the mirror wholesale. That is simpler locally but wrong for the operational contract and will scale poorly.
- Recommendation: Route RecallHit emission through an incremental substrate best-effort event API. It should append canonical JSONL, attempt one mirror insert fail-soft, log through `tracing::warn!`, and return without rebuilding the mirror. If truly fire-and-forget is required, spawn a bounded/background task or use an existing daemon worker, but preserve allocator/mirror semantics.
- Confidence: High

[Medium] [Tests] Review Gate A test coverage does not cover several required §1.3 invariants

- Evidence: Existing added tests pass (`cargo test -p memoryd --test recall_hit_emission`; `cargo test -p memory-substrate --test events_log_mirror`; and the grouped substrate tests for event kinds, supersession, migration, original confidence, and recall row fields), but the files are much narrower than the plan. For example, `crates/memory-substrate/tests/events_log_mirror.rs` has 3 tests and does not cover dual-write failure, covering-index query plan, multi-device overlapping sequences, or fail-soft lag observability. `crates/memory-substrate/tests/migration_v4.rs` tests fresh schema and a simulated v3 reopen but not JSONL backfill or supersession/original-confidence backfill. `crates/memoryd/tests/recall_hit_emission.rs` checks two generated startup responses are equal, but not byte identity against a pre-Stream-G baseline fixture.
- Why it matters: The missing tests are exactly where the current defects live. Review question 1 asks whether all Stream G §1.3 substrate/recall invariants are covered; they are not. Without stronger tests, downstream scoring can be built on a mirror that silently loses rows or a recall emitter that is only safe in single-threaded fixtures.
- Reasoning: The test suite validates the happy path for new fields and one-device event mirroring, but it does not exercise canonical/rebuildable projection semantics under peer logs, mirror failure, migration backfill, or concurrency. The recall tests assert XML does not contain RecallHit markup, but they do not prove the Stream E baseline shape is byte-for-byte unchanged.
- Recommendation: Add the missing behavior tests before proceeding to later Stream G tasks: multi-device overlapping sequence rebuild; mirror fail-soft with JSONL canonical and health finding/lag semantics; `EXPLAIN QUERY PLAN` for the covering index; v4 backfill from JSONL and existing `frontmatter_json`; recursive/depth-bounded supersession chain/cycle; concurrent recall sequence allocation; and a fixture-based XML baseline test.
- Confidence: High

### Non-blocking simplifications

- Once the recall emission API is moved into `memory_substrate::Substrate`, `crates/memoryd/src/recall/render.rs` can drop local-device loading, manual event/id construction, sequence scanning, direct JSONL path construction, and `doctor_reindex_events_log()` calls. That would make the renderer stay focused on rendering and included-id extraction.
- Consider making `EventsLogMirrorHealth` row-count aware after the multi-device schema fix. A max-sequence-only health check is cheap but cannot detect replacement/deletion when max seq is unchanged.

### Test gaps

- Multi-device canonical JSONL rebuild where two device logs both contain `seq = 1`; assert no loss in `events_log`.
- RecallHit concurrent emission; assert unique per-device sequences and no duplicate event ids/operation ids under parallel startup/delta calls.
- Fail-soft mirror write path; assert JSONL remains canonical, WARN is emitted through tracing, SQLite can lag, and health reports the stale mirror accurately.
- `events_log` covering-index query plan for `kind = 'recall_hit' AND memory_id = ? AND ts > ?`.
- Migration v4 backfill from existing JSONL event files, existing `frontmatter_json.supersedes`, and existing `frontmatter_json.original_confidence`.
- Existing event-kind snapshot/shape preservation for pre-Stream-G variants.
- Supersession recursive-chain/cycle/depth-bound behavior required by later scoring.
- Recall XML byte identity against a committed Stream E baseline fixture, not just equality between two post-change renders.
- Best-effort recall event failure path: append failure should not fail or alter the recall response.

### Questions / uncertainties

- The Stream G spec's example schema uses `seq INTEGER PRIMARY KEY` even though Stream A defines event sequences as per-device. If that schema text is considered authoritative despite the conflict, the spec needs correction; otherwise the implementation should prioritize Stream A's canonical per-device event model.
- I did not run the full workspace gate because this lane is scoped to review and targeted validation; unrelated dirty work exists in the tree.
- I did not inspect future Task 4 doctor wiring beyond noting that Task 2 exposes `events_log_mirror_health()`; doctor surfacing is outside this review scope but depends on the health metric being made robust.

### Positives

- The new `Frontmatter::original_confidence` field is backward-compatible (`Option<f64>` with default/skip-when-none), and the fresh schema/index projection writes it through consistently.
- `memory_supersession` is implemented as a derived wholesale-replaced auxiliary table, matching the existing tags/aliases/entities/evidence projection style.
- Startup and delta recall emission is deduplicated by memory id within a response, and the targeted recall emission tests confirm encrypted/body-disabled included memories still emit RecallHit.
