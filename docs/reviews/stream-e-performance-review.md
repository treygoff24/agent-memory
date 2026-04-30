# Stream E Performance Review

Date: 2026-04-30
Scope: Review Gate D performance review for Stream E passive recall.

## Verdict

No P0/P1 performance findings found. The release benchmark passed the Stream E spec latency caps on the recorded `darwin-arm64` profile.

## Release benchmark evidence

Command:

```bash
BENCH_PROFILE=darwin-arm64 bash scripts/stream-e-recall-bench.sh --release | tee bench/stream-e-recall-results.darwin-arm64.json
```

Recorded result file: `bench/stream-e-recall-results.darwin-arm64.json`.

Observed p95s:

- 200 memories: startup warm 12.05ms <= 80ms; cold startup 13.43ms <= 600ms; delta no-match 9.91ms <= 60ms; delta five-entity 9.09ms <= 120ms.
- 1,000 memories: startup warm 15.30ms <= 250ms; cold startup 15.75ms <= 600ms; delta no-match 8.50ms <= 60ms; delta five-entity 8.35ms <= 120ms.

## Checklist

- Bench fixture uses real Stream A writes into a temporary substrate and then exercises Stream E startup/delta assembly.
- Fixture includes active, pinned, candidate, quarantined, passive-recall-disabled, and encrypted-metadata-like (`index_body=false`) rows.
- Release mode enforces spec caps and exits non-zero on any violation.
- Bench JSON records memory count, encrypted metadata-only count, candidate/quarantine count, hardware profile, budget tokens, selected/omitted counts, cold/warm startup p95, and delta p95s.

## Residual notes

- The benchmark intentionally excludes corpus write/setup time from recall latency; it measures request-path startup/delta assembly after the substrate fixture exists.
