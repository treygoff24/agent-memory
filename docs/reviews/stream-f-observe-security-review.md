# Stream F Gate B Security/Privacy Review — `memory_observe`

Date: 2026-04-30
Reviewer role: security/privacy review, read-only implementation lane
Mandatory skills used: clean-code, tdd, rust-engineer

## Verdict

**NOT PASS.**

The requested cargo gates are green, but **S1/S2 privacy and authorization-boundary findings remain** in the `memory_observe` path.

## Scope reviewed

- `docs/specs/stream-f-dreaming-v0.2.md`
- `crates/memoryd/src/mcp.rs`
- `crates/memoryd/src/handlers.rs`
- `crates/memoryd/tests/dream_substrate_fragments.rs`
- `crates/memoryd/tests/privacy_e2e.rs`
- `memory-substrate` append primitive as used by `memory_observe`

Review focus:

- no disk effect before Stream D classification;
- secret/refuse path writes no fragment/event with plaintext;
- PII/encrypted substrate record has no `text` and no raw sensitive canary anywhere outside ciphertext;
- descriptor is safe and does not leak raw text;
- `memory_note` remains canonical memory path and is not silently converted into observe;
- MCP schema does not expose admin dream commands.

## Findings

### S1 — `entities[]` bypasses Stream D classification and can persist secrets/PII in plaintext

**Files:** `crates/memoryd/src/handlers.rs:309-340`, `crates/memoryd/src/handlers.rs:361-373`, `crates/memory-substrate/src/api.rs:693-731`, `crates/memoryd/src/mcp.rs:368-380`, `crates/memoryd/tests/dream_substrate_fragments.rs:36-65`

**What happens**

`memory_observe` validates only that entity strings are non-empty and at most 128 bytes:

- `observe_response` classifies only `text` via `classify_privacy(&text, ...)` before selecting plaintext vs encrypted substrate.
- `validate_observe_entities` does not classify, pattern-restrict, or safe-fragment-check entity values.
- The raw `entities` vector is copied into `SubstrateFragmentAppendRequest`.
- The substrate append primitive serializes `entities` verbatim into both plaintext and encrypted substrate records.

The MCP schema similarly declares `entities.items` as unconstrained strings. The existing PII test proves the encrypted record omits `text` and uses a safe descriptor, but it only passes a safe entity id (`ent_launch`), so it does not cover entity-value leakage.

**Exploitability**

Any MCP caller that can call `memory_observe` can bypass the intended refusal/encryption boundary by placing sensitive content in `entities` instead of `text`:

```json
{
  "text": "safe observation",
  "kind": "signal",
  "entities": ["AKIA1234567890ABCDEF"]
}
```

Because the classifier runs only on `"safe observation"`, the request routes to plaintext `substrate/<device>/<date>.jsonl`, and the secret value is persisted in the `entities` array. A PII variant such as `"reviewer@example.com"` similarly persists outside ciphertext even if the `text` path would have encrypted/refused it.

**Impact**

This is a direct privacy-boundary bypass for the new Stream F write surface. It violates the review contract that refused/secret paths have no disk effect and that encrypted substrate records contain no raw sensitive canary outside ciphertext. It also increases the git-synced raw-observation surface with unclassified caller-controlled strings.

**Minimal remediation**

Pick one strict contract and test it:

1. Prefer: make `entities[]` IDs only. Enforce a canonical entity-id grammar such as `^ent_[A-Za-z0-9_.:-]{1,124}$` or whatever Stream A/E actually accepts. Reject anything else before disk effects.
2. Additionally or alternatively: classify the full persisted caller payload before append, including `text` plus every `entities[]` value. If any entity value is secret, refuse; if any requires encryption, do not store that raw entity outside ciphertext.
3. Add behavior tests:
   - safe `text` + secret entity refuses and writes no substrate/event plaintext canary;
   - PII `text` + PII entity writes encrypted payload with no raw PII anywhere outside ciphertext;
   - `memory_observe` rejects non-id entity strings if the ID-only contract is chosen.

### S2 — `memory_observe` ignores session/project binding and hardcodes all observations into one project scope

**Files:** `docs/specs/stream-f-dreaming-v0.2.md:503-509`, `crates/memoryd/src/handlers.rs:329-340`, `crates/memoryd/src/mcp.rs:112-119`, `crates/memoryd/src/protocol.rs:85-90`

**What happens**

The Stream F spec says `device`, `session`, and `harness` are populated from caller context and that `scope` is inferred from the calling session's project binding. The implementation has no caller binding on `RequestPayload::Observe` / `ObserveRequest`; it writes:

- `session: None`
- `harness: Some("memoryd")`
- `scope: format!("project:{DEFAULT_PROJECT_NAMESPACE}")`
- `source_ref: Some("memoryd.memory_observe")`

So every `memory_observe` call is stored under the same synthetic project scope regardless of the caller's cwd/session/project.

**Exploitability**

Any agent-facing caller from any project context that can reach the daemon can add substrate fragments to the default project scope. There is no per-session scope derivation or rejection when the binding is unavailable.

**Impact**

This breaks the Stream E/F project authorization boundary before the dream runner exists. Future Pass 1 scope filtering will either miss legitimate non-default project observations or include unrelated project observations in the default project's dream context. That is a privacy bleed across project scopes and a provenance/audit loss for low-level telemetry.

**Minimal remediation**

- Extend the observe protocol/MCP request or daemon session state so `memory_observe` receives the same binding context used by `memory_startup` (`cwd`, `session_id`, `harness`, optional version).
- Derive `scope`, `session`, `harness`, and `source_ref` from validated binding context.
- Fail closed when binding cannot be resolved.
- Add tests for two different project bindings proving fragments land under distinct scope values and cannot be written without a valid binding.

## Positive findings / covered controls

- `memory_note` remains a separate canonical path. `NoteRequest` denies unknown fields, `memory_note` maps only to `RequestPayload::WriteNote`, and regression coverage asserts it writes exactly one canonical memory and no substrate records.
- The encrypted `text` path itself is safe: `encrypted_observe_payload` does not use raw text in the descriptor, and the substrate encrypted record shape omits `text`.
- Secret-in-`text` requests currently return before substrate append, and the existing test asserts no substrate fragment and no canary in repo/runtime.
- The MCP manifest enumerates exactly nine agent-facing tools and excludes dream/admin commands; tests cover the manifest and `ToolName` conversion path.

## Verification run

```bash
cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e
```

Result: **PASS**

- `dream_substrate_fragments`: 5 passed
- `mcp_forward`: 4 passed
- `mcp_manifest`: 8 passed
- `privacy_e2e`: 11 passed

```bash
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Result: **PASS**

## TDD note

This was a review-only lane with the report as the only owned file, so I did not add failing tests or implementation fixes. The S1 remediation should start with the narrow red tests listed above, then implement the smallest entity-validation/classification slice, then rerun the same narrow cargo gate.

## Residual risk and confidence

Residual risk is **medium-high** after this review because the green tests do not exercise sensitive content in `entities[]` or project-binding isolation for `memory_observe`.

Confidence is **high** for the two findings above: both are visible in the request schema, handler path, and append serialization path without relying on speculative runtime behavior.
