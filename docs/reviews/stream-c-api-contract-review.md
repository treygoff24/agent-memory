# Stream C API Contract Review

Review lane: Task 13 API contract review.  
Scope: Stream C daemon/MCP protocol compatibility: stable snake_case JSON, bounded response bodies, retryable flags, MCP manifest/admin separation, CLI/MCP separation, and docs matching implemented DTOs.  
Mode: read-only review except this report.

## Summary

- P0: 0
- P1: 4
- P2: 2

MCP admin-tool separation is mostly correct: the manifest declares only the seven agent-facing tools and the tests explicitly reject review/admin names. The main contract risks are that the agent-facing `memory_write` DTO cannot carry the documented metadata, the MCP manifest schemas do not describe what `forward_to_daemon` actually returns, review queue responses are unbounded, and the daemon protocol leaks a kebab-case status in a surface that otherwise promises snake_case JSON.

## Remediation status (2026-04-29)

This report is historical. Local code/docs changes after the review have resolved or partially resolved several findings:

- `memory_write` and `memory_supersede` MCP now accept and forward `meta`; `memory_forget` has no metadata field.
- Review queue responses now apply daemon default/max limits (`50`/`100`).
- Review queue status serialization now uses `pending_review`, while the projector also accepts `pending` and legacy `pending-review`.
- Stream C API/runbook docs and README/CLAUDE Stream C summaries have been updated to describe the implemented write/supersede/forget surface.

Still-current or partially-current items should be revalidated against the latest working tree before treating this report's original P1/P2 list as active blockers.

## Findings

### P1 - `memory_write` MCP request drops the documented `meta` contract

Evidence:

- `docs/specs/system-v0.1.md:929-946` defines agent-facing `memory_write(content, meta?)`, where `meta` carries namespace, type, summary, entities, tags, confidence, sensitivity, evidence, and validity fields.
- `crates/memoryd/src/mcp.rs:68-75` defines the MCP `WriteRequest` as only `body`, optional `title`, and `tags`.
- `crates/memoryd/src/mcp.rs:157-162` forwards every MCP `memory_write` as `RequestPayload::WriteMemory { ..., meta: serde_json::Value::Null }`.
- `crates/memoryd/src/handlers.rs:630-643` shows the daemon governance layer actually supports structured metadata fields such as `namespace`, `type`, `summary`, `confidence`, `sensitivity`, `source_kind`, `source_ref`, and `explicit_user_context`.

Impact:

MCP callers cannot supply the metadata that Stream C needs for policy selection, grounding, privacy fail-closed behavior, or namespace/type correctness. In practice, an agent following the parent MCP contract will send `content/meta`, but the implemented MCP DTO expects `body/title/tags`; even if adapted, `meta` is discarded before reaching governance.

Fix:

Make the MCP tool input match the stable agent-facing contract, or intentionally version the contract. At minimum, add `meta` to `memoryd::mcp::WriteRequest`, decide whether the body field is named `content` or `body`, forward `meta` instead of `Null`, and add a serialization/manifest test that sends representative `meta` through to `RequestPayload::WriteMemory`.

### P1 - MCP output schemas describe bare payloads, but forwarding returns daemon envelopes

Evidence:

- `crates/memoryd/src/mcp.rs:142-181` implements `forward_to_daemon(...) -> Result<ResponseEnvelope>` and returns the daemon response envelope directly for every implemented tool.
- `crates/memoryd/src/protocol.rs:46-50` defines that envelope as `{ id, result }`, where `result` is a tagged success/error object.
- `crates/memoryd/src/mcp.rs:251-270` declares `memory_write`, `memory_supersede`, and `memory_forget` output schemas as bare objects with fields like `status`, `id`, `reason`, `next_actions`, `old_id`, `new_id`, and `tombstone_ref`.
- `crates/memoryd/src/protocol.rs:156-185` defines the actual governance response DTOs with additional fields such as `namespace`, `policy_applied`, `policy_source`, `existing_id`, and optional `chain`.

Impact:

MCP clients and validators will be told one schema but receive another shape. That breaks contract-driven clients, makes generated adapters unreliable, and hides error-envelope fields such as `retryable` from the MCP schema even though the daemon returns them.

Fix:

Choose one boundary: either unwrap daemon envelopes before returning MCP tool results and make descriptors describe the bare payloads, or keep the envelope as the MCP result and make every `output_schema` describe `ResponseEnvelope` including success and error variants. Add tests that compare descriptor schemas to serialized real responses for each tool.

### P1 - Review queue daemon responses are unbounded and can exceed the 64 KiB frame contract

Evidence:

- `crates/memoryd/src/protocol.rs:6-8` sets a single request/response frame cap of 64 KiB.
- `crates/memoryd/src/client.rs:25-30` reads only up to `MAX_FRAME_BYTES` and then immediately decodes the response.
- `crates/memoryd/src/cli.rs:169-176` makes the review queue limit optional with no default.
- `crates/memoryd/src/main.rs:109-117` passes that optional limit directly to `RequestPayload::ReviewQueue`.
- `crates/memoryd/src/handlers.rs:413-439` scans all memory paths, only truncates when `limit` is `Some`, and otherwise serializes every queue item into one response frame.

Impact:

A repo with enough candidate/quarantined memories can produce a valid server response larger than the client-side read cap. The client will then decode a truncated JSON line and surface a transport/decode failure instead of a stable daemon error. This violates the bounded response body contract for daemon JSON, even though MCP does not expose review admin tools.

Fix:

Give `ReviewQueue` a daemon-enforced default and maximum limit, include a `truncated` or `next_cursor` field if needed, and add a contract test that response size remains below `MAX_FRAME_BYTES` for worst-case queue items.

### P1 - Review queue status uses kebab-case `pending-review`, not stable snake_case JSON

Evidence:

- `crates/memory-governance/src/review.rs:45-51` derives serde for `ReviewStatus` with `#[serde(rename_all = "kebab-case")]`.
- `crates/memory-governance/src/review.rs:53-60` maps `PendingReview` to the string `pending-review`.
- `crates/memory-governance/src/review.rs:62-71` recognizes `review_state == "pending-review"` as public review metadata.
- `crates/memoryd/src/handlers.rs:426-437` copies the governance queue item status into the daemon `ReviewQueueItemResponse.status` string.
- `crates/memoryd/src/protocol.rs:192-200` exposes that status string as part of the daemon protocol DTO.

Impact:

The Stream C daemon/MCP lane is otherwise using snake_case serde tags for protocol variants and governance statuses. A public queue status of `pending-review` creates a second enum spelling convention in the JSON contract and will surprise clients expecting snake_case fields/values.

Fix:

Use `pending_review` for protocol-facing status/review_state values. If on-disk frontmatter already uses a historical spelling, normalize at the daemon boundary and preserve backwards-compatible reads internally.

### P2 - `retryable` is too coarse for substrate errors

Evidence:

- `crates/memoryd/src/handlers.rs:126-145` maps `read_memory_envelope` failures in `memory_get` through `HandlerError::substrate`.
- `crates/memoryd/src/handlers.rs:259-271` maps tombstone failures in `memory_forget` through the same substrate wrapper.
- `crates/memoryd/src/handlers.rs:1063-1070` marks every `substrate_error` as `retryable: true`.
- `crates/memoryd/src/protocol.rs:209-214` makes `retryable` part of the stable error DTO.

Impact:

Not all substrate errors are transient. Missing IDs, validation failures, CAS mismatches, tombstoned/not-found reads, and policy-invariant failures should generally be non-retryable or use more specific codes. A blanket `retryable: true` can make clients loop on permanent errors and undermines the value of the stable error contract.

Fix:

Map substrate error variants into daemon error codes and retryability classes. Keep true only for transient I/O/lock/repair-required failures, and add tests for at least invalid ID, missing memory, oversized frame, malformed JSON, and transient substrate failure.

### P2 - Current docs are stale or missing for the implemented Stream C DTOs

Evidence:

- `docs/plans/2026-04-29-stream-c-governance.md:727-752` requires `docs/api/stream-c-governance-api.md` and `docs/runbooks/governance-review.md` to document actual `memory_write`, `memory_supersede`, `memory_forget`, retryability, review queue CLI usage, and why `memory_startup` remains Stream E.
- `CLAUDE.md:11-15` still says Stream B only has Search/Get/Note implemented and that Write/Supersede/Forget return `not_implemented` pending Stream C; it also says Streams C-I have not started.
- `README.md:1-22` still presents the repo as the Stream A workspace and has no current Stream C daemon/MCP API documentation.
- `crates/memoryd/src/mcp.rs:157-170` now forwards `MemoryWrite`, `MemorySupersede`, and `MemoryForget` to governed daemon payloads, so the docs no longer match implementation.

Impact:

A user or agent reading the repo docs will believe Stream C is absent or will use the parent system spec shapes instead of the actual implemented DTOs. That is especially risky because the implemented MCP write input already diverges from `memory_write(content, meta?)`.

Fix:

Land the Stream C API/runbook docs from Task 10 or update the plan status if Task 10 is intentionally deferred. The docs should include exact serialized examples from `RequestEnvelope`, `ResponseEnvelope`, `ToolRequest`, and each governance response DTO, plus the retryable/error-code table.

## Non-findings / positive checks

- MCP admin review tools are not exposed in the manifest. `crates/memoryd/src/mcp.rs:184-199` lists only seven agent-facing names, and `crates/memoryd/tests/mcp_manifest.rs:23-51` rejects admin-like tool names.
- CLI/daemon review commands are separated from MCP: `crates/memoryd/src/cli.rs:153-197` exposes review queue/approve/reject under CLI, while `crates/memoryd/src/mcp.rs:184-199` has no review tool names.
- Search and get bodies are bounded: `crates/memoryd/src/handlers.rs:26-29` defines search/default/get caps, `crates/memoryd/src/handlers.rs:101-123` caps search hits/snippets, and `crates/memoryd/src/handlers.rs:133-145` caps get body previews.
