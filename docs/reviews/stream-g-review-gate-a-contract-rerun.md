# Stream G Review Gate A — Contract/API Rerun

Scope: rerun after fixes for the prior Stream G Gate A contract/API review and clean-code review. Review-only lane; no production code edited.

## Verdict

Changes requested.

The prior contract blockers are substantively closed: the `events_log` mirror no longer collapses same-sequence multi-device events, RecallHit emission now routes through a Stream A substrate API using the central sequence allocator and incremental mirror hook, mirror health detects middle-row holes, and the requested focused contract tests are present and passing.

However, the broader all-target Rust gate still fails to compile because several existing `memory-substrate` test fixtures construct `Frontmatter` without the newly added `original_confidence` field. This is not a runtime contract defect in the fixed Stream G surfaces, but it is a release-blocking API/test fallout from the additive public struct field.

## Remaining findings

### P1 — `memory-substrate` all-target gate fails after adding `Frontmatter::original_confidence`

Evidence:

- `crates/memory-substrate/src/model.rs:459-475` adds public `Frontmatter::original_confidence: Option<f64>`.
- `cargo clippy -p memory-substrate -p memoryd --all-targets --all-features -- -D warnings` fails to compile several existing test targets with `E0063: missing field original_confidence in initializer of memory_substrate::Frontmatter`.
- Reported locations from the gate:
  - `crates/memory-substrate/tests/api_phase5_surface.rs:268`
  - `crates/memory-substrate/tests/crash_matrix.rs:73`
  - `crates/memory-substrate/tests/startup_reconciliation.rs:482`
  - `crates/memory-substrate/tests/vector_lifecycle.rs:238`
  - `crates/memory-substrate/tests/reindex_reconciliation.rs:220`

Why it matters: the focused Stream G tests pass, but an all-target crate gate still cannot compile. Until these fixtures are updated (or a construction helper/defaulting seam is introduced), the workspace remains red for normal Rust review gates.

Recommended fix: update the listed `Frontmatter` struct literals to include `original_confidence: None`, or route tests through a shared fixture constructor so future additive frontmatter fields do not cause repeated test-wide API fallout.

Confidence: High.

## Prior finding closure

### Closed — `events_log` mirror preserves multi-device same-seq events

- Fresh schema now uses `event_id TEXT PRIMARY KEY`, with `device TEXT NOT NULL` and `seq INTEGER NOT NULL`, rather than `seq INTEGER PRIMARY KEY` (`crates/memory-substrate/src/index/schema.rs:63-73`, `crates/memory-substrate/src/index/migrations.rs:130-140`).
- Mirror writes upsert by `event_id` and persist `device` plus `seq` (`crates/memory-substrate/src/index/query.rs:354-369`).
- Rebuild reads all device JSONL files and sorts by `(device, seq, event_id)` (`crates/memory-substrate/src/api.rs:1402-1425`).
- Regression coverage exists in `doctor_reindex_preserves_multi_device_events_with_same_sequence` (`crates/memory-substrate/tests/events_log_mirror.rs:111-138`).

### Closed — RecallHit emission uses Stream A allocator and incremental mirror; no full reindex in recall hot path

- `memoryd` recall rendering calls `substrate.record_recall_hit(id)` and no longer constructs events or paths itself (`crates/memoryd/src/recall/render.rs:46-57`).
- `Substrate::record_recall_hit` delegates to `record_event_best_effort(EventKind::RecallHit { ... })` (`crates/memory-substrate/src/api.rs:1194-1208`).
- `record_event_best_effort` synchronizes/reserves sequence state through the central Stream A sequence path and then calls `append_event_and_mirror`, which mirrors one event incrementally (`crates/memory-substrate/src/api.rs:1196-1203`, `crates/memory-substrate/src/api.rs:1339-1343`, `crates/memory-substrate/src/api.rs:1374-1381`).
- The recall module no longer calls `doctor_reindex_events_log()`; remaining uses are substrate open/reindex helpers only (`rg "doctor_reindex_events_log\(|rebuild_events_log_mirror\(" crates/memoryd/src crates/memory-substrate/src`).
- Concurrency coverage exists in `test_concurrent_recall_emission_uses_unique_central_sequences` (`crates/memoryd/tests/recall_hit_emission.rs:116-150`).

### Closed — mirror health detects missing middle rows

- `EventsLogMirrorHealth` now includes `jsonl_count`, `sqlite_count`, and `missing_count` in addition to max-seq lag (`crates/memory-substrate/src/model.rs:79-93`).
- Health computes missing canonical events by `event_id` (`crates/memory-substrate/src/index/query.rs:333-383`).
- Regression coverage deletes a middle event while max sequence still matches and asserts `missing_count == 1` (`crates/memory-substrate/tests/events_log_mirror.rs:140-170`).

### Closed — requested focused contract tests are present

Confirmed coverage:

- Covering-index query plan: `recall_hit_drift_query_uses_kind_memory_ts_index` (`crates/memory-substrate/tests/events_log_mirror.rs:172-201`).
- Open JSONL backfill: `open_rebuilds_v4_events_log_from_existing_jsonl` (`crates/memory-substrate/tests/migration_v4.rs:62-94`).
- Bounded/cyclic supersession CTE: `recursive_supersession_cte_is_bounded_across_cycles` (`crates/memory-substrate/tests/memory_supersession_projection.rs:41-59`).
- Concurrent recall sequence uniqueness: `test_concurrent_recall_emission_uses_unique_central_sequences` (`crates/memoryd/tests/recall_hit_emission.rs:116-150`).
- Delta RecallHit test is present and not ignored: `test_delta_recall_emits_recall_hit_per_memory` (`crates/memoryd/tests/recall_hit_emission.rs:79-114`), and `rg "#\[ignore\]|ignore\s*="` found no ignore markers in the relevant test files.

### Closed — no `source_count` implementation column added

`rg -n "source_count" crates docs/api -S` returned no matches. Remaining `source_count` references are only spec/plan prose noting the dropped earlier-draft column.

## Gates run

```bash
cargo test -p memory-substrate --test event_kind_new_variants --test events_log_mirror --test memory_supersession_projection --test migration_v4 --test frontmatter_original_confidence --test recall_index_row_indexed_at --test recall_index_row_source_device && cargo test -p memoryd --test recall_hit_emission
```

Result: passed.

- `event_kind_new_variants`: 3 passed; 0 ignored.
- `events_log_mirror`: 6 passed; 0 ignored.
- `frontmatter_original_confidence`: 3 passed; 0 ignored.
- `memory_supersession_projection`: 3 passed; 0 ignored.
- `migration_v4`: 4 passed; 0 ignored.
- `recall_index_row_indexed_at`: 1 passed; 0 ignored.
- `recall_index_row_source_device`: 1 passed; 0 ignored.
- `recall_hit_emission`: 6 passed; 0 ignored.

```bash
cargo fmt --all -- --check
```

Result: passed.

```bash
cargo clippy -p memory-substrate -p memoryd --all-targets --all-features -- -D warnings
```

Result: failed to compile with `E0063` missing `original_confidence` fields in existing `memory-substrate` test fixtures; see P1 above.

## Residual risks

- I did not run a full workspace test suite after the clippy compile failure because the all-target compile issue is already blocking.
- The Stream G/system spec prose still contains the older illustrative `events_log(seq INTEGER PRIMARY KEY, ...)` schema in `docs/specs/*`; `docs/api/stream-a-public-api.md` has the corrected `event_id`/`device` contract. If the specs are intended to stay executable contract rather than historical planning prose, align those snippets before handoff.
