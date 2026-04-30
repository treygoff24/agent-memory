# Stream D Performance Review

Date: 2026-04-29

## Review loop

Performance review found no immediate Stream A regression in the tested paths,
but highlighted avoidable encrypted-write projection work and unbounded admin
scan buffering.

## Findings and fixes

### Encrypted write projection allocations

Initial encrypted writes built reversible masked projections and then indexed
those projections. Besides the security problem, this caused unnecessary body
copies and span-map work.

Fix: encrypted writes no longer build or persist safe projections by default.
This removes the hot-path masking/session allocation from daemon encrypted
writes.

### Key material reload per encrypted write

Open residual: `PrivacyEncryptor` still loads file-backed key material per
write. This is acceptable for v0.1 tests but should become daemon-state caching
when key rotation semantics are finalized.

### CLI scan buffering

Open residual: `privacy scan --file` and `privacy scan-delta` buffer full inputs.
A later hardening pass should add max-byte policy, chunked scanning, and
secret-only early exit for pre-commit use.

### Encrypted tombstone metadata read

Open residual: encrypted tombstone uses envelope reads that decode ciphertext
although lifecycle mutation only needs metadata. A metadata-only envelope read
mode would reduce churn.

## Status

No performance blocker remains for Stream D v0.1 after removing default safe
projection indexing. The remaining items are follow-up hardening work.
