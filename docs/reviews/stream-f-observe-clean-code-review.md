### Verdict

Changes requested

### Intended outcome

Tasks 5-7 appear to add the Stream F observe/protocol/MCP surface: keep `memory_note` as canonical-note-only, add `memory_observe` as the agent-facing substrate-fragment capture tool, keep dream operations out of the MCP manifest, and route observed text through Stream D classification before any substrate disk write. The scoped docs/tests also try to lock the protocol DTOs and behavior-first coverage for plaintext, encrypted, refused, and forwarded observe requests.

### Executive summary

The implementation mostly achieves the privacy and routing goals: `memory_observe` is separate from `memory_note`, admin dream tools do not appear in the MCP manifest, observe classification happens before substrate append, PII routes to encrypted substrate records without plaintext fields, and the requested targeted gates pass. I found one S2/API-contract issue: the MCP manifest advertises an observe output schema that does not match the actual daemon response (`status`/`fragment_ref`/`classification` vs `fragment_id`/`target`). That can break schema-driven MCP clients even though the Rust path works. I also found a lower-severity entity-normalization gap that can persist whitespace-polluted entity ids and later break exact entity matching.

### Findings

[Medium] [API Contract] `memory_observe` manifest output schema does not match the actual response

- Evidence: `crates/memoryd/src/mcp.rs:341-347` declares the observe output schema as `status`, `fragment_ref`, and `classification`, with `status` required. The actual protocol response is `ObserveResponse { fragment_id, target }` in `crates/memoryd/src/protocol.rs:166-170`, returned by `observe_response` as `ResponsePayload::Observe(ObserveResponse { fragment_id: outcome.id, target })`.
- Why it matters: MCP clients often rely on manifest schemas for tool-call validation and result parsing. A client following this manifest will look for `status` and may treat a successful observe response as malformed, even though the daemon wrote the fragment correctly.
- Reasoning: This is not just documentation drift; it is the machine-readable contract exposed to agents. The targeted tests validate the input schema but do not assert that the output schema matches the response DTO, so the mismatch can ship undetected.
- Recommendation: Change the observe output schema to advertise `fragment_id: string` and `target: string` (ideally enum `plaintext_substrate | encrypted_substrate`) as required fields, or change the protocol response to match the advertised schema. Add a manifest test that serializes a real `ObserveResponse` and checks the schema-required keys line up.
- Confidence: High

[Low] [Data Integrity] Observe entities accept whitespace-polluted ids that later will not match exact entity lookups

- Evidence: `validate_observe_entities` rejects only `entity.trim().is_empty()` and byte length in `crates/memoryd/src/handlers.rs:361-373`, but it passes the original `entities` vector through to `append_substrate_fragment` without trimming or rejecting leading/trailing whitespace.
- Why it matters: Stream F and Stream E use entity ids for later dream/question surfacing and exact entity overlap. Persisting `" ent_auth_flow "` instead of `"ent_auth_flow"` creates durable substrate data that appears valid but will fail later exact-match retrieval or pending-attention surfacing.
- Reasoning: The code already treats all-whitespace as invalid, which implies whitespace is not semantically meaningful for ids. Leaving non-empty surrounding whitespace intact makes the validation boundary leaky.
- Recommendation: Either reject `entity.trim() != entity` with `invalid_request`, or normalize entities before append. If normalization is chosen, dedupe after trimming to avoid duplicate ids introduced by whitespace variants.
- Confidence: Medium

### Non-blocking simplifications

- Consider replacing the hand-rolled `base64_encode` helper in `crates/memoryd/src/handlers.rs` with a shared/base64 crate helper if one is already available in the workspace. The current helper is small and tested indirectly, so this is not a blocker, but a library call would reduce bespoke encoding surface.

### Test gaps

- Add a behavior test that the `memory_observe` manifest output schema matches the serialized `ObserveResponse` fields (`fragment_id`, `target`). Current manifest tests cover input shape and tool inclusion/exclusion but miss the output-contract mismatch.
- Add an observe validation test for leading/trailing whitespace in `entities` and decide whether the contract rejects or normalizes it.

### Questions / uncertainties

- I did not review Stream F CLI/runtime implementation outside the scoped files, so this review only covers the protocol/MCP/observe path requested here.
- The docs in `docs/api/stream-b-daemon-mcp-api.md` still say the observe storage handler is owned by a later task even though this slice implements it. I treated that as documentation drift, not a blocker, because the behavioral tests and code path are authoritative.

### Positives

- `memory_note` remains canonical-only and now denies unknown `kind`, protecting the v0.2 reversal away from `memory_note(kind=...)`.
- The observe handler classifies before substrate append and refuses secrets before fragment files are created.
- The encrypted substrate tests check both record shape and plaintext canaries across repo/runtime, which is the right behavior-first privacy coverage.

### Verification

- PASS: `cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e`
- PASS: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

Gate B status: FAIL / changes requested because an S2-equivalent API-contract finding remains.
