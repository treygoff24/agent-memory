# Stream F Final Gate E Performance Review

## Verdict

**Fail for Final Gate E performance.**

No S1 findings. There are S2 findings that block a performance sign-off:

1. The non-updating release benchmark assert command failed in this review run on the cleanup budget.
2. The `memory_observe` benchmark evidence does not certify the durable public write path required by the spec.

The targeted functional tests for cleanup, recall, prompt assembly, observe writes, and leases passed, so this is a performance/evidence gate failure rather than a broad functional failure.

## Findings

### S1

None.

### S2

#### S2-1: Release benchmark assert gate currently fails the cleanup p95 budget.

**Evidence:**

- The Stream F spec requires cleanup full pass p95 over 10k canonical memories and 100k substrate fragments to be `< 60 seconds`: `docs/specs/stream-f-dreaming-v0.2.md:769`.
- Task 15 makes the non-updating assert command the primary release performance gate and requires it to exit nonzero on any v0.2 p95 budget failure: `docs/plans/2026-04-30-stream-f-dreaming.md:869` and `docs/plans/2026-04-30-stream-f-dreaming.md:881`.
- This review run executed:

  ```text
  cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
  ```

  and it failed with:

  ```text
  cleanup_full_pass_representative p95=60434.446ms budget=Some(60000.0) Some(LessThan)
  ```

- The cleanup benchmark is a single full-pass sample, so its reported p95 is one duration: `crates/memoryd/src/bin/stream_f_dream_bench.rs:303-323`. The bench evidence calls this out as `sample_count = 1`: `docs/reviews/stream-f-bench-evidence.md:58`.
- The cleanup implementation does several expensive full-repo/full-index passes in sequence: `run_cleanup` archives fragments, archives candidates, collects findings, rebuilds the entity index, refreshes `observed_at`, and compacts event logs at `crates/memoryd/src/dream/cleanup.rs:47-67`.
- The dominant cleanup shape includes repeated full canonical-memory walks/parses and all-memory index checks: `rebuild_entity_index` queries all memories before and after reindex at `crates/memoryd/src/dream/cleanup.rs:152-158`; `collect_memory_findings` walks/parses canonical memory files at `crates/memoryd/src/dream/cleanup.rs:161-180`; `refresh_observed_at` walks/parses canonical memory files again at `crates/memoryd/src/dream/cleanup.rs:235-260`; `relative_memory_paths` is a repo-wide walk at `crates/memory-substrate/src/tree/layout.rs:158-170`.

**Impact:** Final Gate E cannot certify Stream F performance while the required assert command fails. The current cleanup path also has insufficient margin for a deterministic release gate: the stored evidence says `34,084.036ms`, but the same assert-mode fixture exceeded the budget in this run.

#### S2-2: `memory_observe` p95 is not certified for the durable public write path.

**Evidence:**

- The spec requires `memory_observe` to append through the Stream A atomic-append path with `fsync` per record: `docs/specs/stream-f-dreaming-v0.2.md:517-523`.
- The same spec sets the `memory_observe` p95 budget at `< 5ms`: `docs/specs/stream-f-dreaming-v0.2.md:768`.
- The actual daemon observe path validates the request, classifies privacy, then calls `Substrate::append_substrate_fragment`: `crates/memoryd/src/handlers.rs:407-458`.
- `append_substrate_fragment` writes the substrate record and then records a `SubstrateFragmentWritten` event: `crates/memory-substrate/src/api.rs:722-762`.
- The substrate append path calls `file.sync_all()` and, for full durability, parent-directory fsync: `crates/memory-substrate/src/api.rs:1377-1382`. Event append also calls `file.sync_all()`: `crates/memory-substrate/src/events/log.rs:172-183`.
- The benchmarked observe-write path does not call the public handler or `append_substrate_fragment`; it writes records directly to a `BufWriter`, calls `flush()`, and records `"durability_sync": "not_included_best_effort_fixture"`: `crates/memoryd/src/bin/stream_f_dream_bench.rs:243-281`.
- The bench evidence labels the result as only a throughput fixture: `docs/reviews/stream-f-bench-evidence.md:34`. Its residual risk says calibration through the full append path showed durable `sync_all` tail latency above the 5ms budget: `docs/reviews/stream-f-bench-evidence.md:57`.

**Impact:** The reported `0.050ms` result is not evidence that the public `memory_observe` path meets the v0.2 budget. It excludes the durability and event costs that the spec requires and that production code actually executes.

### S3

#### S3-1: Stream E dream-question overhead is bounded only by fixture size, not by reader-side file limits.

**Evidence:**

- The hot-path requirement is `<= 5ms` added startup p95: `docs/specs/stream-f-dreaming-v0.2.md:770`.
- The reader loads the entire most-recent question file into memory before processing records: `crates/memoryd/src/recall/dream_questions.rs:90-100`.
- It parses every line and runs safe-fragment classification before caps are applied: `crates/memoryd/src/recall/dream_questions.rs:105-129`.
- Caps are applied only after all candidates are collected and sorted: `crates/memoryd/src/recall/dream_questions.rs:79-80` and `crates/memoryd/src/recall/dream_questions.rs:183-201`.
- The release fixture passes with 90 records: `docs/reviews/stream-f-bench-evidence.md:35-36`. The same evidence notes a 240-record calibration exceeded the 5ms budget: `docs/reviews/stream-f-bench-evidence.md:59`.

**Impact:** Normal generated files may stay small, but a large or manually edited question file can push work into the Stream E startup hot path before the 2-per-scope / 6-total output caps help. This is not blocking the current fixture, but it is a fragile hot-path boundary.

#### S3-2: Event compaction is all-in-memory and underrepresented by the fixture.

**Evidence:**

- The cleanup fixture includes only 256 compactable old events: `docs/reviews/stream-f-bench-evidence.md:24`.
- Compaction reads each event log into memory, partitions it, writes archives, and rewrites the live tail: `crates/memoryd/src/dream/cleanup.rs:271-298`.
- Archive handling reads the entire existing zstd archive, appends/dedupes, sorts, and rewrites it: `crates/memoryd/src/dream/cleanup.rs:329-357`.

**Impact:** Current fixture coverage is enough for the task evidence, but event-heavy repos can make compaction CPU/memory cost grow with archive size. This is a follow-up hardening risk, not the current blocking failure.

## Required fixes

1. **Make the release benchmark assert command pass without updating baselines first.**
   - Profile cleanup under the existing fixture.
   - Target the repeated full-tree/index work first: combine canonical memory walks where possible, avoid query-before/query-after full scans if `reindex()` can report rows directly, and avoid reparsing all memories more than necessary.
   - Keep the `< 60s` budget; do not mask the failure by raising thresholds.
   - Rerun:
     ```bash
     cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
     ```

2. **Replace or supplement the observe-write benchmark with a durable public-path measurement.**
   - Measure `memory_observe` through the handler/protocol path, or at minimum `Substrate::append_substrate_fragment` including substrate fsync and `SubstrateFragmentWritten` event append.
   - If the actual durable path cannot meet `< 5ms`, either optimize the dominant sync/event cost or explicitly revise the v0.2 budget/semantics before claiming pass.
   - Only after the assert gate passes should `bench/stream-f-dreaming-results.darwin-arm64.json` be regenerated via the explicit `--write-output` mode.

## Residual risks

- Lease acquisition is certified only against a local bare-origin fixture, not WAN or hosted-remote tail latency: `docs/reviews/stream-f-bench-evidence.md:56`.
- The deterministic bench intentionally excludes real harness/LLM latency, so the 20-minute per-scope daily-run expectation is not certified: `docs/reviews/stream-f-bench-evidence.md:32` and `docs/reviews/stream-f-bench-evidence.md:60`.
- Cleanup behavior tests are green, but performance is close enough to the budget that single-sample p95 evidence is not stable release evidence.
- The review was performed against an already dirty Stream F worktree. I did not modify source files or update baselines; this document is the only review artifact written by this pass.

## Commands run

```bash
git status --short --branch
```

Result: dirty Stream F worktree before review; no source cleanup attempted.

```bash
jq 'keys' bench/stream-f-dreaming-results.darwin-arm64.json
jq '.' bench/stream-f-dreaming-results.darwin-arm64.json | sed -n '1,260p'
```

Result: baseline JSON parsed; fixture has five measurements.

```bash
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
```

Result: **failed**.

```text
Error: current Stream F benchmark failed

Caused by:
    Stream F benchmark budget failures:
    cleanup_full_pass_representative p95=60434.446ms budget=Some(60000.0) Some(LessThan)
```

```bash
cargo test -p memoryd --test dream_cleanup --test dream_recall_integration
```

Result: passed.

```text
dream_cleanup: 10 passed; 0 failed
dream_recall_integration: 9 passed; 0 failed
```

```bash
cargo test -p memoryd --test dream_scope_and_prompts --test dream_substrate_fragments --test dream_lease_election --test dream_lease_scheduled_retry
```

Result: passed.

```text
dream_scope_and_prompts: 3 passed; 0 failed
dream_substrate_fragments: 13 passed; 0 failed
dream_lease_election: 8 passed; 0 failed
dream_lease_scheduled_retry: 6 passed; 0 failed
```

```bash
git status --short --branch
```

Result: source/baseline dirty set unchanged from the pre-review Stream F worktree; review artifact added separately.
