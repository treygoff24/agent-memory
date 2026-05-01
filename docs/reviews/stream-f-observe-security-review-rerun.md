# Stream F Gate B Security/Privacy Review Rerun — `memory_observe`

Date: 2026-05-01
Reviewer role: security/privacy review, read-only implementation lane
Mandatory skills used: clean-code, tdd, rust-engineer
Owned report file only: `docs/reviews/stream-f-observe-security-review-rerun.md`

## Verdict

**NOT PASS.**

The requested cargo gates are green and the obvious prior regressions are improved, but **S1/S2 privacy findings remain** in the `memory_observe` write surface. The main issue is that Stream D classification still only covers `text`; other caller-controlled fields that are serialized into substrate JSONL can carry secrets/PII outside the classification/encryption boundary.

## Scope reviewed

- `docs/specs/stream-f-dreaming-v0.2.md`
- `docs/reviews/stream-f-observe-security-review.md`
- `docs/api/stream-b-daemon-mcp-api.md`
- `crates/memoryd/src/handlers.rs`
- `crates/memoryd/src/mcp.rs`
- `crates/memoryd/src/protocol.rs`
- `crates/memoryd/src/recall/binding.rs`
- `crates/memoryd/tests/dream_substrate_fragments.rs`
- `crates/memoryd/tests/mcp_forward.rs`
- `crates/memoryd/tests/mcp_manifest.rs`
- `crates/memoryd/tests/privacy_e2e.rs`
- `crates/memory-substrate/src/api.rs` append path used by observe

Review focus:

- prior entity leakage finding closure;
- prior binding/scope hardcoding finding closure;
- invalid/missing binding behavior before substrate writes;
- encrypted substrate plaintext leakage;
- MCP/admin surface bounds;
- Rust correctness, async safety, test quality, and spec compliance for the reviewed slice.

## Findings

### S1 — `entities[]` can still carry secrets/PII by using syntactically valid `ent_` ids

**Files:** `crates/memoryd/src/handlers.rs:326-333`, `crates/memoryd/src/handlers.rs:384-412`, `crates/memoryd/src/handlers.rs:352-362`, `crates/memory-substrate/src/api.rs:693-706`, `crates/memory-substrate/src/api.rs:717-730`, `crates/memoryd/tests/dream_substrate_fragments.rs:72-88`, `crates/memoryd/tests/dream_substrate_fragments.rs:151-174`

**What happens**

The rerun implementation now rejects raw free-form entity strings unless they match `ent_[A-Za-z0-9_.:-]{1,124}` and rejects leading/trailing whitespace. That closes the exact raw examples from the first review (`AKIA...`, `reviewer@example.com`, whitespace-polluted ids).

However, `memory_observe` still classifies only `text` (`classify_privacy(&text, ...)`) before selecting plaintext vs encrypted substrate. `validated_observe_entities` is a syntax-only allowlist; it does not run Stream D classification, safe-fragment checks, or registry-backed entity resolution before the values are serialized. The substrate append path then copies `request.entities` verbatim into both plaintext and encrypted substrate records.

Because the allowed grammar includes ASCII alphanumerics, `_`, `.`, `:`, and `-`, sensitive values can be embedded in ids that pass the validator, for example:

```json
{
  "text": "safe observation",
  "kind": "signal",
  "entities": ["ent_AKIA1234567890ABCDEF"]
}
```

Similar bypasses exist for `ent_ghp_...`, `ent_sk_live_...`, `ent_123-45-6789`, and `ent_202-555-0198`. The existing regression tests only reject the unprefixed raw secret/email and generic non-id strings; they do not cover secret/PII substrings inside syntactically valid entity ids.

**Exploitability**

Any MCP caller able to invoke `memory_observe` can send safe `text` plus a sensitive value hidden inside a syntactically valid entity id. Since classification is based only on `text`, the request can route to plaintext `substrate/<device>/<date>.jsonl`, and the sensitive entity string is persisted in git-synced JSONL outside encryption.

**Impact**

This is still a direct privacy-boundary bypass for the Stream F observe surface. It violates the rerun criterion that entities cannot carry secrets/PII outside classification/encryption. It also weakens encrypted-substrate guarantees because encrypted records still store the same `entities` array outside ciphertext.

**Minimal remediation**

Pick one durable contract and test it vertically:

1. Classify every persisted caller-controlled metadata string before append, including each entity id after syntactic validation. If any entity id contains `Secret`, refuse with no substrate/event effect. If any entity id requires encryption, either reject entity metadata outright or store only a non-sensitive canonical id derived from a registry.
2. Better long-term: resolve `entities[]` against a Stream A/E entity registry and persist only existing canonical ids. Unknown ids should be rejected before disk effects, not accepted because they match a regex.
3. Add RED tests before implementation:
   - safe `text` + `ent_AKIA1234567890ABCDEF` refuses and writes no plaintext canary;
   - safe `text` + `ent_202-555-0198` does not persist the raw phone outside ciphertext;
   - encrypted `text` + sensitive-looking entity id does not put the sensitive entity id in the encrypted JSONL metadata.

### S2 — Persisted binding metadata is validated for presence/length only and can carry secrets/PII into substrate JSONL

**Files:** `crates/memoryd/src/recall/binding.rs:22-61`, `crates/memoryd/src/handlers.rs:328-333`, `crates/memoryd/src/handlers.rs:352-361`, `crates/memoryd/src/handlers.rs:438-440`, `crates/memory-substrate/src/api.rs:693-706`, `crates/memory-substrate/src/api.rs:717-730`, `crates/memoryd/src/mcp.rs:392-395`

**What happens**

The prior hardcoded project-scope issue is improved: observe now calls `validate_session_fields`, resolves project binding from `cwd`, writes `project:<canonical_id>` when available, and falls back to `agent` rather than the old `project:agent-memory` default.

But the same binding fields are serialized into the substrate record:

- `session: Some(binding.session_id.clone())`
- `harness: Some(binding.harness.clone())`
- `source_ref: Some(format!("session:{}:memory_observe", binding.session_id))`

`validate_session_fields` only trims and enforces non-empty / <=128-byte limits for `session_id` and `harness`. It does not constrain them to a safe id grammar and does not classify them before persistence. The MCP schema mirrors this by declaring `session_id`, `harness`, and `harness_version` as generic strings with length bounds only.

**Exploitability**

A malformed, compromised, or overly literal MCP client can place sensitive material in `session_id` or `harness` while providing safe observation text. The handler will validate the binding, classify only the safe text, then persist the sensitive binding value in plaintext substrate metadata and in `source_ref`.

Example shape:

```json
{
  "text": "safe observation",
  "kind": "observation",
  "entities": [],
  "cwd": "/absolute/existing/path",
  "session_id": "sess_AKIA1234567890ABCDEF",
  "harness": "codex"
}
```

**Impact**

This is a narrower metadata leak than the entity bypass, but it still violates the observe privacy model: caller-controlled strings that are persisted to git-synced substrate files bypass Stream D classification. It also makes the `source_ref` field an unclassified side channel.

**Minimal remediation**

- Restrict persisted binding fields to safe grammars before observe writes, e.g. `session_id = ^sess_[A-Za-z0-9_.:-]{1,123}$` or another documented generated-id grammar, and `harness` to known harness identifiers or `^[A-Za-z0-9_.:-]{1,128}$`.
- Also run Stream D classification or `safe_plaintext_fragment` over the final persisted metadata values (`session`, `harness`, `source_ref`) and fail closed on secret/private results.
- Add behavior tests proving secret-bearing `session_id`/`harness` values fail before any substrate fragment or event is written.

## Prior finding verification

- **Raw entity secrets/PII rejected:** Partially fixed. Raw `AKIA...`, raw email, non-id strings, and whitespace-polluted ids are rejected before fragment files are written. The fix is incomplete because sensitive substrings embedded inside syntactically valid `ent_...` ids still bypass classification.
- **Observe binding/scope hardcoding:** Mostly fixed. Observe now requires `cwd`, `session_id`, and `harness`, validates `cwd` through the Stream E binding path, derives `project:<canonical_id>` when a project binding exists, and otherwise derives `agent`. The old hardcoded `project:agent-memory` scope is gone. Remaining risk is unclassified persisted binding metadata, not scope hardcoding.
- **Invalid/missing binding before disk effects:** Fixed for absent/invalid required fields and invalid cwd. Existing coverage rejects a relative cwd before substrate files are created. Missing project binding intentionally maps to `agent` scope, which matches the rerun prompt's project/agent scope language.
- **Encrypted substrate plaintext text leak:** Fixed for the observed `text` body. Encrypted observe records omit `text`, use a safe descriptor, and the privacy E2E gate remains green. The metadata side channels above still need closure.
- **MCP/admin surfaces bounded:** Fixed for the MCP manifest. The manifest exposes exactly the nine agent-facing tools and excludes dream/admin/device/privacy control surfaces. `DreamNow` and `DreamStatus` remain daemon protocol variants but the handler returns `not_implemented`, and they are not forwardable through the MCP tool registry.

## Positive controls observed

- `memory_note` remains a separate canonical write path; `NoteRequest` denies unknown fields, and regression coverage proves `memory_note` does not create substrate fragments.
- Observe validates `text` before classification and refuses secret text before substrate writes.
- Project binding resolution is reused from the Stream E startup path instead of a bespoke Stream F parser.
- `cwd` is canonicalized before scope derivation; relative cwd fails before disk effects.
- The MCP manifest declares `additionalProperties: false` for `memory_observe` and excludes admin-only dream/privacy/device controls.
- The reviewed async code is straightforward and bounded: observe awaits binding resolution, classification/encryption, and append in sequence; I did not see detached tasks, shared mutable async state, or cancellation-unsafe partial writes introduced in this slice beyond the existing substrate write outcome model.

## Verification run

```bash
cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e
```

Result: **PASS**

- `dream_substrate_fragments`: 9 passed
- `mcp_forward`: 4 passed
- `mcp_manifest`: 9 passed
- `privacy_e2e`: 11 passed

```bash
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Result: **PASS**

## TDD note

This was a review-only lane with the report as the only owned file, so I did not add RED tests or implementation fixes. The remediation should start with one narrow behavior test for the `ent_AKIA...` entity bypass, then the smallest metadata-classification/registry-validation implementation, then the same narrow gate to GREEN before broadening to binding metadata tests.

## Residual risk and confidence

Residual risk is **medium-high** until all persisted caller-controlled observe metadata is either syntactically constrained to non-sensitive generated ids and/or classified before append. The text encryption/refusal path itself is in materially better shape than the first review, but the metadata channels are still writable by MCP callers.

Confidence is **high** for the S1 entity finding because the code path classifies only `text`, the validator accepts sensitive substrings inside valid `ent_...` ids, and the append path serializes `entities` verbatim. Confidence is **medium-high** for the S2 binding-metadata finding because the fields are caller-controlled and persisted, though the operational likelihood depends on client behavior.
