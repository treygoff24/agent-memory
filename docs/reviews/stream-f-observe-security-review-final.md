# Stream F Gate B Security/Privacy Review Final - `memory_observe`

Date: 2026-05-01
Reviewer role: security/privacy review, read-only implementation lane
Mandatory skills used: clean-code, tdd, rust-engineer
Owned report file only: `docs/reviews/stream-f-observe-security-review-final.md`

## Verdict

**PASS.**

The previously failing S2 from `docs/reviews/stream-f-observe-security-review-closed.md` is closed. Phone-like metadata values in `entities[]`, `session_id`, `harness`, and `harness_version` are now rejected before substrate append, and the targeted behavior tests prove no plaintext or encrypted substrate fragment is written for those rejected requests.

No severity-1 or severity-2 security/privacy findings remain in the scoped `memory_observe` write surface.

## Scope reviewed

- `crates/memoryd/src/handlers.rs`
- `crates/memoryd/src/mcp.rs`
- `crates/memoryd/src/protocol.rs`
- `crates/memoryd/src/recall/binding.rs`
- `crates/memoryd/tests/dream_substrate_fragments.rs`
- `crates/memoryd/tests/privacy_e2e.rs`
- `crates/memoryd/tests/mcp_manifest.rs`
- `crates/memoryd/tests/mcp_forward.rs`
- Prior Gate B reports: `docs/reviews/stream-f-observe-security-review.md`, `docs/reviews/stream-f-observe-security-review-rerun.md`, `docs/reviews/stream-f-observe-security-review-closed.md`

## Findings by severity

None.

### Prior S2 - phone-like PII in observe metadata

**Status: closed.**

**File evidence:**

- `crates/memoryd/src/handlers.rs:326-337` validates observe text, entities, `session_id`, `harness`, and optional `harness_version` before privacy classification and before any substrate append.
- `crates/memoryd/src/handlers.rs:357-371` is the first substrate write point; it is reached only after all metadata validation and secret-text refusal complete.
- `crates/memoryd/src/handlers.rs:399-418` keeps the canonical `ent_` grammar and then calls `validate_observe_metadata_is_safe` for every entity id.
- `crates/memoryd/src/handlers.rs:421-435` validates each binding metadata field for trim, emptiness, length, safe id characters, and metadata safety before returning the value.
- `crates/memoryd/src/handlers.rs:442-457` rejects metadata if `is_safe_plaintext_for_indexing` fails or if observe canary detection finds email, AWS access key, US phone, phone-like digit sequence, GitHub token, or Stripe live-key material.
- `crates/memoryd/src/handlers.rs:467-475` still catches dashed US phone numbers.
- `crates/memoryd/src/handlers.rs:478-493` closes the previous bypass by counting 10+ digit sequences across `-`, `.`, `_`, and spaces, so `202.555.0198`, `2025550198`, and `202_555_0198` embedded in otherwise valid ids fail closed.
- `crates/memoryd/tests/dream_substrate_fragments.rs:133-152` proves `ent_202.555.0198`, `ent_2025550198`, and `ent_202_555_0198` return `invalid_request`, write no plaintext or encrypted substrate records, and leave no canary in repo/runtime plaintext.
- `crates/memoryd/tests/dream_substrate_fragments.rs:184-216` proves phone-like `session_id`, `harness`, and `harness_version` variants return `invalid_request`, write no plaintext or encrypted substrate records, and leave no canary in repo/runtime plaintext.
- `crates/memoryd/tests/dream_substrate_fragments.rs:409-418` defines the disk-effect assertion used by these tests: both `substrate/` and `encrypted/substrate/` JSONL record sets must be empty.
- `crates/memoryd/tests/dream_substrate_fragments.rs:438-455` defines the repo/runtime canary scan used to prove the rejected values do not appear on disk.

**Exploitability after fix:** Low for the reviewed bypass. A caller can still submit phone-like metadata, but the server-side handler rejects it before append. The MCP schema remains a shape/length/type contract, not a complete privacy classifier; the security boundary is the handler validation, which is now covered by behavior tests.

**Impact after fix:** No confirmed plaintext metadata leak for the phone-like variants in scope.

## Required control confirmations

### Phone-like entity ids are rejected before disk effects

**Confirmed.**

`ent_202.555.0198`, `ent_2025550198`, `ent_202_555_0198`, and similar separator variants are caught by `contains_phone_like_digit_sequence` before `append_substrate_fragment`. The targeted test covers all three named canaries and asserts `invalid_request`, no plaintext substrate records, no encrypted substrate records, and no repo/runtime canary leak.

Evidence: `crates/memoryd/src/handlers.rs:399-418`, `crates/memoryd/src/handlers.rs:442-457`, `crates/memoryd/src/handlers.rs:478-493`, `crates/memoryd/tests/dream_substrate_fragments.rs:133-152`, `crates/memoryd/tests/dream_substrate_fragments.rs:409-455`.

### Phone-like `session_id`, `harness`, and `harness_version` are rejected before disk effects

**Confirmed.**

`session_id` variants `sess_202.555.0198`, `sess_2025550198`, `sess_202_555_0198`; `harness` variants `codex_202.555.0198`, `codex_2025550198`, `codex_202_555_0198`; and matching `harness_version` variants are rejected before append. This also prevents unsafe `session_id` values from being copied into `source_ref`.

Evidence: `crates/memoryd/src/handlers.rs:328-337`, `crates/memoryd/src/handlers.rs:421-435`, `crates/memoryd/src/handlers.rs:478-493`, `crates/memoryd/src/handlers.rs:504-505`, `crates/memoryd/tests/dream_substrate_fragments.rs:184-216`, `crates/memoryd/tests/dream_substrate_fragments.rs:409-455`.

### Secret text refuses before disk effects

**Confirmed.**

Observe text is classified before append. If classification returns a refusing storage action, the handler returns a privacy error before the first substrate write call. The observe test with `AKIA1234567890ABCDEF` verifies no substrate record and no canary write.

Evidence: `crates/memoryd/src/handlers.rs:338-341`, `crates/memoryd/src/handlers.rs:357-371`, `crates/memoryd/tests/dream_substrate_fragments.rs:219-235`.

### PII text encrypts without plaintext leak

**Confirmed.**

PII-bearing observe text routes to `EncryptedSubstrate`, writes ciphertext plus safe descriptor metadata, omits raw `text` from encrypted substrate records, and the test scans repo/runtime for the raw PII and full observed text.

Evidence: `crates/memoryd/src/handlers.rs:338-356`, `crates/memoryd/src/handlers.rs:508-521`, `crates/memory-substrate/src/api.rs:717-730`, `crates/memoryd/tests/dream_substrate_fragments.rs:40-69`.

The broader Stream D privacy control for phone contact content also remains green: raw phone body content is encrypted, not searchable by raw phone, and reveal requires explicit `memory_reveal` intent.

Evidence: `crates/memoryd/tests/privacy_e2e.rs:210-310`.

### `cwd` is not persisted

**Confirmed.**

`cwd` is used for binding validation and scope derivation, but the append request does not include `cwd`. Plaintext and encrypted substrate records persist `session`, `harness`, `scope`, `entities`, `kind`, `source_ref`, privacy spans, and payload/encryption fields, not `cwd`.

Evidence: `crates/memoryd/src/handlers.rs:334-337`, `crates/memoryd/src/handlers.rs:357-371`, `crates/memory-substrate/src/api.rs:693-730`.

### `source_ref` derives only from sanitized session id

**Confirmed.**

The handler validates `session_id` through `validated_observe_binding_field` before binding resolution. `source_ref` is then generated only as `session:{binding.session_id}:memory_observe`. Because phone-like, secret-like, email-like, and unsafe-character session ids are rejected first, this no longer provides a metadata side channel for the reviewed canaries.

Evidence: `crates/memoryd/src/handlers.rs:328-337`, `crates/memoryd/src/handlers.rs:421-435`, `crates/memoryd/src/handlers.rs:449-457`, `crates/memoryd/src/handlers.rs:478-493`, `crates/memoryd/src/handlers.rs:504-505`, `crates/memoryd/tests/dream_substrate_fragments.rs:184-216`.

### Project scope comes from binding

**Confirmed.**

Observe uses Stream E binding validation instead of a hardcoded project scope. `validate_session_fields` canonicalizes `cwd`, resolves project binding, and returns the binding used by `observe_scope`. The behavior test creates two `.memory-project.yaml` bindings and verifies the resulting substrate scopes are `project:proj_alpha` and `project:proj_beta`.

Evidence: `crates/memoryd/src/handlers.rs:334-337`, `crates/memoryd/src/handlers.rs:496-501`, `crates/memoryd/src/recall/binding.rs:22-41`, `crates/memoryd/tests/dream_substrate_fragments.rs:284-307`.

### No admin dream tools via MCP

**Confirmed.**

The MCP manifest exposes exactly nine agent-facing tools and includes `memory_observe` but no dream admin tools. `ToolName::try_from` accepts only those manifest tool names. The manifest tests explicitly reject `memory_dream_now`, `memory_dream_status`, `memory_dream_enable`, and `memory_dream_disable`. Daemon protocol variants for `DreamNow`/`DreamStatus` still exist but the handler returns `not_implemented`, and they are not reachable through MCP forwarding.

Evidence: `crates/memoryd/src/mcp.rs:212-257`, `crates/memoryd/tests/mcp_manifest.rs:4-60`, `crates/memoryd/src/protocol.rs:95-100`, `crates/memoryd/src/handlers.rs:118-123`.

## Commands run

```bash
git status --short && git branch --show-current && git rev-parse --short HEAD
sed -n '1,260p' docs/reviews/stream-f-observe-security-review-closed.md
rg -n "memory_observe|phone|phone-like|source_ref|session_id|harness|harness_version|cwd|secret|PII|privacy|sanitize|entity" crates/memoryd/src/handlers.rs crates/memoryd/src/mcp.rs crates/memoryd/tests/dream_substrate_fragments.rs crates/memoryd/tests/privacy_e2e.rs
ls -la docs/reviews && rg -n "Stream F|Gate B|memory_observe|phone-like|S2|secret|source_ref|cwd|admin dream|MCP" docs/reviews
nl -ba crates/memoryd/src/handlers.rs | sed -n '300,535p'
nl -ba crates/memoryd/src/mcp.rs | sed -n '90,135p;180,265p;300,410p'
nl -ba crates/memoryd/tests/dream_substrate_fragments.rs | sed -n '1,390p'
nl -ba crates/memoryd/tests/dream_substrate_fragments.rs | sed -n '390,455p'
nl -ba crates/memoryd/tests/privacy_e2e.rs | sed -n '1,330p;360,530p;570,620p'
nl -ba crates/memory-substrate/src/api.rs | sed -n '680,740p'
nl -ba crates/memoryd/tests/mcp_manifest.rs | sed -n '1,170p'
nl -ba crates/memoryd/src/protocol.rs | sed -n '70,125p;145,230p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '100,125p'
nl -ba crates/memoryd/tests/mcp_forward.rs | sed -n '1,130p'
nl -ba crates/memoryd/tests/mcp_forward.rs | sed -n '130,230p'
nl -ba crates/memoryd/src/recall/binding.rs | sed -n '1,80p'
cargo test -p memoryd --test dream_substrate_fragments --test privacy_e2e --test mcp_manifest --test mcp_forward
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Results:

- `cargo test -p memoryd --test dream_substrate_fragments --test privacy_e2e --test mcp_manifest --test mcp_forward`: PASS (`dream_substrate_fragments`: 13 passed; `mcp_forward`: 4 passed; `mcp_manifest`: 10 passed; `privacy_e2e`: 11 passed).
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.

## Worktree note

The repo was already dirty before this review, including modified Stream F source files and untracked Stream F docs/tests. I did not edit source. The only intended write from this review is this report file.

## Residual risk and confidence

Residual risk is **low** for the reviewed Gate B `memory_observe` write surface. The remaining architectural caveat is that MCP JSON schema validation does not itself encode the phone-like/privacy canary policy for every metadata field; however, the server handler is the authoritative security boundary and now rejects the in-scope canaries before disk effects.

Confidence is **high**. The PASS verdict is backed by source-line review, targeted behavior tests through the public handler/daemon/MCP surfaces, repo/runtime canary scans, and green focused Rust gates.
