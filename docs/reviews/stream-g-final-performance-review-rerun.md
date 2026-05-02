# Stream G Final Performance Review Rerun

Date: 2026-05-02  
Repository: `/Users/treygoff/Code/agent-memory`  
Reviewer lane: Performance  
Scope: Review-only rerun after the prior Stream G final performance findings. Production/test code was not modified in this pass.

## Verdict

Approved.

No blocking performance findings remain. The prior P1 blocker is resolved in the production Reality Check scoring path, not just in benchmark-local code: production scoring now batches the 10k candidate inputs, the session handler calls that production scorer, the regression test exercises `memoryd::reality_check::score_memories_at`, and the promoted Stream G bench baseline plus this rerun's assert command are under the required budgets.

## Skills loaded

Mandatory skills were loaded before review:

- `clean-code`
- `tdd`
- project-local `rust-engineer`
- `rust-engineer` testing reference for Rust test/benchmark review context

## Review focus

I inspected current code, tests, bench outputs, and docs for:

1. Whether the previous production-path scoring bottleneck was actually fixed.
2. Whether `stream_g_bench` measures shipped production paths where the previous review required it.
3. New risks from batched SQL, TUI/web/notification dispatch measurements, benchmark bootstrap/assert behavior, and baseline/docs claims.

## Evidence inspected

### Performance contract and baseline

- Spec §12 requires TUI, web, Reality Check scoring, and notification budgets. In particular, 10k Reality Check scoring must be `<=500 ms`, top-N `<=50 ms`, session resume `<=100 ms`, web entity graph serialization `<=200 ms`, and passive queue append `<=1 ms` (`docs/specs/stream-g-observability-v0.1.md:1807-1851`).
- Task 17 requires deterministic fixtures, assert mode that does not dirty the tree, canonical baseline updates only via explicit `--write-output`, and no threshold raising to hide regressions (`docs/plans/2026-05-01-stream-g-observability.md:1311-1356`).
- The final review lane specifically asks for bench fixture determinism, 10k scoring `<=500 ms p95`, TUI render budget, web entity graph budget, and passive queue append budget (`docs/plans/2026-05-01-stream-g-observability.md:1435-1439`).
- The canonical baseline exists with `runs: 1`, profile `darwin-arm64`, deterministic fixture shape, and passing values for every measurement (`bench/stream-g-observability-results.darwin-arm64.json:1-175`).

### Production scoring path

- `score_memories_at` now filters candidates once, opens the index once, builds candidate id slices once, and batches all three expensive data inputs before the scoring loop: 30-day recall counts, static indexed fields, and distinct source counts (`crates/memoryd/src/reality_check/scoring.rs:23-39`).
- The per-row loop now only performs in-memory hash lookups and arithmetic before sorting/top-N selection (`crates/memoryd/src/reality_check/scoring.rs:45-68`).
- The batch helpers chunk SQL parameters at 500 ids and avoid the old per-candidate query pattern (`crates/memoryd/src/reality_check/scoring.rs:137-254`).
- The production session handler still obtains active/pinned/passive recall rows and calls `score_memories_at` directly (`crates/memoryd/src/reality_check/session.rs:172-190`), so the optimized path is the shipped daemon path.
- The 10k production-path regression test calls `score_memories_at` against a fixture with 10,000 memories, recall-hit events, supersession edges, and a `<=500 ms` p95 assertion (`crates/memoryd/tests/scoring.rs:306-328`).

### Benchmark path

- `stream_g_bench` imports `memoryd::reality_check::score_memories_at` and `memoryd::notifications::PassiveQueue` from shipped code (`crates/memoryd/src/bin/stream_g_bench.rs:14-15`).
- The `scoring_10k_memories` measurement calls `score_memories_at(&fixture.rows, &fixture.substrate, ...)` and records `implementation_path = "memoryd::reality_check::score_memories_at"` (`crates/memoryd/src/bin/stream_g_bench.rs:209-228`).
- The scoring fixture inserts 10,000 indexed rows plus 29,994 recall hits and 2,499 supersession edges into the real Stream A index tables used by production scoring (`crates/memoryd/src/bin/stream_g_bench.rs:661-791`).
- The passive queue benchmark now calls shipped `PassiveQueue::append_at` (`crates/memoryd/src/bin/stream_g_bench.rs:464-484`), and production dispatch always appends to the same passive queue before optional OS/external channels (`crates/memoryd/src/notifications/dispatcher.rs:27-34`; `crates/memoryd/src/notifications/passive.rs:29-35`).
- Assert mode validates baseline schema/profile/fixture/measurement contract and enforces budgets on both baseline and current report when a real baseline exists (`crates/memoryd/src/bin/stream_g_bench.rs:128-143`, `crates/memoryd/src/bin/stream_g_bench.rs:629-650`).

## Commands run in this rerun

```bash
cargo test -p memoryd --test scoring test_score_memories_at_10k_fixture_under_500ms_p95 -- --exact --nocapture
```

Result: PASS. `1 passed; 0 failed; 20 filtered out; finished in 1.96s`.

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
```

Result: PASS, non-bootstrap assert against the promoted canonical baseline.

Current assert measurements from this rerun:

| Measurement                           | Statistic | Current rerun |    Budget | Result |
| ------------------------------------- | --------: | ------------: | --------: | ------ |
| `scoring_10k_memories`                |       p95 |    206.071 ms |  <=500 ms | PASS   |
| `top_n_selection_10k`                 |       p95 |      3.854 ms |   <=50 ms | PASS   |
| `session_resume_from_persisted_state` |       p95 |      3.402 ms |  <=100 ms | PASS   |
| `tui_panel_switch`                    |       p95 |      0.001 ms |   <=16 ms | PASS   |
| `tui_detail_modal_open`               |       p95 |      0.002 ms |   <=32 ms | PASS   |
| `tui_entity_typeahead`                |       p95 |     96.139 ms |  <=100 ms | PASS   |
| `web_entity_graph_serialization_5k`   |       p95 |     26.582 ms |  <=200 ms | PASS   |
| `web_status_p99`                      |       p99 |      0.017 ms |   <=50 ms | PASS   |
| `passive_notification_queue_append`   |       p95 |      0.000 ms |    <=1 ms | PASS   |
| `slack_mock_first_dispatch`           |       p95 |      0.012 ms | <=2000 ms | PASS   |

Note: the bench command initially waited on Cargo's build-directory lock because other Cargo commands were active; that wait was outside the benchmarked measurements and the command completed successfully.

## Prior blocker status

### P1 — production Reality Check scoring missed the 10k budget

Resolved.

The old issue was bench/prod drift: the benchmark had a fast scorer while production still did per-candidate SQL and measured around 20 seconds p95. Current production code batches the expensive data inputs (`crates/memoryd/src/reality_check/scoring.rs:23-39`, `crates/memoryd/src/reality_check/scoring.rs:137-254`), the daemon session path uses that scorer (`crates/memoryd/src/reality_check/session.rs:172-190`), and both the targeted regression test and the bench assert exercise `memoryd::reality_check::score_memories_at`.

The canonical baseline reports 192.222 ms p95 for 10k scoring (`bench/stream-g-observability-results.darwin-arm64.json:20-36`), and this rerun measured 206.071 ms p95, still comfortably inside the 500 ms budget.

### P2 — bench-local stand-ins weakened non-scoring performance evidence

Mostly resolved for the prior blocking production-path concern, with residual non-blocking caveats.

- Scoring is no longer bench-local.
- Passive queue append is no longer bench-local.
- TUI and web measurements are still explicitly synthetic / serialization-focused in the bench evidence and baseline (`bench/stream-g-observability-results.darwin-arm64.json:67-127`; `docs/reviews/stream-g-bench-evidence.md:108-113`). This matches the Task 17 plan's wording for synthetic TUI budget coverage (`docs/plans/2026-05-01-stream-g-observability.md:1330-1334`), so I am not treating it as a final-gate blocker for this performance rerun.

## Residual risks and non-blocking recommendations

1. **TUI/web benchmark coverage is still not end-to-end production UI proof.** The current bench measures synthetic TUI work and bench-local web payload serialization. That is acceptable for this gate because the plan explicitly calls out synthetic TUI measurement and server-side serialization, and the evidence doc records the residual risk. A future hardening pass should add optional benchmarks around `memoryd_tui::App` rendered through `ratatui::backend::TestBackend` and the actual `memoryd_web::routes::entity_graph::EntityGraphResponse` route payload.
2. **Entity typeahead has very little budget headroom.** The benchmark includes a 96 ms debounce inside a 100 ms budget, so any real daemon query or config drift can consume the remaining margin quickly (`crates/memoryd/src/bin/stream_g_bench.rs:35-45`, `crates/memoryd/src/bin/stream_g_bench.rs:377-405`). This is not blocking because the measured current bench passes and the implementation currently treats this as a synthetic budget fixture, but it is the first place I would harden if dogfood shows UI search lag.
3. **Distinct-source scoring should be watched under deeper/fan-out supersession histories.** The recursive CTE is depth-bounded and chunked, which closes the prior 10k blocker, but the deterministic fixture has a simple supersession pattern. If real corpora develop denser supersession graphs, add a second stress fixture before changing the query shape.

## Files inspected

- `docs/reviews/stream-g-final-performance-review.md`
- `docs/reviews/stream-g-bench-evidence.md`
- `docs/specs/stream-g-observability-v0.1.md`
- `docs/plans/2026-05-01-stream-g-observability.md`
- `docs/api/stream-g-observability-api.md`
- `docs/dev/stream-g-architecture.md`
- `README.md`
- `CLAUDE.md`
- `bench/stream-g-observability-results.darwin-arm64.json`
- `crates/memoryd/src/reality_check/scoring.rs`
- `crates/memoryd/src/reality_check/session.rs`
- `crates/memoryd/tests/scoring.rs`
- `crates/memoryd/src/bin/stream_g_bench.rs`
- `crates/memoryd/src/notifications/passive.rs`
- `crates/memoryd/src/notifications/dispatcher.rs`
- `crates/memoryd-web/src/routes/entity_graph.rs`
- `crates/memoryd-tui/src/app.rs`
- `crates/memoryd-tui/src/config.rs`
- `crates/memoryd-tui/src/panels/entities.rs`
- `crates/memoryd-tui/tests/panel_render.rs`
- `crates/memory-substrate/src/index/schema.rs`
- `crates/memory-substrate/src/index/migrations.rs`
- `crates/memory-substrate/tests/events_log_mirror.rs`
- `crates/memory-substrate/tests/memory_supersession_projection.rs`
