# B3 metadata amendment design

**Date:** 2026-07-15

## Decision

W5 needs to add dream-generated `abstraction` and `cues` to memories imported before those fields existed. The shipped `abstraction_compile` path uses governance supersede, so it treats unchanged import-era `file:` evidence as evidence for a new body and re-runs grounding. The rehearsal recorded 100 grounding refusals in 100 probes. This design adopts the selected metadata-amendment path: a narrow actor arm that changes only those two frontmatter keys.

The operation is not a weaker write path. It accepts `{id, expected_base_hash, abstraction, cues}` only from `memoryd-abstraction-compile`; the actor is hardcoded at that boundary. It re-reads the memory and derives namespace, storage form, and immutable state from that read. Body, identity, lifecycle, namespace, sensitivity, evidence, provenance, path, and encryption envelope cannot change. `created_at` remains fixed; a changed amendment advances `updated_at`. A stale base is a typed refusal.

## Validation and privacy

The amendment uses the existing substrate validator, `memory_substrate::frontmatter::{normalize_abstraction_cues, validate_frontmatter}`, so the W2 caps stay identical: abstraction is at most eight words and 120 characters; cues are zero to three values, each at most six words and 64 characters. It uses `handlers/governance/privacy.rs::classify_plaintext_memory`, extended for this operation to scan proposed abstraction/cues always and stored plaintext body, summary, and tags only when those plaintext fields are available. It does not decrypt solely to scan, so encrypted body content is outside the scan by construction.

The namespace comes from the stored memory. This matters for the June review-reject failure class: namespace establishes the privacy tier floor. A `me` memory must classify as `PrivacyNamespace::Me`, not as agent/default merely because the dream process is acting. If the proposed retrieval metadata would require a higher tier than the stored memory, the arm refuses. It does not silently re-tier or encrypt the memory because that would make a metadata-only action change storage, recall eligibility, and potentially the canonical location. It does not drop the fields and call the operation a success because the backfill needs a truthful per-item result. Secret material is refused before disk effects.

## Storage, event, and concurrency

Plaintext rows use a dedicated thin amend write: CAS-write the file, update the index, and append exactly one `MetadataAmended`. Encrypted rows retain `Substrate::update_encrypted_memory_metadata(id, actor, mutate)` in `crates/memory-substrate/src/api/write.rs`, the W3-hardened primitive already used by `memoryd-review` and `memoryd-reality-check`. The worker-hash comparison is inside `mutate`, so the primitive's fresh-read CAS protects it while preserving ciphertext and its envelope; the handler appends `MetadataAmended` only after mutation succeeds. The actor is `memoryd-abstraction-compile`, not an open operator-supplied string. The review actor-arm precedents are `crates/memoryd/src/handlers/review.rs:327` and `:443`.

A changed amendment executes the index update. The W2 auxiliary fence in Stream A §10.2.1 therefore replaces changed aux jobs and metadata atomically and makes old-hash vectors unservable. The F1 commit worker treats it as a normal durable canonical write. The expected base hash is compared before the idempotent check: an identical request with a stale hash is refused. An identical request with a current hash is a clean `unchanged` success with no write, event, reindex, or commit signal. A stale base or concurrent lifecycle/immutable change is refused and retried only by a fresh compile cycle.

## Deliberate boundary

The arm skips grounding re-validation because the body and evidence never change. That is the point of the arm, not a general grounding exemption. It does not create a superseding version for every retrieval annotation. Option ii would preserve version history but produces version churn for derived metadata and keeps the irrelevant grounding gate that blocked the backfill. The file diff, typed event, aux hashes, and git commit history are sufficient audit evidence for this constrained mutation.

The existing `memoryd dream abstraction-compile` command is the operator surface; no standalone free-form metadata patch command is added.
