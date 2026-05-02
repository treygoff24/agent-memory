# Stream I Task 21 Peer Relevance Bench Evidence

Date: 2026-05-02  
Profile: `darwin-arm64`  
Bench binary: `cargo run -p memorum-coordination --bin peer_relevance_bench`

## TDD / RED-GREEN record

- RED: `cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json`
  - Exit: 101
  - Failure: no bin target named `peer_relevance_bench` in the `memorum-coordination` package.
- GREEN/bootstrap: the same assert command exited 0 after implementation and wrote `bench/stream-i-cross-session-results.darwin-arm64.json.proposed` while the canonical baseline file was absent.
- GREEN/canonical: after review, the explicit release/update command wrote `bench/stream-i-cross-session-results.darwin-arm64.json`, and a non-bootstrap assert against that canonical baseline passed.

## Fixture shape

| Field                           |                      Value |
| ------------------------------- | -------------------------: |
| Peer-write candidates           |                        100 |
| Within recency window           |                         50 |
| Outside recency window          |                         50 |
| Session salient entities        |                         10 |
| Session salient paths           |                         10 |
| Precomputed embedding dimension |                         16 |
| Repetitions                     |                        301 |
| Timing unit                     | milliseconds per candidate |

The fixture uses fixed entity ids, fixed path ids, fixed timestamps, and precomputed in-memory embeddings. Timing starts immediately before the relevance gate evaluates the in-memory candidate slice and stops after scoring, filtering, sorting, capping, and cooldown recording complete. Embedding-worker wait time is intentionally excluded.

## Baseline state

The canonical baseline now exists and was created only through the explicit release/update command:

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --write-output bench/stream-i-cross-session-results.darwin-arm64.json
```

`bench/stream-i-cross-session-results.darwin-arm64.json.proposed` also remains as first-run bootstrap evidence from assert mode when the canonical baseline was absent.

## Measured values

These values are from `bench/stream-i-cross-session-results.darwin-arm64.json` created by the explicit `--write-output` command above:

| Measurement                       |         p50 |         p95 |         p99 |     Budget | Result |
| --------------------------------- | ----------: | ----------: | ----------: | ---------: | ------ |
| Peer relevance gate per candidate | 0.005927 ms | 0.006917 ms | 0.007670 ms | <=5 ms p95 | PASS   |

A final non-bootstrap assert against the promoted baseline also passed with p50 0.005687 ms, p95 0.006647 ms, and p99 0.007426 ms.

## Commands run

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json
```

Initial result before canonical baseline existed: PASS via first-run bootstrap. Stderr included:

```text
first run — wrote .proposed; commit as baseline once verified.
```

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --write-output /tmp/stream-i-cross-session-results.test.json
```

Result: PASS. Wrote `/tmp/stream-i-cross-session-results.test.json` and did not modify the canonical repo baseline.

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --write-output bench/stream-i-cross-session-results.darwin-arm64.json
```

Result: PASS. Wrote canonical baseline with p95 0.006917 ms.

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- --profile darwin-arm64 --assert --baseline bench/stream-i-cross-session-results.darwin-arm64.json
```

Result after canonical baseline existed: PASS, non-bootstrap; p95 0.006647 ms.

```bash
cargo clippy -p memorum-coordination --bin peer_relevance_bench -- -D warnings
```

Result: PASS after the Stream I framing helper arity cleanup.

```bash
cargo clippy -p memorum-coordination --all-targets --all-features -- -D warnings
```

Result: PASS after the Stream I framing helper arity cleanup.

```bash
cargo fmt -p memorum-coordination -- --check
```

Result: PASS.

## Residual risks

1. The timing fixture is in-memory and deterministic. It certifies the relevance gate computation path, not daemon IPC, SQLite query time, recall XML rendering, or terminal/browser latency.
2. Embedding-worker wait time is excluded by design because Task 21 measures candidate-read-to-score-computed latency with precomputed embeddings.
3. The first-run `.proposed` file remains useful historical evidence, but the release gate should assert against the canonical `bench/stream-i-cross-session-results.darwin-arm64.json` baseline.
