# Stream D Security Review

Date: 2026-04-29

## Review loop

Fresh-context security review found two P0 issues and three P1/P2 issues in the
initial Stream D diff. This file records the fix loop outcome.

## Findings and fixes

### P0: encrypted-tier safe projection could persist raw confidential plaintext

Initial encrypted writes always supplied a `safe_index_projection` built from
span masking. Caller-raised confidential text with no spans produced an unchanged
projection, which Stream A stored in frontmatter and FTS.

Fix: encrypted writes now pass `safe_index_projection: None` by default.
Confidential/personal records are metadata-only in indexes unless a future
projector can prove a projection is safe.

Regression: `privacy_e2e_caller_confidential_without_spans_is_metadata_only_encrypted`.

### P0: classifier scanned only body, not persisted metadata

Initial classification ignored title, summary, tags, and source references even
though those fields persist in frontmatter. Secrets could bypass refusal through
metadata, and encrypted records could leak plaintext summaries.

Fix: `classify_input_privacy` scans the full persisted input envelope. Encrypted
records persist generic summaries, no caller tags, and daemon source references.
Encrypted notes use generic summaries.

Regression: `privacy_e2e_metadata_secret_is_refused_before_disk_effects`.

### P1: file-backed age identity permissions

Initial local key onboarding used default filesystem permissions.

Fix: the development file provider now hardens the privacy key directory to
`0700`, writes key files via a private temp file with `0600`, rejects symlinked
key paths, and validates file permissions before loading on Unix.

Regression: `local_key_file_is_private`.

### P1: encrypted lifecycle gaps

Initial daemon lifecycle paths used plaintext-only reads for forget/supersede
/review. The first encrypted supersede attempt also bypassed Stream A atomicity.

Fix: Stream A id resolution and tombstone now use encrypted-aware envelopes;
`memory_forget` tombstones encrypted records without plaintext. Encrypted
supersede replacements and encrypted review decisions now fail closed until an
atomic encrypted lifecycle API exists.

Regressions: `privacy_e2e_encrypted_memory_can_be_forgotten_without_plaintext_leak`,
`privacy_e2e_encrypted_supersede_replacement_fails_closed_without_disk_effects`,
and `privacy_e2e_encrypted_review_decision_fails_closed_without_not_found`.

### P1: raw daemon admin socket

Fix: the daemon socket is chmodded owner-only after bind, and the server smoke
test asserts it is not group/world accessible on Unix. MCP still does not expose
admin privacy or review tools. Same-user capability separation remains a later
daemon-hardening item, but Stream D no longer leaves the default socket
group/world accessible.

### P2: error echoing

Still open: some invalid metadata errors can echo caller-supplied strings. This
should be cleaned up with daemon protocol hardening.

## Status

No open Stream D P0/P1 findings remain. Open residual items are finer-grained
same-user daemon capability separation and error redaction hardening outside the
core classification/encryption mutation path.
