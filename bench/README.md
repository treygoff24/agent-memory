# Benchmark promotion flow

Benchmark binaries may assert against checked-in canonical results, but they must not promote new canonical files by default. Normal update runs write `.proposed` files only.

## Stream G observability

```bash
cargo run -p memoryd --bin stream_g_bench -- \
  --profile darwin-arm64 \
  --output bench/stream-g-observability-results.darwin-arm64.json
```

This writes `bench/stream-g-observability-results.darwin-arm64.json.proposed` and leaves the canonical JSON untouched.

After reviewing the proposed diff from a human shell session:

```bash
cargo run -p memoryd --bin stream_g_bench -- \
  --profile darwin-arm64 \
  --output bench/stream-g-observability-results.darwin-arm64.json \
  --promote-canonical

cargo run -p memoryd --bin stream_g_bench -- \
  --profile darwin-arm64 \
  --assert \
  --baseline bench/stream-g-observability-results.darwin-arm64.json
```

## Stream I cross-session coordination

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- \
  --profile darwin-arm64 \
  --output bench/stream-i-cross-session-results.darwin-arm64.json
```

This writes `bench/stream-i-cross-session-results.darwin-arm64.json.proposed` only.

After reviewing the proposed diff from a human shell session:

```bash
cargo run -p memorum-coordination --bin peer_relevance_bench -- \
  --profile darwin-arm64 \
  --output bench/stream-i-cross-session-results.darwin-arm64.json \
  --promote-canonical

cargo run -p memorum-coordination --bin peer_relevance_bench -- \
  --profile darwin-arm64 \
  --assert \
  --baseline bench/stream-i-cross-session-results.darwin-arm64.json
```

## Recall quality baseline (Task 4.2)

`bench/quality-baseline.json` is the committed baseline for the golden-corpus recall quality metrics (precision/recall@K, MRR, nDCG, trap-rate@5). It is **human-committed only**, the same convention as `baseline.*.json` — no tool, CI step, or agent ever writes it.

The runner emits the report to an arbitrary `--output-file` (e.g. `quality-results.json`) for review; it never touches `bench/quality-baseline.json`:

```bash
cargo run -p memorum-eval --bin memorum-eval-quality -- --output-file /tmp/quality.json
```

To establish or update the baseline, a human reviews the emitted JSON and copies it to `bench/quality-baseline.json` in an explicit commit. The gate test (`cargo test -p memorum-eval --test quality_baseline`) **skips cleanly** when the baseline is absent and otherwise fails on a regression beyond the tolerance band.

## Rule

`--promote-canonical` is a human-review flag. CI, autonomous agents, and unattended scripts should use `--assert` or `.proposed` output mode only. The same no-programmatic-write rule covers `bench/quality-baseline.json`.
