# Stream D Privacy API

## Crate surface

`memory-privacy` is a decision/classification crate. It does not mutate Stream A
state directly.

Primary types:

- `PrivacyTier::{Public, Internal, Confidential, Personal, Secret}`
- `PrivacyStorageAction::{Plaintext, EncryptAtRest, Refuse}`
- `PrivacyDecision { tier, storage_action, spans, scan }`
- `PrivacySpan { label, start, end, confidence }`
- `SafeFragmentDecision::{Allow, OmitEncryptedBodyHidden, OmitReviewPending}`
- `PrivacyClassifier`
- `PrivacyFilterProvider`
- `PrivacyEncryptor`
- `KeyProvider`
- `MaskingSession`

`PrivacyTier::classification()` maps to Stream A:

- `Public/Internal` -> `ClassificationOutcome::Trusted`
- `Confidential/Personal` -> `ClassificationOutcome::RequiresEncryption`
- `Secret` -> `ClassificationOutcome::Secret`

`PrivacyStorageAction` is the daemon's write-routing authority. Detected PII can
select `EncryptAtRest` without raising a project/agent record from `internal` to
`personal`.

`safe_plaintext_fragment(classifier, fragment)` is the Stream D helper for
Stream E passive-recall prose, hook diagnostics, and echoed CLI/error fragments.
It classifies the fragment under `PrivacyNamespace::Me`, allocates no persistent
state, and never calls reveal/decrypt logic. Results map Stream D policy to
emission safety:

- `SafeFragmentDecision::Allow` for plaintext, URL-only, date-only, or no-span
  fragments.
- `SafeFragmentDecision::OmitReviewPending` for encrypted-at-rest private or
  account-like fragments.
- `SafeFragmentDecision::OmitEncryptedBodyHidden` for refused or secret-like
  fragments, including classifier failures.

The strictest detected span wins.

`safe_descriptor_projection(classifier, text, fallback_summary, fallback_tags)`
is the companion helper for encrypted records that still need descriptor-level
findability or synthesis signal. It removes classifier spans from the source
text, emits a bounded `summary_safe` plus `tag_safe` values only after
`safe_plaintext_fragment` allows them, and falls back to caller-provided generic
descriptors when no safe content remains.

## Daemon write behavior

`memory_write`, `memory_note`, and `memory_supersede` are privacy-mediated before
Stream A mutation.

- Secret-like content in the body or persisted metadata returns a refusal/error
  before disk effects.
- Internal/public content writes plaintext through Stream A.
- Personal/confidential caller metadata and detected PII require local encryption
  key material and write through Stream A `write_encrypted`.
- URL/date spans are detected but remain plaintext by default.
- SSNs, Luhn-valid credit-card numbers, credentials, and other secret-like
  material are refused.
- Encrypted records never index raw or masked body text. They may index safe
  title/summary/tags/source references and `meta.privacy_descriptors` lookup
  hints after separate safety checks.
- `memory_reveal` explicitly decrypts an encrypted record by id with a non-empty
  reason, returns bounded plaintext to the caller, and emits an audit event
  without persisting plaintext. `memory_get` remains redacted for ciphertext.
- Encrypted `memory_forget` tombstones the encrypted record without decrypting or
  writing plaintext. Encrypted supersede replacements and encrypted review
  decisions currently fail closed until Stream A has an atomic encrypted
  lifecycle update API.
- Stream E consumes `safe_plaintext_fragment` for recall-safe prose and diagnostics.
- `memory_startup` is implemented by Stream E and never calls `memory_reveal`.

## CLI commands

```bash
memoryd privacy status --repo . --runtime .memoryd
memoryd privacy scan --text "Contact trey@example.com"
memoryd privacy scan --file ./candidate.txt
memoryd privacy scan-delta --repo .
memoryd privacy-filter status
memoryd privacy-filter install
memoryd privacy-filter enable
memoryd privacy-filter disable
memoryd device onboard --runtime .memoryd
memoryd device rotate-keys --runtime .memoryd
memoryd device revoke dev_xxx --runtime .memoryd
```

The optional Privacy Filter commands do not download model weights during tests.
They expose the provider boundary and current disabled status.

## MCP boundary

No Stream D admin/key commands are exposed as MCP tools. The MCP manifest remains
agent-facing only:
search/get/write/supersede/forget/reveal/startup/note.

## Error codes

`privacy_error` is non-retryable at the daemon protocol layer unless the operator
changes local privacy configuration, for example by onboarding key material.

## Stream F dreaming integration

Stream F is the first user of `MaskingSession` for outbound LLM prompt masking. Dreaming creates a session with `MaskingSession::new`, masks prompt text before invoking a harness CLI, restores returned text through the same session, and relies on Drop-based teardown so the in-memory token map is not persisted.

`memory_observe` also reuses Stream D's deterministic classification before substrate disk effects. Plaintext observations may append to `substrate/...`; observations requiring encryption route to `encrypted/substrate/...` with a `safe_descriptor_projection`; secret/refused content fails closed with no fragment written. Dream passes never call `memory_reveal` and never decrypt encrypted substrate.

## Runtime enforcement switches

Memorum separates the non-negotiable secret-refusal invariant from optional
local test friction. At daemon startup, `memoryd serve` loads the per-device
`local-device.yaml` privacy block and installs it once with
`memory_privacy::install_runtime_enforcement(...)`. Fresh configs and missing
runtime configs fall back to `PrivacyEnforcement::paranoid()` so alpha dogfood
and standalone library callers stay safe by default.

The local-only config shape is:

```yaml
privacy:
  classifier: true
  encryption: true
  masking: true
```

`classifier` controls the full PII classifier. When it is deliberately turned
off for local testing, `DeterministicPrivacyClassifier::new()` still runs the
always-on secret scan for AWS/GitHub/Stripe tokens, PEM private keys, JWTs,
SSNs, Luhn-valid card numbers, and credential-like high-entropy tokens; any hit
returns `PrivacyStorageAction::Refuse` before disk effects. Non-secret content
returns trusted plaintext in that explicit low-friction mode.

`encryption` controls whether labels that normally route to `EncryptAtRest`
actually take the encrypted storage path. With `classifier: true` and
`encryption: false`, the classifier still records private spans, but the
storage action is downgraded to plaintext for explicit local tests. `masking`
is reserved for masking-session consumers and is installed with the same runtime
flag set.

Environment parsing helpers accept `MEMORUM_PRIVACY_CLASSIFIER`, `MEMORUM_PRIVACY_ENCRYPTION`, and `MEMORUM_PRIVACY_MASKING` with `on/off`, `true/false`, `1/0`, or `yes/no`. These switches are per-device runtime state and must not be added to synced `config.yaml`.
