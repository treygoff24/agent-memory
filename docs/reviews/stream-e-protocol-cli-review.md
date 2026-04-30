# Stream E Protocol/MCP/CLI Review

Date: 2026-04-30
Scope: Review Gate C1 for Tasks 10-11 in `docs/plans/2026-04-30-stream-e-passive-recall.md`.

## Verdict

No P0/P1 findings found in the current protocol/MCP/CLI Stream E wiring.

## Checklist

- MCP legacy shape removed: `memoryd::mcp::StartupRequest` now reuses the Stream E startup DTO with required `cwd`, `session_id`, and `harness`; manifest schema marks those fields required.
- `memory_startup` no longer short-circuits in the MCP forwarder; it forwards `RequestPayload::Startup` to the daemon.
- `since_event_id` is the only intentional startup `not_implemented` path; syntactic request validation runs before that check.
- `StatusResponse.recall` is additive: legacy status JSON deserializes with zero counters, while new serialized status includes `recall`.
- CLI recall commands use the daemon socket path derived from `--runtime` unless `--socket` is explicitly supplied; no direct-substrate fallback is present.
- CLI success stdout is XML only for `recall startup-block` and `recall delta-block`; diagnostics/errors go to stderr.
- `delta-block` no-match emits exactly `<memory-delta empty="true" />`.
- Daemon counters increment through the same in-process state for startup and delta CLI invocations.

## Verification

Passed locally:

```bash
cargo test -p memoryd --test startup_recall_mcp
cargo test -p memoryd --test recall_cli
cargo test -p memoryd --test mcp_forward --test mcp_governance_forward --test mcp_manifest
cargo test -p memoryd --test protocol_contract --test server_smoke
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

## Residual notes

- `memoryd recall ... --repo` is accepted for the hook contract but intentionally not used for direct substrate access; the daemon socket is authoritative in Stream E v0.5.
- The delta implementation is intentionally minimal at this gate and is further covered by later privacy/output/performance acceptance tasks.
