### Verdict

Changes requested

### Intended outcome

This rerun is intended to verify the Review Gate B fixes for the Stream F observe/protocol/MCP slice: `memory_observe` should expose the substrate-fragment path without changing `memory_note`, the MCP output schema should match `ObserveResponse { fragment_id, target }`, entity handling should preserve data integrity, and the observe path should not introduce new S1/S2 protocol, MCP, privacy, or substrate-write regressions.

### Executive summary

The two prior findings are materially fixed: the MCP output schema now advertises `fragment_id` and `target`, and entity validation rejects whitespace-polluted/non-canonical ids before any fragment write. The requested test and clippy gates pass. However, I found one remaining S2/API-contract issue: the MCP manifest requires `entities` even though both the canonical Stream F spec and the Rust `ObserveRequest`/daemon protocol default it to an empty vector. Schema-driven MCP clients can reject a valid zero-entity observe call before it reaches the daemon. Gate B should stay FAIL until this contract drift is corrected.

### Findings

[Medium] [API Contract] `memory_observe` manifest makes defaulted `entities` mandatory

- Evidence: `docs/specs/stream-f-dreaming-v0.2.md:110-116` defines `MemoryObserveRequest.entities` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, and `docs/specs/stream-f-dreaming-v0.2.md:228-233` repeats the daemon `RequestPayload::Observe` with `#[serde(default)] entities: Vec<String>`. The implementation mirrors this in `crates/memoryd/src/mcp.rs:112-123`, where `ObserveRequest.entities` is also defaulted. But the MCP manifest schema at `crates/memoryd/src/mcp.rs:376-399` declares `"required": ["text", "kind", "entities", "cwd", "session_id", "harness"]`.
- Why it matters: A schema-driven MCP client may refuse to call `memory_observe` unless it supplies `entities: []`, even though the daemon and canonical spec accept omitted entities as an empty list. That creates unnecessary client breakage for common observations where no entity ids are known, and it makes the machine-readable manifest stricter than the actual protocol contract.
- Reasoning: This is the same class of issue as the prior output-schema mismatch: the Rust path can work while the manifest gives agents a different contract. The current tests assert that `entities` is required, so they lock in the drift rather than protecting the spec behavior.
- Recommendation: Remove `entities` from the manifest `required` array while keeping the array schema, bounds, and item pattern. Add a manifest/request test that omits `entities`, confirms `request_from_args(... memory_observe ...)` defaults to `Vec::new()`, and confirms the manifest no longer marks `entities` required.
- Confidence: High

### Non-blocking simplifications

- None.

### Test gaps

- Missing regression coverage for omitted `entities` on `memory_observe`. The behavior to protect is: schema-driven clients may omit `entities`; Rust deserialization defaults it to `[]`; the handler writes a valid zero-entity substrate fragment.

### Questions / uncertainties

- I treated `docs/specs/stream-f-dreaming-v0.2.md` as the canonical contract per the task prompt. `docs/api/stream-b-daemon-mcp-api.md` currently says `entities` is required, but that appears to be downstream documentation drift from the implementation rather than a spec override.
- This review focused on the protocol/MCP/observe path requested here, not the full Stream F dream run/lease/harness pipeline.

### Positives

- Prior MCP output-schema finding is fixed: `observe_output_schema()` now requires `fragment_id` and `target`, and the manifest test serializes a real `ObserveResponse` to catch future drift.
- Prior entity whitespace/data-integrity finding is fixed: handler validation now rejects leading/trailing whitespace, non-`ent_` ids, free-form emails/secrets, and overlong entity ids before any substrate write.
- Privacy routing remains behavior-first and well covered: PII goes to encrypted substrate without plaintext leakage, and secrets are refused before disk effects.

## Prior finding verification

- Prior S2, MCP output schema mismatch: fixed. Evidence: `crates/memoryd/src/mcp.rs:402-411` advertises `fragment_id` and `target`, and `crates/memoryd/tests/mcp_manifest.rs:88-106` checks those required keys against serialized `ObserveResponse`.
- Prior low-severity entity whitespace/data-integrity gap: fixed. Evidence: `crates/memoryd/src/handlers.rs:386-395` rejects leading/trailing whitespace and non-canonical `ent_` ids, and `crates/memoryd/tests/dream_substrate_fragments.rs:151-174` covers leading/trailing whitespace and non-id entities.
- New S1/S2 status: one S2-equivalent API-contract issue remains for the observe input manifest requiring defaulted `entities`.

## Gate results

```bash
cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e
# PASS: dream_substrate_fragments 9 passed; mcp_forward 4 passed; mcp_manifest 9 passed; privacy_e2e 11 passed

cargo clippy -p memoryd --all-targets --all-features -- -D warnings
# PASS
```

Gate B status: FAIL. Do not mark PASS while the remaining S2 API-contract drift above is open.
