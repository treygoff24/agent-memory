### Verdict

Changes requested

### Intended outcome

Stream G Review Gate B is intended to validate Tasks 4-7 after fixes: daemon-local state file recovery and reporting, additive Reality Check protocol and notifications, drift-risk scoring from index/event projections, and Reality Check session lifecycle handlers. The rerun specifically checks that prior findings around `observed_at`, stale `updated_at` fallback behavior, state-load reporting, mutation serialization, notification wiring, forget-reason privacy, and daemon protocol/session regressions are closed.

### Executive summary

The fixes materially improve the Gate B implementation. The prior clean-code/correctness findings are closed: `confirm` now writes an `observed_at` signal, the index hydrates `memories.observed_at` from that signal or `created_at` rather than `updated_at`, metadata-only updates no longer reset observation freshness, and state-load fallbacks now expose parse/version failure reasons. The prior security findings are also largely addressed: mutating Reality Check requests share a daemon-side lock, forget reasons are sanitized before persistence, the notification channel is stored in handler state, startup/hourly due checks are wired, and metadata-only encrypted rows are included in List/Run.

I found one remaining correctness gap: encrypted memories are now intentionally returned by Reality Check, but `confirm` and `not_relevant` still route through a plaintext-only metadata mutation helper. That means an encrypted item shown in a real Reality Check session cannot be confirmed even after the user reveals it, and not-relevant also fails despite being only a frontmatter mutation. This should be fixed or the protocol/UI should explicitly refuse those actions before presenting them as available. Requested validations all pass.

Validations run:

```bash
cargo test -p memoryd --test daemon_state_files --test doctor_mirror_health --test protocol_contract --test notification_channel --test scoring --test scheduling --test responses
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo fmt -p memoryd -- --check
cargo test -p memory-substrate --test recall_index_row_indexed_at recall_index_writes_observed_at_from_frontmatter_extra_or_created_at
```

Results: all passed.

### Findings

[Medium] [Correctness] Encrypted Reality Check items are listed but cannot be confirmed or marked not relevant

- Evidence: `crates/memoryd/src/reality_check/session.rs:171-180` now uses `query_recall_index_including_metadata_only`, so encrypted metadata-only rows are eligible for Reality Check. `crates/memoryd/src/reality_check/session.rs:220-230` renders encrypted items with an empty title and `encrypted: true`. `docs/specs/stream-g-observability-v0.1.md:912` says encrypted memories are scored and shown, and that the user can `forget` or `skip`; `confirm` and `correct` require running `memoryd reveal` first. However, `crates/memoryd/src/handlers.rs:294-311` and `crates/memoryd/src/handlers.rs:399-413` implement `confirm` and `not_relevant` through `mutate_reality_check_metadata`, and `crates/memoryd/src/handlers.rs:419-431` rejects any non-`MemoryContent::Plaintext(_)` envelope with `invalid_request: reality check metadata updates for encrypted records require an encrypted metadata API`. `read_memory_envelope` returns `MemoryContent::Ciphertext` for encrypted records in `crates/memory-substrate/src/api.rs:143-190`, and `memory_reveal` is a read/audit path, not a conversion that makes a later metadata mutation plaintext.
- Why it matters: A user can start Reality Check, receive an encrypted item, reveal/inspect it as instructed, and still be unable to confirm that it is current. The item will keep reappearing unless the user skips, defers, or forgets it. `not_relevant` is also frontmatter-only and should not require decrypting the body, but it is blocked by the same helper. This breaks the session lifecycle for a class of items the fixes deliberately put back into the review pool.
- Reasoning: The previous security review correctly identified that encrypted rows were omitted from the real handler path. The fix includes them in List/Run, but response actions were not reconciled with encrypted storage. The code path checks the content discriminator before applying frontmatter-only mutations, so there is no successful confirm/not-relevant path for encrypted memories even though scoring/session selection now surfaces them.
- Recommendation: Add an encrypted-safe metadata update path for frontmatter-only Reality Check actions, ideally in `memory-substrate` so `observed_at`, confidence, passive recall, and tags can be updated atomically without reading/decrypting body content. If that API is intentionally out of scope for Gate B, then exclude encrypted items from sessions that present confirm/not-relevant controls, or return a typed `RespondRefused { kind: RequiresReveal }`/equivalent after reveal-aware UI support exists. Add handler-level tests for encrypted `confirm` after reveal semantics and encrypted `not_relevant` behavior.
- Confidence: High

### Non-blocking simplifications

- `observed_at` is currently bridged through `frontmatter.extras["observed_at"]`. That is acceptable for closing this Gate B correctness issue because the index now hydrates it, but a typed `Frontmatter::observed_at` field would be cleaner if Stream G/I will continue to depend on this timestamp across multiple code paths.
- `sanitize_forget_reason` is intentionally conservative and local. If forget-reason policy grows, consider moving it behind a small privacy helper so the same redaction rules can be reused by CLI/web layers rather than reimplemented.

### Test gaps

- Add a handler-level test for an encrypted Reality Check item where the user reveals the memory and then confirms it; the expected behavior should be either successful encrypted-safe metadata mutation or a deliberate typed refusal that the UI can render.
- Add a handler-level test for `not_relevant` on an encrypted metadata-only item. The action mutates only frontmatter and should either succeed through an encrypted-safe metadata API or be explicitly excluded from encrypted-item affordances.
- The scheduler has unit coverage for `HandlerState::fire_reality_check_due_if_due`, but there is not yet an integration test that `serve_substrate_with` fires startup/hourly notifications through the real server-owned shared state. This is lower priority because the code path is simple and the requested scheduling tests pass.

### Questions / uncertainties

- It is unclear whether Gate B is expected to implement an encrypted metadata mutation API or defer encrypted `confirm`/`not_relevant` until a later Stream D/G surface. The current spec says encrypted items are shown and may be confirmed after reveal, so I treated the current dead-end as a Gate B correctness issue.
- I did not review Tasks 8+ UI/web/dispatcher implementation beyond checking that the Gate B notification channel and scheduling hooks exist.

### Positives

- The observed-at fix is directionally correct: `confirm` writes an observation timestamp, the index projects it into `memories.observed_at`, scoring reads `observed_at`/`created_at` instead of falling back to `updated_at`, and tests cover non-confirm metadata updates not resetting observation freshness.
- State load recovery now preserves safe startup while exposing parse/version fallback reasons via `DaemonStateLoadReport` and warning output.
- The mutating Reality Check path now uses shared handler state and a lock, and the concurrent response regression test covers the stale double-submit class called out in the security review.
