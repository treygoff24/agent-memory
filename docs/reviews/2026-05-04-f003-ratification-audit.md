# F-003 Ratification Audit — Stream A public API additions

Date: 2026-05-04
Verdict: fully ratified

## Scope

Audited the two post-v1.1 public APIs named by F-003:

- `Substrate::update_encrypted_memory_metadata` at `crates/memory-substrate/src/api.rs:685`
- `Substrate::query_recall_index_including_metadata_only` at `crates/memory-substrate/src/api.rs:1148`

Compared against:

- `docs/specs/stream-a-core-substrate-v1.1.md` post-v1.1 authorized additions (§top amendment and public API section)
- `docs/api/stream-a-public-api.md`

## Findings

1. `update_encrypted_memory_metadata` is ratified.
   - Spec authorizes safe metadata-only mutation for encrypted canonical memory, plaintext rejection, ciphertext/envelope/path preservation, validation, and CAS-style atomic replacement.
   - API doc states the same operator-facing contract.
   - Implementation rejects plaintext envelopes, preserves body/encryption/path, validates the mutated memory, computes the current encrypted metadata hash, and writes through the existing replace path.

2. `query_recall_index_including_metadata_only` is ratified.
   - Spec authorizes encrypted metadata-only recall-index projection for observability/scoring consumers without hydration/decryption/plaintext body exposure.
   - API doc states the same safe-field boundary.
   - Implementation forwards to the index projection helper; no encrypted envelope hydration occurs in the public API method.

3. Version recommendation: keep v1.1 filename for now.
   - The amendment is explicit and dated in `stream-a-core-substrate-v1.1.md`.
   - A v1.2 rename would be cleaner semver, but it is not required for dogfood readiness because the ratification is already visible in both spec and API docs.
   - If another public Stream A API is added, bundle these amendments into `stream-a-core-substrate-v1.2.md` then.

## Gaps

No material ratification gaps found.
