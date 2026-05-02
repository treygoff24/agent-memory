# Stream G Review Gate A — Contract/API Review

Scope: Stream G Tasks 2-3 against `docs/specs/stream-g-observability-v0.1.md` §1.3 and §10, `docs/specs/system-v0.2.md` §19, and `docs/plans/2026-05-01-stream-g-observability.md` Tasks 2-3.

## Findings

### P1 — `events_log` cannot faithfully mirror per-device JSONL because `seq` is used as a global primary key

`events_log` is populated from every `events/<device_id>.jsonl` file, but `seq` is per-device in the canonical event log. The mirror schema makes `seq` the sole primary key, and `mirror_event_row` uses `INSERT OR REPLACE` on that key:

- `crates/memory-substrate/src/index/schema.rs:63-71` defines `events_log(seq INTEGER PRIMARY KEY, ...)` with no `device` column.
- `crates/memory-substrate/src/index/query.rs:343-356` inserts by `seq` only with `INSERT OR REPLACE`.
- `crates/memory-substrate/src/api.rs:1392-1408` reads all device JSONL files and sorts by `seq`, confirming the mirror is fed multi-device data.

Two devices will routinely emit `seq = 1`, `seq = 2`, etc. Reindexing or backfilling all device logs will overwrite earlier rows with later rows for the same sequence number. That undercounts `RecallHit` rows and can corrupt drift scoring, recall history, and future Stream I device-attribution consumers.

Contract impact: Stream G §1.3 says JSONL remains canonical and SQLite is a derived projection from each device's JSONL log. A derived projection that drops rows from other devices is not faithful enough for `recall_count_30d`.

Recommended fix: make event identity in the mirror globally unique. The clean contract is to add `device TEXT NOT NULL` and use a composite key like `(device, seq)` or `event_id` as the primary key, then keep the required query index on `(kind, memory_id, ts)`. Because the published spec currently shows `seq INTEGER PRIMARY KEY`, this likely needs a small spec/contract amendment before implementation repair.

### P1 — RecallHit emission bypasses the Stream A event append API and can race duplicate sequence numbers while blocking the recall hot path

`memoryd` manually constructs and appends `RecallHit` events instead of routing through the substrate's central event append path:

- `crates/memoryd/src/recall/render.rs:51-91` reads device config, computes sequence numbers, calls `append_event` directly, then calls `substrate.doctor_reindex_events_log()` synchronously.
- `crates/memoryd/src/recall/render.rs:216-225` computes the next sequence by scanning the current event log and adding one.
- The central Stream A path in `crates/memory-substrate/src/api.rs:1345-1385` already owns event construction, durability handling, JSONL append, and SQLite mirror fail-soft behavior.

This creates two contract/API problems:

1. Concurrent startup/delta recall responses can both read the same max sequence and append duplicate per-device `seq` values. With the current mirror primary key, duplicate seq values also cause replacement in SQLite.
2. Task 3 requires RecallHit emission to be best-effort and not block the recall response. Rebuilding the entire SQLite events mirror after every recall response is synchronous work on the recall path, not a cheap fire-and-forget append.

Recommended fix: expose a small substrate API such as `record_recall_hits(ids: impl IntoIterator<Item = MemoryId>)` or `record_event_best_effort(EventKind)` that uses the existing sequence reservation and `append_event_and_mirror` path. `memoryd` should call that API and only warn on failure. Do not call `doctor_reindex_events_log()` from recall rendering. Add a concurrency test that runs two recall responses concurrently and asserts unique event sequences.

### P2 — Migration v4 does not perform the required `events_log` JSONL backfill

The Task 2 contract requires migration v4 to backfill `events_log` from existing `events/<device_id>.jsonl` files. Current migration v4 creates the table/index and backfills `original_confidence` plus `memory_supersession`, but it does not read JSONL or insert events into `events_log`:

- `crates/memory-substrate/src/index/migrations.rs:124-162` contains all v4 migration work; there is no JSONL reader/backfill path.
- `crates/memory-substrate/tests/migration_v4.rs:1-65` verifies table creation and idempotence, but not the planned v3-to-v4 JSONL event backfill.

`Substrate::open_with_options` currently rebuilds the mirror after opening (`crates/memory-substrate/src/api.rs:1233-1238`), which can mask this in normal daemon startup, but it does not satisfy the explicit migration contract and leaves `index::open_index` migrations incomplete.

Recommended fix: either move the JSONL backfill into a substrate-level migration/open step that has repo-root access and document that as the actual contract, or extend the migration interface so v4 can see the repo events directory. Add the missing test from the plan: seed JSONL events before v4 migration and assert matching `events_log` rows after migration/open.

### P2 — `events_log_mirror_health()` can report healthy while rows are missing

The health helper compares only `MAX(seq)` in JSONL and SQLite:

- `crates/memory-substrate/src/index/query.rs:334-340` returns `lag = jsonl_max_seq.saturating_sub(sqlite_max_seq)`.

If SQLite misses a middle event but later events mirror successfully, both max values match and `lag = 0` even though the mirror is stale. That violates the spec intent that dual-write divergence be observable before drift scoring uses bad data. The current tests clear the whole mirror and check max lag (`crates/memory-substrate/tests/events_log_mirror.rs:56-93`), but they do not simulate a missing middle row.

Recommended fix: track enough identity to detect holes, e.g. compare per-device `(count, max_seq)` or detect missing `(device, seq)` rows when the mirror has a composite key. Add a test where seq 2 is missing while seq 3 exists; health must be unhealthy.

### P3 — Task 2 test coverage is materially thinner than the plan's acceptance contract

The focused tests pass, but several planned contract tests are absent or weaker than specified:

- `event_kind_new_variants.rs` covers new variant serde but does not lock existing pre-Stream-G event JSON shapes.
- `events_log_mirror.rs` does not cover the required covering-index query plan, dual-write failure fail-soft behavior, WARN emission, or missing-row health semantics.
- `memory_supersession_projection.rs` covers basic write/replace and schema, but not migration backfill from existing `frontmatter.supersedes`, FK constraints, or bounded recursive CTE/cycle behavior.
- `migration_v4.rs` does not cover JSONL backfill, supersession backfill from frontmatter, or duplicate-row idempotence beyond version row count.

Recommended fix: add the missing contract tests before advancing to Task 6 scoring, because scoring depends directly on `events_log` and `memory_supersession` correctness.

## Required-check notes

- No `source_count` column was found in production code or API docs. The only Stream G spec references are historical/drop notes, not an implementation column.
- `NotificationEvent`, Reality Check protocol variants, and `MethodNotAllowedOnMcp` were not found in `crates/`; no accidental partial Task 5 protocol work appears present.
- Delta `RecallHit` test is present and not ignored. The focused `cargo test -p memoryd --test recall_hit_emission` run reported `5 passed; 0 ignored`.

## Gates run

```bash
cargo test -p memory-substrate --test event_kind_new_variants --test events_log_mirror --test memory_supersession_projection --test migration_v4 --test frontmatter_original_confidence --test recall_index_row_indexed_at --test recall_index_row_source_device
cargo test -p memoryd --test recall_hit_emission
```

Result: all requested focused tests passed.

## Residual risks

- I did not run full workspace `cargo clippy` or broad Stream A/Stream E regression suites; this was limited to the requested Gate A contract lane.
- The `events_log` primary-key issue may require a spec correction because the current spec schema conflicts with the per-device sequence model it also describes.
