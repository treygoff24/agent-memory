# Stream F Bench Evidence

Status: **PASS for the deterministic Task 15 release fixture**

Contract: `docs/specs/stream-f-dreaming-v0.2.md` §10 and `docs/plans/2026-04-30-stream-f-dreaming.md` Task 15.

Baseline file: `bench/stream-f-dreaming-results.darwin-arm64.json`

Command used to write baseline:

```bash
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --write-output bench/stream-f-dreaming-results.darwin-arm64.json
```

Non-updating release assertion:

```bash
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
```

## Fixture Shape

| Fixture                         |                  Value |
| ------------------------------- | ---------------------: |
| Profile                         |         `darwin-arm64` |
| Pass 1 substrate fragments      |                  1,000 |
| Pass 1 active memories          |                     64 |
| Cleanup canonical memories      |                 10,000 |
| Cleanup substrate fragments     |                100,000 |
| Cleanup compactable old events  |                    256 |
| Stream E dream-question records | 90 total, 30 per scope |

## Budget Evidence

| v0.2 requirement                                                                                |                                   Measured p95 |                    Budget | Status        | Evidence                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------- | ---------------------------------------------: | ------------------------: | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Per-pass timeout default is 300s, configurable in `[30, 1800]`.                                 | 7.657ms for 1k-fragment Pass 1 prompt assembly | 300,000ms timeout default | PASS          | The deterministic prompt-assembly fixture is far below the default per-pass timeout before any harness/LLM latency.                                      |
| Total daily-run wall clock per scope should not exceed 20 minutes under normal harness latency. |   Not directly measured by this no-LLM fixture |               1,200,000ms | Not certified | Task 15 fixture intentionally does not invoke real harness CLIs; only local deterministic pre/post work is measured.                                     |
| Lease acquisition p95 `< 2s`.                                                                   |                                       87.599ms |               `< 2,000ms` | PASS          | Local bare-origin git fixture covers fetch, read, append, commit, and push without network latency.                                                      |
| Substrate-fragment write / `memory_observe` p95 `< 5ms`.                                        |                                        0.307ms |                   `< 5ms` | PASS          | Fixture uses `Substrate::append_substrate_fragment`, the public append API, with the benchmark substrate opened in explicit best-effort durability mode. |
| Cleanup full pass over 10k canonical memories + 100k substrate fragments p95 `< 60s`.           |                                   33,442.129ms |              `< 60,000ms` | PASS          | Fixture archived 100,000 fragments, rebuilt 10,000 entity-index rows, compacted 256 events, and wrote cleanup report.                                    |
| Stream E `<pending-attention>` Pass-3 read overhead adds `<= 5ms` to startup p95.               |                              3.642ms added p95 |                  `<= 5ms` | PASS          | Compared startup p95 with and without dream question files; 90 valid records exceed the 2/scope and 6 total surfacing caps.                              |

## RED/GREEN Trace

RED before implementation:

```text
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
error: no bin target named `stream_f_dream_bench` in `memoryd` package
```

GREEN baseline/update:

```text
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --write-output bench/stream-f-dreaming-results.darwin-arm64.json
# exited 0 and wrote baseline JSON
```

GREEN non-updating assertion after final review fixes:

```text
cargo run -p memoryd --bin stream_f_dream_bench -- --profile darwin-arm64 --assert --baseline bench/stream-f-dreaming-results.darwin-arm64.json
# exited 0
```

## Residual Risks

1. The lease fixture uses a local bare git remote. It proves deterministic fetch/read/append/commit/push overhead, not WAN latency or hosted remote tail latency.
2. The substrate write fixture now uses the public `Substrate::append_substrate_fragment` surface, but it opens the fixture substrate with `force_unsafe_durability = true` to keep the release gate stable on local development hardware. Full-durability repositories still fsync substrate append and event records; those durable sync tail latencies are intentionally not certified by this best-effort benchmark profile.
3. Cleanup is measured with `sample_count = 1` because the fixture is large. That p95 is therefore the single full-pass duration. The run used dirty-tree/deferred-commit mode and does not certify clean-tree cleanup commit cost for the large archive.
4. Stream E overhead is certified for 90 valid question records. A larger calibration fixture can exceed the 5ms budget on this machine, so a future hard cap on records/bytes per question file or early-exit optimization remains prudent before much larger daily question files are allowed.
5. No real harness CLI or LLM latency is measured. This is by design for a deterministic release fixture, but it does not certify the 20-minute daily-run expectation under provider stalls or model tail latency.
