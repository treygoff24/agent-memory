### Verdict

Approve

### Intended outcome

This rerun verifies the single remaining Stream G Gate B clean-code blocker from `docs/reviews/stream-g-review-gate-b-clean-code-rerun.md`: encrypted Reality Check items must be eligible for `List`/`Run`, and frontmatter-only lifecycle actions (`Confirm` and `NotRelevant`) must work without decrypting or requiring a plaintext body. The expected behavior is that ciphertext remains preserved, metadata/frontmatter updates are applied, and Reality Check remains metadata-only for encrypted records.

### Executive summary

No material issues found. The previous blocker is closed: `confirm` and `not_relevant` now route encrypted/non-plaintext envelopes through `Substrate::update_encrypted_memory_metadata`, which mutates only metadata/frontmatter, preserves the ciphertext body and encryption envelope, validates frontmatter, atomically rewrites the encrypted markdown projection, and refreshes the encrypted metadata-only index row. Handler tests now cover encrypted listing, encrypted confirm, and encrypted not-relevant lifecycle behavior, including ciphertext preservation and event recording. The implementation is scoped and simple enough for Gate B; I found no new clean-code, correctness, or metadata-only semantics blocker in the reviewed paths.

Validations run:

```bash
cargo test -p memoryd --test responses
cargo test -p memoryd --test daemon_state_files --test doctor_mirror_health --test protocol_contract --test notification_channel --test scoring --test scheduling --test responses
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo fmt -p memoryd -- --check
cargo test -p memory-substrate --test api_write_read encrypted -- --nocapture
cargo test -p memory-substrate --test recall_index_row_indexed_at
```

Results: all passed.

### Findings

None.

### Previous blocker closure

[Closed] [Correctness] Encrypted Reality Check items can be listed/run and confirmed or marked not relevant without plaintext body

- Evidence: `crates/memoryd/src/handlers.rs:420-428` now dispatches non-plaintext `MemoryEnvelope` content to `Substrate::update_encrypted_memory_metadata` instead of rejecting encrypted records. `crates/memory-substrate/src/api.rs:684-740` reads the encrypted envelope, preserves `memory.body` and the original `extras["encryption"]` value across the caller's metadata mutation, validates frontmatter, writes atomically with `allow_encrypted_namespace: true`, and re-upserts the encrypted index projection. `crates/memoryd/tests/responses.rs:306-339` verifies encrypted `Confirm` is accepted, persists `observed_at`, bumps confidence, records `RealityCheckConfirmed`, and preserves ciphertext. `crates/memoryd/tests/responses.rs:341-369` verifies encrypted `NotRelevant` is accepted, preserves ciphertext, keeps the memory active, disables passive recall, tags the memory, records `RealityCheckNotRelevant`, and does not tombstone.
- Why it matters: Gate B intentionally includes encrypted metadata-only rows in Reality Check. Without an encrypted-safe metadata mutation path, the daemon could show encrypted items but strand users when they tried to complete the lifecycle. That dead-end is now removed for the two frontmatter-only actions in scope.
- Reasoning: The lifecycle mutation boundary is now content-aware: plaintext records continue through the existing `write_memory(AdminRepair)` path, while encrypted/metadata-only records use the new substrate API that never constructs or requires plaintext body content. The tests assert the behavior at the handler boundary, not just at a helper boundary, so the real `RealityCheckRequest::Respond` flow is covered.
- Recommendation: No blocking change. Keep `Correct`/supersession on the explicit plaintext/governance path until an encrypted supersession API exists; that is outside this blocker.
- Confidence: High

### Non-blocking simplifications

- `mutate_reality_check_metadata` is a good small seam for this Gate B fix. If more encrypted lifecycle actions are added later, consider naming the two branches explicitly (for example, plaintext rewrite vs encrypted metadata rewrite) to keep future reviewers from assuming both branches have identical event/audit semantics.

### Test gaps

None blocking for this rerun. The added handler-level encrypted `Confirm` and `NotRelevant` tests cover the prior blocker directly, and the broader `responses` test target passed. I also ran the substrate encrypted write/read and observed-at index tests as supporting coverage for the newly used substrate API and staleness projection.

### Questions / uncertainties

- I did not review unrelated Stream G/I changes outside the requested files and prior review artifacts. This review is limited to the remaining Gate B clean-code blocker and the requested validation commands.
- `Substrate::update_encrypted_memory_metadata` is currently covered through the memoryd handler tests rather than a dedicated substrate unit test for metadata mutation. That is acceptable for this blocker because the failing product path is the handler lifecycle, but a direct substrate test would be useful if this API becomes a broader public contract.

### Positives

- The fix keeps encrypted lifecycle mutation at the substrate boundary, which is the right layer for preserving ciphertext and encrypted namespace write invariants.
- The tests assert both behavior and privacy invariants: accepted response, metadata update, event emission, active/not-tombstoned status, and ciphertext preservation.
- The handler code remains simple: Reality Check actions still express only the metadata mutation they need, while storage-mode-specific details stay in `memory-substrate`.
