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

## Rule

`--promote-canonical` is a human-review flag. CI, autonomous agents, and unattended scripts should use `--assert` or `.proposed` output mode only.
