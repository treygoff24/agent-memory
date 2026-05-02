Verdict: Changes requested

# Stream G Final Review Gate E — Performance Review

Date: 2026-05-02
Repository: `/Users/treygoff/Code/agent-memory`
Reviewer lane: Performance
Scope: Review-only. No production code edited.

## Summary

The Stream G bench command currently reports passing numbers for the requested budgets, and its first-run bootstrap behavior does not overwrite the canonical `bench/stream-g-observability-results.darwin-arm64.json` file.

However, the gate is not acceptable as a final performance proof because the benchmark's 10k scoring path does not call the shipped `memoryd::reality_check::score_memories_at` implementation. A temporary out-of-tree probe against the shipped production scoring function on the same 10k-style fixture measured `p95_ms=20894.622`, far above the required `<=500 ms` p95 budget.

There is also a coverage gap in the TUI/web/notification measurements: the bench binary uses bench-local stand-ins for the TUI app, entity graph payload, passive queue, and Slack payload rather than the shipped crates/types. Those measurements are useful smoke data, but they are not final evidence that the production Stream G paths meet their budgets.

## Required findings

### P1 — Production Reality Check scoring misses the 10k <=500ms p95 budget

**Budget:** Stream G spec §12.3 requires score computation for 10,000 memories to complete in `<=500 ms` (`docs/specs/stream-g-observability-v0.1.md:1833-1841`). The final review lane explicitly asks for scoring 10k memories `<=500 ms p95` (`docs/plans/2026-05-01-stream-g-observability.md:1435-1439`).

**Observed production-path measurement:** an out-of-tree, no-repo-edit probe imported the shipped `memoryd::reality_check::score_memories_at`, initialized a temporary Stream A substrate/index, inserted 10,000 active/pinned memories plus 29,994 recall-hit events and 2,499 supersession edges, and ran 5 scoring samples.

```text
rows=10000 scored=10000 recall_events=29994 supersessions=2499
samples_ms=14405.060,20017.765,20080.809,20761.967,20894.622
p95_ms=20894.622
```

That is roughly 41.8x over the 500 ms budget.

**Why the official bench missed it:**

- `stream_g_bench` measures `score_bench_memories_at`, a bench-local implementation (`crates/memoryd/src/bin/stream_g_bench.rs:213-218`, `crates/memoryd/src/bin/stream_g_bench.rs:733-779`), not the shipped `memoryd::reality_check::score_memories_at`.
- The bench-local scoring implementation batches static fields and source counts once for all rows (`crates/memoryd/src/bin/stream_g_bench.rs:741-743`, `crates/memoryd/src/bin/stream_g_bench.rs:815-871`).
- The production scoring implementation still performs per-candidate database lookups inside the 10k loop: `indexed_static_fields(&index, row)?` and `distinct_sources(&index, row)?` are called for every scoring candidate (`crates/memoryd/src/reality_check/scoring.rs:35-56`), and both helpers run SQL queries (`crates/memoryd/src/reality_check/scoring.rs:149-187`).
- The production session handler calls that production scoring path after loading all active/pinned recall rows (`crates/memoryd/src/reality_check/session.rs:172-190`).

**Required change:** move the batched static-field and distinct-source lookup strategy into production scoring, then make the bench call the production scoring function (or a shared production helper) so the 10k gate measures the code users actually run. Re-run the final performance gate after the benchmark and production path converge.

### P2 — Bench-local stand-ins weaken TUI/web/notification performance evidence

The reported TUI, web entity graph, and notification metrics pass their numeric budgets, but they do not exercise the shipped production types/code paths:

- TUI: the bench uses `SyntheticTuiFixture` / `SyntheticTuiApp` and string rendering (`crates/memoryd/src/bin/stream_g_bench.rs:1152-1237`), while the shipped TUI render path is `memoryd_tui::app::App` with ratatui frames (`crates/memoryd-tui/src/app.rs:166-184`) and existing tests render through `ratatui::backend::TestBackend` (`crates/memoryd-tui/tests/panel_render.rs:7-14`).
- Web entity graph: the bench serializes its own `EntityGraphPayload` (`crates/memoryd/src/bin/stream_g_bench.rs:1239-1284`), while the shipped route returns `memoryd_web::routes::entity_graph::EntityGraphResponse` from `entity_graph` (`crates/memoryd-web/src/routes/entity_graph.rs:8-30`, `crates/memoryd-web/src/routes/entity_graph.rs:114-122`).
- Passive notification append: the bench uses `BenchPassiveQueue` (`crates/memoryd/src/bin/stream_g_bench.rs:462-486`, `crates/memoryd/src/bin/stream_g_bench.rs:1407-1439`), while production uses `memoryd::notifications::PassiveQueue` (`crates/memoryd/src/notifications/passive.rs:15-39`) through `NotificationDispatcher::dispatch_event` (`crates/memoryd/src/notifications/dispatcher.rs:27-34`).

**Required change:** either benchmark the shipped production types directly, or extract shared fixtures/helpers so the benchmark cannot drift away from production behavior. At minimum, add production-path perf assertions for:

- actual `memoryd_tui::App` key event -> ratatui `TestBackend` frame render,
- actual `memoryd_web` `/api/entity-graph` serialization at 5,000 nodes,
- actual `PassiveQueue::append_at` / dispatcher passive append path.

## What passed in the current bench harness

The requested command was run:

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
```

It exited `0` via first-run bootstrap because the canonical baseline file is absent. Stderr included:

```text
first run — wrote .proposed; commit as baseline once verified.
```

Selected measurements from that run:

| Measurement                           | Statistic |   Measured |    Budget | Result |
| ------------------------------------- | --------: | ---------: | --------: | ------ |
| `scoring_10k_memories`                |       p95 | 100.786 ms |  <=500 ms | PASS   |
| `top_n_selection_10k`                 |       p95 |   3.853 ms |   <=50 ms | PASS   |
| `session_resume_from_persisted_state` |       p95 |   3.942 ms |  <=100 ms | PASS   |
| `tui_panel_switch`                    |       p95 |   0.002 ms |   <=16 ms | PASS   |
| `tui_detail_modal_open`               |       p95 |   0.002 ms |   <=32 ms | PASS   |
| `tui_entity_typeahead`                |       p95 |  96.171 ms |  <=100 ms | PASS   |
| `web_entity_graph_serialization_5k`   |       p95 |  28.355 ms |  <=200 ms | PASS   |
| `web_status_p99`                      |       p99 |   0.016 ms |   <=50 ms | PASS   |
| `passive_notification_queue_append`   |       p95 |   0.000 ms |    <=1 ms | PASS   |
| `slack_mock_first_dispatch`           |       p95 |   0.012 ms | <=2000 ms | PASS   |

The bench fixture metadata is deterministic in shape:

```json
{
  "run_date": "2026-05-02",
  "scoring_memory_count": 10000,
  "entity_graph_node_count": 5000,
  "tui_panel_count": 8,
  "typeahead_debounce_ms": 96,
  "passive_queue_sample_count": 1001,
  "slack_dispatch_sample_count": 7
}
```

## Baseline and bootstrap behavior

- `bench/stream-g-observability-results.darwin-arm64.json` is absent in this worktree.
- Running the assert command in that state writes `bench/stream-g-observability-results.darwin-arm64.json.proposed` and exits 0, matching the documented bootstrap behavior (`crates/memoryd/src/bin/stream_g_bench.rs:127-135`, `crates/memoryd/src/bin/stream_g_bench.rs:594-607`).
- The assert-mode run did not create or overwrite the canonical `bench/stream-g-observability-results.darwin-arm64.json` file.
- During this review, the pre-existing untracked `.proposed` file was backed up and restored after the command to avoid leaving generated benchmark churn in the worktree.

## Additional verification run

```bash
cargo test -p memoryd --test scoring -q
```

Result: PASS, 20 tests.

```bash
cargo test -p memoryd-tui --test panel_render -q
```

Result: PASS, 11 tests.

```bash
cargo test -p memoryd-web --test api_contract -q
```

Result: PASS, 15 tests.

```bash
cargo test -p memoryd --test notification_channel -q
```

Result: PASS, 2 tests.

These are correctness/shape checks only; they do not close the production-path performance blocker above.

## Recommended fix sequence

1. Refactor production Reality Check scoring to batch:
   - static fields for all candidate ids,
   - 30-day recall summaries,
   - distinct source counts across supersession chains.
2. Make `stream_g_bench` call the production scoring code instead of maintaining `score_bench_memories_at`.
3. Replace the bench-local TUI/web/passive stand-ins with shipped code paths or shared benchmark helpers.
4. Re-run:

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
```

5. Only after production-path numbers pass, promote or capture the canonical baseline via the explicit `--write-output` path requested by the plan.
