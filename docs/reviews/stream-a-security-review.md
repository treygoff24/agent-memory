# Stream A security review

Status: release-certification candidate.

Security-critical behaviors now covered by tests:

- Plaintext writes reject `Secret` and `RequiresEncryption` before disk effects.
- Trusted plaintext writes reject confidential/personal frontmatter.
- Encrypted writes accept only `RequiresEncryption`.
- Encrypted ciphertext is derived under `encrypted/`, rejects non-memory originals, and refuses overwrite.
- Safe projections index only caller-supplied safe text; the original encrypted body is not indexed.
- Encrypted pending-index repair records clear the body when no safe projection exists.
- Pending encrypted repair replay verifies ciphertext hash and blocks startup on mismatch.
- BestEffort durability requires explicit opt-in for plaintext and encrypted writes.
- Repo-relative path validation rejects path escape attempts.
- Public `read_path` rejects repo-relative path escapes before filesystem reads.
- `privacy_scan.labels: [private_credential]` is rejected for non-quarantined plaintext writes before disk effects.
- Non-final malformed event-log lines require operator repair instead of being silently skipped.
- Plaintext Markdown under `encrypted/` is refused during startup and public reindex.
- Event-after-commit repair state is explicit and prevents unsafe write retries.
- Git commands use explicit argv and preflight validates merge-driver configuration before merge.

Final independent review: no blocking findings for the encrypted privacy boundary, repair durability, startup reconciliation, or watcher suppression scope.
