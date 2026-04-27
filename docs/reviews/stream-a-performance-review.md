# Stream A performance review

Status: release-certification candidate.

Performance gate contract:

- `scripts/check.sh` writes smoke results to `bench/results.<profile>.smoke.json`.
- `scripts/check.sh` writes release-tier results to `bench/results.<profile>.json`.
- `scripts/check.sh` runs `scripts/bench-regression-check.sh` against `bench/baseline.<profile>.json`.
- A real `darwin-arm64` baseline is committed from the Stream A bench harness.
- The `linux-x86_64` file remains an explicit `runs: 0` placeholder until the Linux release runner promotes its first real baseline.
- Cross-profile comparisons are forbidden.

Current release-tier corpus contract:

- runs: 9
- corpus size: 10,000
- variants: long bodies, large bodies, aliases, entity aliases, regressions, prospective memories, tombstones, encrypted metadata-only records
- metrics: cold reindex, query by id, filtered metadata query, FTS chunk query, vector chunk query, tree validator

Regression rule:

- A metric regresses iff `current.p95_ms > 1.10 * baseline.p95_ms` and the difference exceeds the baseline metric's `noise_floor_ms`.

Baseline note:

- `scripts/bench-gate.sh` runs `memory-substrate --bin stream_a_bench`; it no longer fabricates fixed metric JSON.
- The harness exercises real `Substrate::init`, `write_memory`, `reindex`, metadata queries, FTS queries, vector updates/queries, and tree validation over a deterministic 10K synthetic corpus.
- The Darwin baseline is active. The Linux placeholder is honest bootstrap state, not a release-certified measurement.
