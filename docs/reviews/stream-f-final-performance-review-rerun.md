# Stream F Final Performance Review Rerun

## Verdict

**BLOCK.**

The current non-updating release assertion failed in this rerun. The gate is still suitable as a non-updating assert command, but the current implementation/evidence cannot receive Stream F performance sign-off because:

1. `substrate_fragment_write_memory_observe` missed the `< 5ms` p95 budget.
2. `cleanup_full_pass_representative` missed the `< 60s` p95 budget by a large margin.

No S1 findings were found. The previously questioned Stream E recall overhead fixture is passing in the current assert run, but it remains a bounded-fixture-only certification with an oversized-file hardening risk.

## Findings by severity

### S1

None.

### S2

#### S2-1: The current non-updating Stream F release assert command fails.

**Evidence:**

- The spec requires `memory_observe` p95 `< 5ms`, cleanup p95 `< 60s`, and Stream E dream-question startup overhead `<= 5ms`: `docs/specs/stream-f-dreaming-v0.2.md:765-772`.
- The plan makes this command the primary non-updating release assert gate: `docs/plans/2026-04-30-stream-f-dreaming.md:869-881`.
- The benchmark implementation enforces both the checked-in baseline and the current run in `--assert` mode and only writes in the separate `--write-output` mode: `crates/memoryd/src/bin/stream_f_dream_bench.rs:128-139`.
- This rerun executed:

  ```bash
  cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
  ```

  Result:

  ```text
  Error: current Stream F benchmark failed

  Caused by:
      Stream F benchmark budget failures:
      substrate_fragment_write_memory_observe p95=33.198ms budget=Some(5.0) Some(LessThan)
      cleanup_full_pass_representative p95=103221.231ms budget=Some(60000.0) Some(LessThan)
  ```

**Impact:** Final Stream F performance sign-off is blocked. This is not a stale-baseline-only issue; the command validated the checked-in baseline and then failed the current measurements.

**Required fix:** Make this exact assert command pass without updating `bench/stream-f-dreaming-results.darwin-arm64.json` first. Only after the non-updating assert passes should the baseline be regenerated through the explicit `--write-output` path.

#### S2-2: Cleanup p95 is not stable enough for the release budget.

**Evidence:**

- The checked-in baseline says cleanup p95 is `33,442.129ms` for 10k canonical memories and 100k substrate fragments: `bench/stream-f-dreaming-results.darwin-arm64.json`.
- The prior final performance review recorded a non-updating assert failure at `60,434.446ms`: `docs/reviews/stream-f-final-performance-review.md`.
- This rerun failed at `103,221.231ms`, well above the `< 60,000ms` budget.
- The cleanup benchmark has `sample_count = 1`; its p95 is therefore one large full-pass duration, not a stable distribution: `crates/memoryd/src/bin/stream_f_dream_bench.rs:294-324`.
- The cleanup implementation still performs multiple expensive full-repo/full-index phases in sequence: archive fragments, archive candidates, collect findings, rebuild the index, refresh `observed_at`, compact events, write report, and possibly stage/commit: `crates/memoryd/src/dream/cleanup.rs:42-97`.
- Dominant-looking hot paths remain:
  - full `substrate/` byte snapshots before and after archival to infer changed files: `crates/memoryd/src/dream/cleanup.rs:113-124`, `crates/memoryd/src/dream/cleanup.rs:479-513`;
  - full substrate archive read/partition/sort/rewrite for the 100k-fragment fixture: `crates/memory-substrate/src/api.rs:807-864`;
  - repeated canonical-memory walks/parses for candidate archival, lint findings, reindex, and observed-at refresh: `crates/memoryd/src/dream/cleanup.rs:127-150`, `crates/memoryd/src/dream/cleanup.rs:153-190`, `crates/memoryd/src/dream/cleanup.rs:230-265`, `crates/memory-substrate/src/api.rs:1025-1038`, `crates/memory-substrate/src/api.rs:1666-1708`.

**Impact:** Cleanup cannot be treated as certified. A baseline at ~33s, a prior rerun at ~60s, and this rerun at ~103s against the same command/fixture show the gate is not reproducible with useful margin.

**Required fix:** Profile cleanup under the Task 15 fixture before changing code. Target the dominant full-file/full-tree passes first: avoid whole-substrate snapshots when archival can return exact changed paths, combine canonical memory walks where possible, avoid query-before/query-after full scans if `reindex()` can report rows directly, and avoid reparsing unchanged memory files.

#### S2-3: The observe benchmark now uses the public append API, but the current public-path evidence still fails and still does not certify full-durability `memory_observe`.

**Evidence:**

- The prior direct-`BufWriter` benchmark issue is partially addressed: the current fixture calls `Substrate::append_substrate_fragment`: `crates/memoryd/src/bin/stream_f_dream_bench.rs:243-272`.
- The daemon `memory_observe` handler also routes through `append_substrate_fragment`: `crates/memoryd/src/handlers.rs:419-469`.
- However, the fixture opens the substrate with `force_unsafe_durability = true`: `crates/memoryd/src/bin/stream_f_dream_bench.rs:637-643`.
- In best-effort mode, the substrate append path skips the file and directory `sync_all()` calls that full durability performs: `crates/memory-substrate/src/api.rs:1407-1421`.
- In best-effort mode, event logging uses `append_event_best_effort`; full-durability event logging still uses the fsyncing path: `crates/memory-substrate/src/api.rs:1298-1314`, `crates/memory-substrate/src/events/log.rs:172-199`.
- The spec requires the substrate append to use the Stream A atomic-append pattern with `fsync` per record and to emit a `SubstrateFragmentWritten` event: `docs/specs/stream-f-dreaming-v0.2.md:515-523`.
- This rerun failed the `< 5ms` budget even in the best-effort fixture: `substrate_fragment_write_memory_observe p95=33.198ms`.

**Impact:** The previous "observe benchmark public path" blocker is not cleanly closed. The benchmark now exercises the public append API, but the release evidence still fails the p95 budget and still does not certify production full-durability behavior. If the best-effort public append path misses `< 5ms`, the full-durability `fsync` path is not credibly certified.

**Required fix:** First make the public append fixture pass stably. Then either add a separate full-durability calibration benchmark/report for `memory_observe`, or explicitly revise the spec/evidence language so the release fixture is documented as best-effort throughput only and not full durability certification.

### S3

#### S3-1: Stream E dream-question overhead passes the current fixture, but oversized-file risk remains.

**Evidence:**

- The current assert failure list did not include `stream_e_pending_attention_question_read_overhead`, so the Stream E overhead budget passed in this rerun.
- The checked-in baseline records `3.642ms` added p95 against the `<= 5ms` budget with 90 question records: `bench/stream-f-dreaming-results.darwin-arm64.json`.
- The benchmark compares startup recall with and without dream questions over 21 paired samples: `crates/memoryd/src/bin/stream_f_dream_bench.rs:327-360`.
- The implementation does keep the hook scoped: one most-recent question file per in-scope namespace, no LLM, entity intersection, safe-fragment classification, deterministic sort, then caps: `crates/memoryd/src/recall/dream_questions.rs:50-83`, `crates/memoryd/src/recall/dream_questions.rs:151-159`.
- But the selected file is fully read, every valid line can be parsed/classified, and caps are applied after candidate collection/sort: `crates/memoryd/src/recall/dream_questions.rs:90-139`, `crates/memoryd/src/recall/dream_questions.rs:183-201`.
- The bench evidence itself records that a larger calibration fixture can exceed the 5ms budget: `docs/reviews/stream-f-bench-evidence.md`.

**Impact:** This is not blocking the current rerun because the release fixture passes. It remains a real hot-path fragility if daily question files grow beyond the 90-record fixture or are manually inflated.

**Recommended hardening:** Add an early return when `active_entity_ids` is empty, and consider a configured max bytes/records read per selected question file before classification work begins.

## Reassessment of prior findings

- **Cleanup p95 stability:** Still open and blocking. The sequence `33.442s baseline -> 60.434s prior failed rerun -> 103.221s current rerun` shows the single-sample cleanup benchmark is not stable enough to certify a `< 60s` budget.
- **Observe benchmark public path:** Partially improved but still blocking. The fixture now uses `Substrate::append_substrate_fragment`, but it runs in forced best-effort durability, does not certify the full-durability fsync contract, and failed `< 5ms` in the current run anyway.
- **Stream E recall overhead:** Current release fixture passes. Keep the prior oversized-file concern as S3, not a blocker for this rerun.

## Commands run

```bash
git status --short
```

Result: dirty Stream F worktree existed before this review. Source files were not edited by this review.

```bash
jq '.' bench/stream-f-dreaming-results.darwin-arm64.json
```

Result: parsed the checked-in baseline; all checked-in measurements pass there, including cleanup `33442.129ms`, observe `0.307ms`, and Stream E overhead `3.642ms`.

```bash
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
```

Result: **failed** with current observe p95 `33.198ms` and cleanup p95 `103221.231ms`.

## Residual risks

- The benchmark command currently builds/runs the `dev` profile because the plan-specified command is plain `cargo run`. That is the contract I reviewed, but it contributes to release-gate noise unless the plan/spec intentionally want dev-profile performance gates.
- The lease fixture remains a local bare-origin git fixture, not WAN/hosted-remote latency certification.
- The deterministic bench excludes real harness/LLM latency, so it does not certify the 20-minute per-scope daily-run expectation under provider stalls.
- Event compaction still reads/rewrites archives whole-file in memory. It is not the current failure signal, but it remains a likely future cleanup-tail contributor in event-heavy repos.
