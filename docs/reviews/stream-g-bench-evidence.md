# Stream G Task 17 Performance Gate Evidence

Date: 2026-05-02  
Profile: `darwin-arm64`  
Bench binary: `cargo run -p memoryd --bin stream_g_bench`

## TDD / RED-GREEN record

- RED: added `test_score_memories_at_10k_fixture_under_500ms_p95` against the shipped `memoryd::reality_check::score_memories_at` path.
  - Command: `cargo test -p memoryd --test scoring test_score_memories_at_10k_fixture_under_500ms_p95 -- --exact --nocapture`
  - Result before the production batching fix: FAIL, `score_memories_at 10k p95 20.4129755s exceeded 500ms`.
- GREEN: moved the batch lookup strategy into production scoring and reran the same test.
  - Result after fix: PASS, 1 focused test, finished in 2.09s on the first green run.
- Regression suite after fixture cleanup:
  - Command: `cargo test -p memoryd --test scoring`
  - Result: PASS, 21 tests, finished in 2.51s.

## Baseline state

The canonical baseline now exists and was created only through the explicit release/update command:

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --write-output bench/stream-g-observability-results.darwin-arm64.json
```

`bench/stream-g-observability-results.darwin-arm64.json.proposed` was also produced by assert-mode bootstrap while the canonical baseline was absent, preserving the first-run `.proposed` workflow.

## Production-path changes represented by this evidence

- `stream_g_bench` now measures `memoryd::reality_check::score_memories_at` for `scoring_10k_memories`; it no longer carries a divergent bench-local scoring implementation.
- Production scoring now batches over the candidate IDs for:
  - static memory fields (`observed_at` / `created_at`, `original_confidence`, encrypted/metadata-only state),
  - 30-day recall summaries,
  - distinct source-harness counts across bounded supersession chains.
- The passive notification queue benchmark now calls shipped `memoryd::notifications::PassiveQueue::append_at` instead of a bench-local queue clone.

## Measured values: canonical write-output

These values are from `bench/stream-g-observability-results.darwin-arm64.json` created by the explicit `--write-output` command above:

| Area                  |                                                               Measurement | Statistic |   Measured |     Budget | Result |
| --------------------- | ------------------------------------------------------------------------: | --------: | ---------: | ---------: | ------ |
| Reality Check scoring | Production `score_memories_at` over 10,000 indexed active/pinned memories |       p95 | 192.222 ms |   <=500 ms | PASS   |
| Reality Check scoring |                             Top-N sort + take over 10,000 scored memories |       p95 |   3.662 ms |    <=50 ms | PASS   |
| Reality Check session |      Resume from persisted `reality-check-session.json` with 10k item ids |       p95 |   3.039 ms |   <=100 ms | PASS   |
| TUI synthetic         |                                           Panel switch key-event-to-frame |       p95 |   0.001 ms |    <=16 ms | PASS   |
| TUI synthetic         |                                                         Detail modal open |       p95 |   0.002 ms |    <=32 ms | PASS   |
| TUI synthetic         |                                Entity typeahead including debounce window |       p95 |  96.125 ms |   <=100 ms | PASS   |
| Web synthetic         |                 Entity graph serialization with 5,000 nodes / 4,999 edges |       p95 |  24.751 ms |   <=200 ms | PASS   |
| Web synthetic         |                                              Status payload serialization |       p99 |   0.041 ms |    <=50 ms | PASS   |
| Notifications         |                                      Production `PassiveQueue::append_at` |       p95 |   0.000 ms |     <=1 ms | PASS   |
| Notifications         |                                                 Slack/mock first dispatch |       p95 |   0.010 ms | <=2,000 ms | PASS   |

A final non-bootstrap assert against the promoted baseline also passed:

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
```

Selected final assert values:

| Measurement                         | Statistic |   Measured |   Budget | Result |
| ----------------------------------- | --------: | ---------: | -------: | ------ |
| `scoring_10k_memories`              |       p95 | 192.643 ms | <=500 ms | PASS   |
| `passive_notification_queue_append` |       p95 |   0.000 ms |   <=1 ms | PASS   |

## Commands run

```bash
cargo test -p memoryd --test scoring test_score_memories_at_10k_fixture_under_500ms_p95 -- --exact --nocapture
```

Result before fix: FAIL, production scorer p95 `20.4129755s` over 10k.  
Result after fix: PASS.

```bash
cargo test -p memoryd --test scoring
```

Result: PASS, 21 tests.

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
```

Result before canonical baseline existed: PASS via first-run bootstrap, wrote `.proposed`; production scorer p95 `190.653 ms` after the final passive-queue bench update.

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --write-output bench/stream-g-observability-results.darwin-arm64.json
```

Result: PASS. Wrote canonical baseline with production scorer p95 `192.222 ms`.

```bash
cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
```

Result after canonical baseline existed: PASS, non-bootstrap; production scorer p95 `192.643 ms`.

```bash
cargo clippy -p memoryd --bin stream_g_bench --all-targets --all-features -- -D warnings
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo fmt -p memoryd -- --check
```

Result: PASS for all three.

## Residual risks

- TUI measurements still use deterministic synthetic in-process render stand-ins, not the shipped `memoryd_tui::app::App` plus ratatui `TestBackend`. I did not touch `memoryd-tui` to avoid colliding with TUI/web fix lanes.
- Web entity graph and status measurements still serialize bench-local payload shapes, not the shipped `memoryd_web` route response types. I did not touch `memoryd-web` for the same ownership/collision reason.
- Slack measurement still uses a local in-process mock and validates first-dispatch overhead only; it does not measure real Slack network latency or availability.
- The scoring blocker is closed by production-path evidence: the bench and regression test now both exercise `score_memories_at` rather than a bench-local scorer.
