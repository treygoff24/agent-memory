# Stream F Review Gate D: cleanup, recall hook, CLI/status performance review

Status: **PASS**

Scope: Tasks 12-14 only. Reviewed the cleanup layer, Stream E Pass-3 `<pending-attention>` startup hook, and `memoryd dream {status,review,...}` CLI/status surfaces against `docs/specs/stream-f-dreaming-v0.2.md` performance budgets and the Task 13 invariant.

No source files were edited as part of this review.

## Severity findings

No severity-1 or severity-2 performance findings remain.

## Evidence by focus area

### Startup recall pending-attention hook

PASS.

- The hook is integrated into startup recall at `crates/memoryd/src/recall/startup.rs:73-82`, after the normal Stream E index/ranking path has selected recall rows and derived `active_entity_ids`.
- The hook does **not** do a repo-wide scan. It iterates only `namespaces_in_scope`, maps each namespace to one `dreams/questions/<scope_path>/` directory, and selects the most recent `*.jsonl` file with date `<= today` (`crates/memoryd/src/recall/dream_questions.rs:45-70`, `crates/memoryd/src/recall/dream_questions.rs:132-140`).
- File reads are bounded to at most one selected question file per scope. The parser reads that file, parses JSONL records, intersects explicit `entities` with the active seed set, applies the deterministic privacy safe-fragment classifier, and truncates question text to 240 UTF-8 bytes (`crates/memoryd/src/recall/dream_questions.rs:76-122`).
- No LLM, decryption, `memory_reveal`, or Pass 3 rerun occurs in the hook. The code path imports only filesystem/JSON parsing, SHA-256 novelty hashing, XML escaping, and `safe_plaintext_fragment` (`crates/memoryd/src/recall/dream_questions.rs:1-12`, `crates/memoryd/src/recall/dream_questions.rs:108-120`).
- Per-scope and total caps are implemented as 2 per scope and 6 total (`crates/memoryd/src/recall/dream_questions.rs:20-22`, `crates/memoryd/src/recall/dream_questions.rs:164-185`), and omissions are recorded into daemon recall counters after startup success (`crates/memoryd/src/handlers.rs:159-170`, `crates/memoryd/src/recall/counters.rs:53-58`).

Nonblocking performance note: caps are applied after the selected daily file has been fully read, parsed, and privacy-classified. That is acceptable for v0.2 because the scan is one recent file per in-scope namespace, but Task 15's later benchmark should include an oversized question-file fixture to prove the <=5ms p95 overhead target under pathological-but-valid daily JSONL size. A cheap future hardening would be an early return when `active_entity_ids` is empty and/or a configured max records/bytes read per question file.

### Cleanup compaction and archival

PASS.

- Cleanup is an explicit `run_cleanup` operation, not part of `memoryd serve` startup. `memoryd serve` only opens/initializes the substrate and enters the server loop (`crates/memoryd/src/main.rs:23-36`), and the current worker supervisor has no dream cleanup worker (`crates/memoryd/src/workers.rs:7-13`, `crates/memoryd/src/workers.rs:91-153`).
- The cleanup entry point performs its operations and writes a report; it does not run from startup recall (`crates/memoryd/src/dream/cleanup.rs:41-97`).
- Expired substrate archival is delegated to the Stream A substrate primitive and is limited to the current device's `substrate/<device>/` directory (`crates/memory-substrate/src/api.rs:777-858`).
- Cleanup report and commit behavior are explicit and dirty-tree aware (`crates/memoryd/src/dream/cleanup.rs:74-97`, `crates/memoryd/src/dream/cleanup.rs:436-449`).
- The focused cleanup tests cover idempotent fragment archival, stale candidate archival without body deletion, deterministic `observed_at` refresh, monthly zstd compaction, cleanup-bot commit metadata, dirty-tree deferred commits, and two-device convergence (`crates/memoryd/tests/dream_cleanup.rs:18-214`).

Nonblocking performance note: cleanup does multiple full-tree passes over canonical memory files and snapshots substrate bytes before/after fragment archival (`crates/memoryd/src/dream/cleanup.rs:112-124`, `crates/memoryd/src/dream/cleanup.rs:126-159`, `crates/memoryd/src/dream/cleanup.rs:161-190`, `crates/memoryd/src/dream/cleanup.rs:229-263`, `crates/memoryd/src/dream/cleanup.rs:476-510`). This is outside the startup hot path and is acceptable pending Task 15's cleanup full-pass benchmark, but if that benchmark misses the 10k canonical / 100k substrate p95 <60s budget, the byte snapshotting and repeated canonical-memory scans are the first bottlenecks to target.

### zstd event compaction memory/IO behavior

PASS.

- Event compaction only runs inside cleanup, scans `events/*.jsonl`, partitions old/live events, writes monthly archives under `events/archive/<YYYY-MM>.jsonl.zst`, and rewrites the live tail (`crates/memoryd/src/dream/cleanup.rs:271-299`).
- zstd archive handling deduplicates by event id, sorts deterministically, then writes the compressed archive (`crates/memoryd/src/dream/cleanup.rs:301-327`).
- Current implementation uses whole-archive decode/encode (`zstd::stream::decode_all` / `encode_all`) rather than a streaming merge (`crates/memoryd/src/dream/cleanup.rs:329-357`). For the v0.2 operational budget this is acceptable because it is cleanup-only and monthly, but Task 15 should include a large event-archive case before release certification.

### CLI status/review scans

PASS.

- `memoryd dream` is a CLI/admin command family, not an MCP-forwarded agent hot path (`crates/memoryd/src/cli.rs:216-234`, `crates/memoryd/src/main.rs:186-220`).
- Dream status loads config, checks the runtime-local disabled sentinel, builds a small harness inventory, reads active leases, summarizes journal runs, and counts cleanup reports (`crates/memoryd/src/dream/status.rs:36-49`, `crates/memoryd/src/dream/status.rs:101-177`).
- Dream review requires an explicit `--since` window and scans only admin review surfaces: journals, questions, dream candidates, and cleanup reports (`crates/memoryd/src/dream/review.rs:44-55`, `crates/memoryd/src/dream/review.rs:70-194`).
- Admin scans collect and sort matching paths before rendering (`crates/memoryd/src/dream/status.rs:186-207`, `crates/memoryd/src/dream/review.rs:202-228`). That is not startup-path work. Output is bounded by previews (`crates/memoryd/src/dream/review.rs:10`, `crates/memoryd/src/dream/review.rs:256-268`).

Nonblocking performance note: status/review scan inputs are not hard-capped before traversal. This is acceptable for admin-only v0.2 CLI commands, but if repos accumulate very large `dreams/` history, pagination or per-scope/date direct paths would be the right later optimization.

## Commands run

```bash
cargo test -p memoryd --test dream_cleanup --test dream_recall_integration --test dream_cli
```

Result: pass. `dream_cleanup`: 10 passed, 0 failed. `dream_cli`: 7 passed, 0 failed. `dream_recall_integration`: 8 passed, 0 failed.

```bash
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Result: pass.

```bash
scripts/stream-e-recall-bench.sh --quick
```

Result: pass. Current quick smoke baseline on `aarch64`, 50 memories, warm-runs=1:

```json
{
  "cold_start_p95_ms": 61.55475,
  "startup_warm_p95_ms": 55.551083,
  "delta_no_match_p95_ms": 55.410917,
  "delta_five_entity_match_p95_ms": 50.115209
}
```

This is useful context only. It does **not** certify the Stream F pending-attention hook's <=5ms added-overhead budget because the Task 15 Stream F bench fixture is intentionally later work.
