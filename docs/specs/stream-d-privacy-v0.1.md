# Stream D Privacy Spec v0.1

## 1. Scope

Stream D supplies the privacy classification and encrypted-storage routing that
Stream C intentionally failed closed without. It classifies daemon write inputs,
refuses secret and high-risk identity-theft material before disk effects, routes
PII and caller-marked confidential/personal content through Stream A
`write_encrypted`, and exposes admin-only privacy/key commands plus an explicit
agent-facing reveal path for encrypted records.

Non-goals:

- Stream E recall-block assembly and startup context.
- Stream G dashboard/UI.
- History rewrite automation beyond a documented operator runbook.
- Downloading or running optional Privacy Filter weights in normal tests.

## 2. Classification layers

Layer 1 is always enabled and offline. It uses deterministic regex and entropy
rules for credentials, private keys, token-shaped strings, email, phone, URL,
address, date, SSNs, Luhn-valid credit-card numbers, and high-entropy generic
secrets.

The optional OpenAI Privacy Filter provider is behind a `PrivacyFilterProvider`
trait. The default provider is disabled and returns an explicit unavailable
error; tests use a fixture provider. The model is treated as one defense layer,
not as an anonymization or compliance guarantee.

## 3. Tier policy

Tier and storage routing are separate:

1. namespace default: `me -> personal`, `project/agent -> internal`;
2. caller metadata can raise but never lower;
3. Layer 1/model spans do not raise the caller-visible tier;
4. PII spans select encrypted-at-rest storage;
5. secret/high-risk identity-theft spans refuse before disk effects.

Storage routing:

- `public`, `internal` -> Stream A plaintext `write_memory` with
  `ClassificationOutcome::Trusted` unless PII spans require encrypted storage.
- `confidential`, `personal`, email, phone, address, person, and generic account
  spans -> Stream A `write_encrypted` with
  `ClassificationOutcome::RequiresEncryption`.
- URL/date spans are detected and audited but remain plaintext by default.
- Credentials/private keys/JWT/high-entropy secrets, SSNs, and Luhn-valid
  credit-card numbers -> refused before Stream A mutation.

`secret` is runtime-only and is never persisted as frontmatter sensitivity.

## 4. Encrypted tier

Encrypted writes use the `age` file-encryption crate with X25519 recipients.
Missing local key material fails closed before Stream A writes. Encrypted records
store ciphertext under `encrypted/`. v0.1 does not persist a safe body
projection from raw or masked body text. Encrypted records may index safe
caller-supplied descriptors such as title, summary, tags, source references, and
`privacy_descriptors` lookup hints after those fields are separately checked for
plaintext safety.

Persisted metadata is inside the privacy boundary. Stream D scans the write
body, title, summary, tags, source references, and privacy descriptors before
mutation. For encrypted records, unsafe frontmatter is minimized; safe
descriptors may remain so the record can be found without indexing the raw
encrypted value.

Encrypted `memory_forget` is supported as a metadata/ciphertext-preserving
tombstone update. Encrypted supersession replacements and encrypted review
decisions fail closed in v0.1 until Stream A exposes an atomic encrypted
supersession/review lifecycle API.

Encrypted `memory_reveal` is an explicit agent-facing access path. It requires a
non-empty safe reason, local key material, and an encrypted memory id; `memory_get`
continues to return a redacted body for ciphertext. Reveal returns bounded
plaintext to the caller, never writes plaintext back to the repo/index, and emits
an audit event without plaintext.

The current local file key provider is an explicit development/onboarding
boundary. Production keychain-backed providers should replace it without
changing daemon write routing.

## 5. Masked projection

Masked synthesis replaces privacy spans with stable session-local tokens such as
`Person_A` and `Email_A`. The salt table is in memory only and restore fails for
the wrong session. Restored proposals must be reclassified before write.

## 6. Acceptance signals

- `crates/memory-privacy/tests/decision_contract.rs`: tier serialization and
  Stream A classification mapping.
- `crates/memory-privacy/tests/layer1_contract.rs`: secret refusal,
  PII encrypted-at-rest routing, URL/date plaintext defaults, high-risk identity
  refusal, namespace defaults, and no caller downgrade.
- `crates/memory-privacy/tests/policy_contract.rs`: tier/storage policy.
- `crates/memory-privacy/tests/privacy_filter_contract.rs`: disabled provider
  and fixture-provider merge behavior.
- `crates/memory-privacy/tests/encryption_contract.rs`: real age roundtrip,
  nondeterministic ciphertext, private local key-file permissions, and
  missing-key fail closed.
- `crates/memory-privacy/tests/masking_contract.rs`: stable in-session tokens
  wrong-session restore failure, and non-rescanning restore.
- `crates/memoryd/tests/privacy_e2e.rs`: secret write/note/metadata refusal,
  URL/date plaintext searchability, encrypted but descriptor-findable/revealable
  contact records, no raw search/projection leakage, encrypted forget, and
  fail-closed encrypted supersede/review lifecycle gaps.
- `crates/memoryd/tests/cli_contract.rs`: Stream D admin commands parse and stay
  outside MCP.
