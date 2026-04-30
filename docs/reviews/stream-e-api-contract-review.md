# Stream E API Contract Review

Date: 2026-04-30
Scope: Review Gate D API contract review for Stream E passive recall.

## Verdict

No P0/P1 API contract findings found.

## Checklist

- Rust protocol DTOs expose `RequestPayload::Startup(StartupRequest)` and `ResponsePayload::Startup(StartupResponse)` using the Stream E request/response fields.
- MCP manifest requires `cwd`, `session_id`, and `harness` for `memory_startup`; the legacy `{ include_recent }` shape no longer deserializes as a complete request.
- CLI examples are backed by `memoryd recall startup-block` and `memoryd recall delta-block`; both use daemon protocol by default.
- `RecallSectionExplanation.omitted_count` and `RecallExplanation.omitted_truncated_count` are present and serialized.
- `RecallOmission.alias` and `RecallOmission.colliding_ids` are JSON-additive: old omission JSON without those keys deserializes and empty/default values skip serialization.
- `StatusResponse.recall` is always serialized on new status responses; legacy status JSON without `recall` deserializes to zero/default counters.
- `stream-e-v0.5` is the active policy/version string in recall output and DTO defaults. Older version strings appear only in historical spec revision text and the plan revision history.
- Stable error codes map through `RecallError`: `invalid_request`, `substrate_error`, `recall_unavailable`, `privacy_error`, `not_implemented`.
- CLI recall errors map to exit codes 1/2/3/4.

## Verification

Passed locally:

```bash
cargo test -p memoryd --test protocol_contract
cargo test -p memoryd --test mcp_manifest
cargo test -p memoryd --test recall_cli
```
