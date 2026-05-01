### Verdict

PASS

### Intended outcome

This Gate B rerun verifies the scoped Stream F observe/protocol/MCP fixes after the prior clean-code review: `memory_observe` should expose substrate-fragment capture without changing `memory_note`, the MCP manifest should match the daemon DTOs, omitted observe entities should be valid and default to `[]`, observe caller-binding fields should forward consistently through MCP/protocol/handler, and admin dream controls must stay out of MCP.

### Executive summary

No material issues found. The prior API-contract finding on defaulted `entities` is fixed: the manifest no longer requires `entities`, the MCP DTO defaults omitted entities to an empty vector, and docs now call the field optional. The prior output-schema drift is also fixed: MCP output schema requires exactly the `ObserveResponse` fields. `memory_note` remains a canonical-note-only MCP surface and rejects `kind`; `memory_observe` carries the additive caller-binding fields consistently through manifest, DTO, daemon payload, handler validation, and substrate append. No severity-1/2 findings remain.

### Findings

No material issues found. No severity-1/2 findings remain.

### Confirmation evidence

- `memory_observe` manifest no longer requires defaulted `entities`, and omitted entities default to `[]`.
  - Evidence: `crates/memoryd/src/mcp.rs:117-118` marks `ObserveRequest.entities` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
  - Evidence: `crates/memoryd/src/mcp.rs:376-399` defines the `entities` schema but the required array is only `["text", "kind", "cwd", "session_id", "harness"]` at `crates/memoryd/src/mcp.rs:397`.
  - Evidence: `crates/memoryd/tests/mcp_manifest.rs:67` asserts that exact required array, and `crates/memoryd/tests/mcp_manifest.rs:84-102` parses a `memory_observe` request without `entities` and asserts `observe.entities.is_empty()`.
  - Evidence: `docs/api/stream-b-daemon-mcp-api.md:57-64` documents `entities` as optional and defaulting to `[]`.

- MCP output schema matches `ObserveResponse`.
  - Evidence: `crates/memoryd/src/protocol.rs:170-180` defines `ObserveResponse { fragment_id, target }` and `ObserveTarget::{PlaintextSubstrate, EncryptedSubstrate}` with snake_case serialization.
  - Evidence: `crates/memoryd/src/mcp.rs:402-411` declares the observe output schema with required `fragment_id` and `target`, and `target` enum values `plaintext_substrate | encrypted_substrate`.
  - Evidence: `crates/memoryd/tests/mcp_manifest.rs:104-124` serializes a real `ObserveResponse` and verifies every schema-required key exists on the DTO.
  - Evidence: `docs/api/stream-b-daemon-mcp-api.md:66-75` documents the same output shape.

- `memory_note` remains canonical-note-only and rejects `kind` at the MCP boundary.
  - Evidence: `crates/memoryd/src/mcp.rs:106-110` defines `NoteRequest` as only `{ text }` with `#[serde(deny_unknown_fields)]`.
  - Evidence: `crates/memoryd/src/mcp.rs:188` forwards `ToolRequest::MemoryNote` only to `RequestPayload::WriteNote { text }`.
  - Evidence: `crates/memoryd/tests/mcp_manifest.rs:133-145` verifies `memory_note` with an added `kind` field fails with an unknown-field error.
  - Evidence: `crates/memoryd/src/handlers.rs:287-309` writes notes through the canonical memory path, and `crates/memoryd/tests/dream_substrate_fragments.rs:183-201` verifies `memory_note` writes exactly one canonical memory and no substrate records.
  - Evidence: `docs/api/stream-b-daemon-mcp-api.md:19-35` documents `memory_note` as unchanged, canonical-memory-only, and accepting no `kind`, `entities`, dream controls, or admin fields.

- Observe binding fields are additive and consistent with protocol/MCP forwarding.
  - Evidence: `crates/memoryd/src/protocol.rs:85-94` carries `text`, `kind`, defaulted `entities`, `cwd`, `session_id`, `harness`, and optional `harness_version` in `RequestPayload::Observe`.
  - Evidence: `crates/memoryd/src/mcp.rs:112-123` defines the same fields in `ObserveRequest`, and `crates/memoryd/src/mcp.rs:189-197` forwards them one-for-one into the daemon `Observe` payload.
  - Evidence: `crates/memoryd/src/handlers.rs:326-337` validates observe text/entities/binding metadata and resolves the Stream E session binding before disk effects; `crates/memoryd/src/handlers.rs:357-375` appends the substrate fragment with session, harness, scope, entities, kind, source ref, privacy spans, payload, and classification.
  - Evidence: `crates/memoryd/tests/mcp_forward.rs:143-187` verifies MCP forwarding preserves observe binding fields; `crates/memoryd/tests/dream_substrate_fragments.rs:229-253` verifies project binding affects stored observe scope.
  - Evidence: `docs/api/stream-b-daemon-mcp-api.md:55-64` and `docs/api/stream-b-daemon-mcp-api.md:77-88` document the binding fields and forwarding/storage contract.

- No admin dream CLI tools are exposed via MCP.
  - Evidence: `crates/memoryd/src/mcp.rs:212-225` lists exactly nine MCP tools and includes only `Observe` as the Stream F addition; `crates/memoryd/src/mcp.rs:227-238` maps those names and contains no dream/admin tool names.
  - Evidence: `crates/memoryd/tests/mcp_manifest.rs:4-24` asserts the exact nine-tool manifest order, and `crates/memoryd/tests/mcp_manifest.rs:26-60` explicitly checks `memory_dream_now`, `memory_dream_status`, `memory_dream_enable`, and `memory_dream_disable` are absent.
  - Evidence: `docs/api/stream-b-daemon-mcp-api.md:90-101` documents dream/admin commands as non-tools.

### Non-blocking simplifications

None.

### Test gaps

No Gate-B-blocking test gaps found. Existing tests now cover the requested regressions: omitted observe entities, observe output schema/DTO alignment, `memory_note` rejecting `kind`, observe MCP forwarding, observe substrate write/privacy routing, project binding scope, and dream/admin exclusion from the MCP manifest.

### Questions / uncertainties

- This review is scoped to Stream F Tasks 5-7 observe/protocol/MCP and the latest Gate B fixes. It does not validate the later dream harness, lease, cleanup, recall, or CLI runtime behavior beyond confirming dream/admin tools are not exposed through MCP.

### Positives

- The fix added behavior-level regression coverage instead of only changing docs/schema text.
- Observe validation is now stricter at the trust boundary: invalid entity ids, whitespace-polluted ids, sensitive-looking entity/binding metadata, invalid caller binding, secrets, and refused privacy tiers all fail before substrate writes.
- The docs, MCP manifest, protocol DTOs, handler, and forwarding tests are now aligned for the observe slice.

### Commands run

```bash
cargo test -p memoryd --test protocol_contract --test handler_contract --test mcp_manifest --test mcp_forward --test dream_substrate_fragments
# PASS: dream_substrate_fragments 11 passed; handler_contract 5 passed; mcp_forward 4 passed; mcp_manifest 10 passed; protocol_contract 5 passed

cargo clippy -p memoryd --all-targets --all-features -- -D warnings
# PASS
```
