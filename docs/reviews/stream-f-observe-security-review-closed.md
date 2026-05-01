# Stream F Gate B Security/Privacy Review Closed Rerun - `memory_observe`

Date: 2026-05-01
Reviewer role: security/privacy review, read-only implementation lane
Mandatory skills used: clean-code, tdd, rust-engineer
Owned report file only: `docs/reviews/stream-f-observe-security-review-closed.md`

## Verdict

**FAIL.**

The scoped observe fixes close the prior AWS/email/dashed-phone canaries and the core text privacy route is now materially stronger. However, one **severity-2 privacy finding remains**: phone-like PII can still be embedded in syntactically valid `entities[]`, `session_id`, or `harness` metadata using dot-separated, digit-only, or underscore-separated forms. Those values pass validation, produce plaintext substrate disk effects, and are persisted outside ciphertext.

No severity-1 finding was confirmed in this rerun. Severity-2 remains, so this gate is not passable yet.

## Scope reviewed

- Contract: `docs/specs/stream-f-dreaming-v0.2.md`
- Prior reports:
  - `docs/reviews/stream-f-observe-security-review.md`
  - `docs/reviews/stream-f-observe-security-review-rerun.md`
- Implementation:
  - `crates/memoryd/src/handlers.rs`
  - `crates/memoryd/src/mcp.rs`
  - `crates/memoryd/src/protocol.rs`
  - `crates/memoryd/src/recall/binding.rs`
  - `crates/memory-substrate/src/api.rs`
  - `crates/memory-privacy/src/regex.rs`
  - `crates/memory-privacy/src/decision.rs`
- Tests:
  - `crates/memoryd/tests/dream_substrate_fragments.rs`
  - `crates/memoryd/tests/privacy_e2e.rs`
  - `crates/memoryd/tests/mcp_manifest.rs`
  - `crates/memoryd/tests/mcp_forward.rs`

## Findings by severity

### S2 - Phone-like PII still bypasses observe metadata validation in non-dashed forms

**Status:** open / blocking

**File evidence:**

- `crates/memoryd/src/handlers.rs:389-418` validates `entities[]` as canonical `ent_` ids, then calls metadata safety validation.
- `crates/memoryd/src/handlers.rs:421-435` validates `session_id`, `harness`, and `harness_version` as non-empty, <=128 bytes, safe-id-character strings, then calls metadata safety validation.
- `crates/memoryd/src/handlers.rs:442-456` rejects metadata when `safe_plaintext_fragment` rejects it or a narrow observe canary check hits.
- `crates/memoryd/src/handlers.rs:466-474` only detects phone canaries in the dashed `NNN-NNN-NNNN` form.
- `crates/memoryd/src/handlers.rs:414` and `crates/memoryd/src/handlers.rs:438-439` allow `.`, `_`, digits, and `-` inside entity/binding ids, so dot-separated, digit-only, and underscore-separated phone-shaped ids are syntactically valid.
- `crates/memory-privacy/src/regex.rs:52-55` defines phone detection with word boundaries. When the phone digits are embedded after an underscore in `ent_...`, `sess_...`, or `codex_...`, the deterministic classifier does not catch the dot/no-separator/underscore variants.
- `crates/memoryd/src/handlers.rs:338-356` classifies only `text` for storage routing; metadata validation is the only remaining barrier for these fields.
- `crates/memoryd/src/handlers.rs:357-371` appends accepted metadata into the substrate fragment.
- `crates/memory-substrate/src/api.rs:693-706` persists plaintext substrate metadata fields including `session`, `harness`, `scope`, `entities`, `text`, and `source_ref`.
- `crates/memory-substrate/src/api.rs:717-730` also persists the same metadata fields for encrypted substrate records outside ciphertext.
- `crates/memoryd/src/handlers.rs:485-486` derives `source_ref` directly from `session_id`, so unsafe session metadata also fans out into another persisted field.
- `crates/memoryd/tests/dream_substrate_fragments.rs:111-131` covers `ent_AKIA1234567890ABCDEF`, `ent_202-555-0198`, and `ent_reviewer@example.com`, but not dot-separated, digit-only, or underscore-separated phone-like values.
- `crates/memoryd/tests/dream_substrate_fragments.rs:133-161` covers sensitive binding metadata with dashed phone and AWS canaries, but not dot-separated, digit-only, or underscore-separated phone-like values.

**Behavioral proof:** a read-only probe built outside the repo at `/tmp/agent-memory-observe-probe` exercised the public `handle_request` observe path against a temp substrate. Relevant output:

```text
entity ent_202-555-0198: code=invalid_request wrote=false leaked=false
entity ent_202.555.0198: code=success wrote=true leaked=true
entity ent_2025550198: code=success wrote=true leaked=true
entity ent_202_555_0198: code=success wrote=true leaked=true
entity ent_AKIA1234567890ABCDEF: code=invalid_request wrote=false leaked=false
entity ent_reviewer@example.com: code=invalid_request wrote=false leaked=false
session sess_202-555-0198: code=invalid_request wrote=false leaked=false
session sess_202.555.0198: code=success wrote=true leaked=true
session sess_2025550198: code=success wrote=true leaked=true
session sess_202_555_0198: code=success wrote=true leaked=true
session sess_AKIA1234567890ABCDEF: code=invalid_request wrote=false leaked=false
harness codex_202-555-0198: code=invalid_request wrote=false leaked=false
harness codex_202.555.0198: code=success wrote=true leaked=true
harness codex_2025550198: code=success wrote=true leaked=true
harness codex_202_555_0198: code=success wrote=true leaked=true
harness codex_AKIA1234567890ABCDEF: code=invalid_request wrote=false leaked=false
```

`wrote=true` means the safe observation reached plaintext `substrate/`; `leaked=true` means the canary string was present in repo/runtime plaintext. This directly violates the requested confirmation that phone-like `entities[]` and persisted binding metadata are rejected/constrained before disk effects.

**Exploitability:** Any MCP caller that can invoke `memory_observe` can provide safe `text` plus a phone-like value in `entities[]`, `session_id`, or `harness`. The current handler accepts several common phone encodings and writes them to git-synced JSONL outside encryption.

**Impact:** PII can be persisted as plaintext metadata even when the text path would have routed the phone number to encrypted substrate. It also contaminates `source_ref` when the value is in `session_id`.

**Minimal remediation:**

1. Add failing behavior tests for `ent_202.555.0198`, `ent_2025550198`, `ent_202_555_0198`, and equivalent `session_id` / `harness` values. Assert no plaintext or encrypted substrate records and no canary in repo/runtime.
2. Do not rely on word-boundary phone regexes for embedded ids. Normalize candidate metadata by stripping allowed id separators/prefixes, then reject phone-like digit sequences, or classify both the raw value and a separator-normalized projection.
3. Consider reducing persisted metadata to generated opaque ids only. If `session_id` and `harness` are caller-controlled, constrain them to documented generated-id grammars that cannot encode user data.
4. Re-run the targeted observe tests and the external probe variants before passing Gate B.

## Required control confirmations

### Secrets in `text` refuse before disk effects

**Confirmed.**

- `crates/memoryd/src/handlers.rs:326-340` validates text, runs Stream D classification over `text`, and returns `privacy` before substrate append when storage is refused.
- `crates/memoryd/src/handlers.rs:357-371` is the first substrate append point and is reached only after the refusal check.
- `crates/memoryd/tests/dream_substrate_fragments.rs:163-181` asserts an AWS secret in observe text returns an error, writes no substrate records, and leaves no secret canary in repo/runtime.

### PII text routes to encrypted substrate with no raw sensitive canary outside ciphertext

**Confirmed for the text body.**

- `crates/memoryd/src/handlers.rs:344-356` selects `EncryptedSubstrate` when privacy routing requires encryption.
- `crates/memoryd/src/handlers.rs:489-502` encrypts the observed text and emits only encryption metadata plus a safe descriptor.
- `crates/memory-substrate/src/api.rs:717-730` writes encrypted substrate records without a `text` field.
- `crates/memoryd/tests/dream_substrate_fragments.rs:40-69` verifies `reviewer@example.com` in observe text routes to encrypted substrate, the encrypted record has no `text`, ciphertext/descriptor fields are present, and neither the raw PII nor the full observed text appears in repo/runtime plaintext.

**Residual caveat:** metadata can still carry phone-like PII per S2 above.

### Sensitive-looking `entities[]` values are rejected before disk effects

**Partially confirmed, but not complete. FAIL.**

- Fixed cases: raw AWS, email-like values with `@`, `ent_AKIA1234567890ABCDEF`, and dashed phone `ent_202-555-0198` are rejected; see `crates/memoryd/tests/dream_substrate_fragments.rs:71-131`.
- Open gap: `ent_202.555.0198`, `ent_2025550198`, and `ent_202_555_0198` are accepted and persisted, proven by the external probe above.

### Persisted binding metadata is constrained/safe; `cwd` is not persisted

**Partially confirmed, but not complete. FAIL.**

- `cwd` is used for Stream E binding validation and scope derivation, but the append request only persists `session`, `harness`, `scope`, `entities`, `kind`, `source_ref`, `privacy_spans`, and payload; see `crates/memoryd/src/handlers.rs:357-371`. No `cwd` field is sent to `SubstrateFragmentAppendRequest`.
- The substrate records persist `session` and `harness`, but not `cwd`; see `crates/memory-substrate/src/api.rs:693-706` and `crates/memory-substrate/src/api.rs:717-730`.
- `session_id`, `harness`, and `harness_version` now have safe-character and canary validation at `crates/memoryd/src/handlers.rs:421-435`.
- Open gap: dot-separated, digit-only, and underscore-separated phone-like values still pass in `session_id` and `harness`, are persisted, and can also leak through `source_ref`; see S2.

### Project scope is derived from Stream E binding rather than a hardcoded default

**Confirmed.**

- `crates/memoryd/src/handlers.rs:334-337` calls `crate::recall::binding::validate_session_fields` with the observe request's `cwd`, `session_id`, and `harness`.
- `crates/memoryd/src/recall/binding.rs:22-41` canonicalizes `cwd`, resolves the project binding through Stream E recall binding, and returns `SessionBinding` with project/namespaces.
- `crates/memoryd/src/handlers.rs:477-483` derives observe scope from `binding.project.canonical_id`, falling back to `agent` only when no project binding exists.
- `crates/memoryd/tests/dream_substrate_fragments.rs:229-253` proves two different `.memory-project.yaml` bindings produce `project:proj_alpha` and `project:proj_beta` substrate scopes.

### No dream admin operations are exposed over MCP

**Confirmed.**

- `crates/memoryd/src/mcp.rs:212-224` enumerates exactly nine MCP tools: search/get/write/supersede/forget/reveal/startup/note/observe.
- `crates/memoryd/src/mcp.rs:242-257` only accepts those tool names in `TryFrom<&str>`.
- `crates/memoryd/tests/mcp_manifest.rs:4-24` asserts the manifest contains exactly those nine tools.
- `crates/memoryd/tests/mcp_manifest.rs:26-60` asserts `memory_dream_now`, `memory_dream_status`, `memory_dream_enable`, and `memory_dream_disable` are absent from the MCP manifest.
- `crates/memoryd/src/handlers.rs:118-123` keeps `DreamNow` and `DreamStatus` daemon protocol variants unimplemented in the handler; regardless, they are not reachable through MCP forwarding.

## Positive controls observed

- `memory_note` remains separate from `memory_observe`; `NoteRequest` denies unknown fields at `crates/memoryd/src/mcp.rs:106-110`, and `crates/memoryd/tests/mcp_manifest.rs:133-145` proves `kind` is rejected on `memory_note`.
- Observe schema now requires binding context (`cwd`, `session_id`, `harness`) and denies unknown fields: `crates/memoryd/src/mcp.rs:112-124` and `crates/memoryd/src/mcp.rs:376-399`.
- Encrypted observe records use safe descriptors and omit raw text.
- Invalid relative `cwd` is rejected before substrate writes; see `crates/memoryd/tests/dream_substrate_fragments.rs:255-270`.
- Existing targeted tests and clippy are green.

## Commands run

```bash
git status --short
sed -n '1,260p' docs/specs/stream-f-dreaming-v0.2.md
ls -1 docs/reviews/stream-f-observe-security-review*.md 2>/dev/null | sort | xargs -I{} sh -c 'echo --- {}; sed -n "1,220p" "$1"' sh {}
rg -n "memory_observe|Dream|dream|admin|encrypt|encrypted|PII|pii|secret|AKIA|session_id|harness|harness_version|source_ref|cwd|entities|Stream E|project" crates/memoryd/src crates/memoryd/tests docs/specs/stream-f-dreaming-v0.2.md docs/reviews/stream-f-observe-security-review*.md
nl -ba crates/memoryd/src/handlers.rs | sed -n '90,130p;300,530p;900,930p;1888,1900p'
nl -ba crates/memoryd/src/mcp.rs | sed -n '1,470p'
nl -ba crates/memoryd/src/protocol.rs | sed -n '70,115p;145,230p'
nl -ba crates/memory-substrate/src/api.rs | sed -n '670,745p'
nl -ba crates/memoryd/src/recall/binding.rs | sed -n '1,120p'
nl -ba crates/memoryd/tests/dream_substrate_fragments.rs | sed -n '1,430p'
nl -ba crates/memoryd/tests/privacy_e2e.rs | sed -n '1,260p'
nl -ba crates/memoryd/tests/mcp_manifest.rs | sed -n '1,260p'
nl -ba crates/memoryd/tests/mcp_forward.rs | sed -n '1,220p'
nl -ba docs/specs/stream-f-dreaming-v0.2.md | sed -n '190,260p;280,330p;340,390p;490,545p;620,650p'
nl -ba crates/memory-privacy/src/classifier.rs | sed -n '1,240p'
nl -ba crates/memory-privacy/src/decision.rs | sed -n '60,125p'
nl -ba crates/memory-privacy/src/regex.rs | sed -n '1,240p'
cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
# external read-only behavior probe built under /tmp/agent-memory-observe-probe, then:
cd /tmp/agent-memory-observe-probe && cargo run --quiet
```

Verification results:

- `cargo test -p memoryd --test dream_substrate_fragments --test mcp_manifest --test mcp_forward --test privacy_e2e`: PASS (`dream_substrate_fragments`: 11 passed; `mcp_forward`: 4 passed; `mcp_manifest`: 10 passed; `privacy_e2e`: 11 passed).
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`: PASS.
- External observe probe: FAIL for dot-separated, digit-only, and underscore-separated phone-like observe metadata values as shown under S2.

## Residual risk and confidence

Residual risk is **medium-high** until observe metadata validation rejects or normalizes all phone-like PII encodings before append. The core `text` path for secrets and PII is in good shape, and MCP admin exposure appears bounded, but metadata remains a plaintext side channel.

Confidence is **high** for the S2 finding because it is backed by source-line evidence plus a public-path behavioral probe that produced plaintext disk effects and canary leakage. Confidence is **high** for the confirmed controls above because they are covered by both source inspection and green targeted tests.
