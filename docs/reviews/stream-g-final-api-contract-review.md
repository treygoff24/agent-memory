Verdict: Changes requested

# Stream G Final Review Gate E API Contract Review

Review scope: protocol/CLI/web route docs versus shipped DTOs; `ComponentScores` wire field names; `NotificationEvent` seven-variant contract; web JSON shapes from spec §4.3; Reality Check/admin protocol MCP rejection; Task 18 docs versus code.

## Blockers

### 1. Task 18 API docs do not match the shipped Reality Check protocol/CLI DTOs

`docs/api/stream-g-observability-api.md` documents a CLI and daemon protocol that are not the shipped Rust contract:

- The doc advertises `memoryd reality-check reset` as a CLI command at `docs/api/stream-g-observability-api.md:85-90`, but the shipped CLI enum only exposes `Run`, `Skip`, and `Snooze` at `crates/memoryd/src/cli.rs:118-126`. There is no `Reset` CLI subcommand or `reality_check_request_payload` branch for reset; the shipped mapping covers only JSON/list run, interactive run, skip, and snooze at `crates/memoryd/src/cli.rs:710-729`.
- The doc's daemon protocol omits shipped fields/variants: it shows `Run { session_id, namespace }`, `Snooze`, and `Reset` at `docs/api/stream-g-observability-api.md:109-116`, while the shipped DTO is `Run { session_id, namespace, limit }`, `Skip`, `Snooze { until }`, and `Reset` at `crates/memoryd/src/protocol.rs:149-158`.
- The CLI docs therefore also drift from shipped CLI behavior for snooze: shipped `memoryd reality-check snooze --until <date>` parses into `RealityCheckRequest::Snooze { until: Some(DateTime<Utc>) }` at `crates/memoryd/src/cli.rs:722-727`, not the fieldless `Snooze` shape documented at `docs/api/stream-g-observability-api.md:113-115`.

Impact: downstream users and future agents will implement the wrong wire shape from the docs. This is an API-contract blocker even though the Rust DTO tests pass.

### 2. Web `GET /api/audit/:id` JSON shape does not match spec §4.3

Spec §4.3 defines `GET /api/audit/:id` as a top-level `AuditMemoryResponse` containing fields such as `memory_id`, `title`, `body`, `status`, `namespace`, `confidence`, `recall_count_total`, `recall_count_30d`, `last_recalled`, `provenance_chain`, `policy_decisions`, `privacy_scan`, `supersession_history`, and `sync_state` at `docs/specs/stream-g-observability-v0.1.md:721-752`.

The shipped web response instead wraps the daemon trust artifact under an `artifact` object and adds a `sections` object: `AuditMemoryResponse { memory_id, artifact, sections }` at `crates/memoryd-web/src/routes/audit.rs:11-16`, returned by the route at `crates/memoryd-web/src/routes/audit.rs:70-77` and `crates/memoryd-web/src/routes/audit.rs:89-92`. The underlying `TrustArtifact` DTO also uses different field names from the spec example, e.g. `id`, `current_confidence`, `recall`, `supersedes`, and `superseded_by` at `crates/memoryd/src/trust_artifact.rs:79-99`, not top-level `confidence`, `recall_count_total`, `recall_count_30d`, `last_recalled`, and `supersession_history`.

Impact: a client written to the spec §4.3 web contract will fail against the shipped `/api/audit/:id` route. Either the route must be adapted to the spec shape or the spec/API docs need an explicit versioned contract change before this gate can approve.

### 3. Task 18 docs understate the shipped web audit shape instead of documenting the actual contract

Task 18's Stream G API doc only says `GET /api/audit/:id` returns a “full trust artifact” at `docs/api/stream-g-observability-api.md:70-73` and later summarizes trust artifact fields at `docs/api/stream-g-observability-api.md:153-157`. It does not document the actual shipped JSON envelope `{ memory_id, artifact, sections }` from `crates/memoryd-web/src/routes/audit.rs:11-16`, nor does it reconcile that envelope with spec §4.3's top-level audit shape at `docs/specs/stream-g-observability-v0.1.md:721-752`.

Impact: the route docs are not sufficient as a stable API contract. This compounds blocker #2 because neither the spec nor the Task 18 API doc accurately describes the shipped response shape.

## Confirmed contract points

- `ComponentScores` field names match spec §5.7. The spec requires `days_since_observed_norm`, `recall_frequency_norm`, `cross_source_corroboration`, `confidence_decay`, and `sensitivity_weight` at `docs/specs/stream-g-observability-v0.1.md:1109-1115`; the shipped DTO has the same fields at `crates/memoryd/src/protocol.rs:307-314`; the scoring code populates the same fields at `crates/memoryd/src/reality_check/scoring.rs:92-100`; and the protocol test asserts the serialized JSON keys at `crates/memoryd/tests/protocol_contract.rs:207-238`.
- `NotificationEvent` has exactly the seven spec variants. The spec lists seven variants at `docs/specs/stream-g-observability-v0.1.md:99-110`; the shipped enum matches at `crates/memoryd/src/protocol.rs:332-340`; and the notification-channel test constructs/sends/receives those seven variants at `crates/memoryd/tests/notification_channel.rs:8-35`.
- Reality Check/admin protocol MCP rejection is implemented for shipped admin/UI payloads. `forward_payload_to_daemon` rejects `TrustArtifact`, `WebEnable`, `WebDisable`, `WebStatus`, `RealityCheck(_)`, and peer-state payloads before socket I/O at `crates/memoryd/src/mcp.rs:223-242`; the stable `method_not_allowed_on_mcp` error is defined as non-retryable at `crates/memoryd/src/protocol.rs:782-805`; tests cover web/peer admin rejection at `crates/memoryd/tests/mcp_manifest.rs:70-111` and Reality Check rejection at `crates/memoryd/tests/notification_channel.rs:37-52`.
- Web route registration broadly matches the spec §4.3 route table: status, entity graph/detail, ROI, reality check, reality-check history, audit, audit walk, audit temporal, review, and the POST routes are registered at `crates/memoryd-web/src/server.rs:173-198`. This does not clear blocker #2 because `/api/audit/:id`'s JSON body shape still diverges from spec §4.3.

## Verification run

Executed on 2026-05-02:

```bash
cargo test -p memoryd --test protocol_contract --test notification_channel --test cli_contract
cargo test -p memoryd-web --test api_contract
```

Result: all selected tests passed (18 CLI tests, 2 notification tests, 16 protocol-contract tests, 15 web API tests). Passing tests do not change the verdict because the blockers are spec/docs-vs-shipped-contract drift that current tests either encode or do not assert against the normative spec shape.
