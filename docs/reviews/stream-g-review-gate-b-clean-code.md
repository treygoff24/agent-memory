### Verdict

Changes requested

### Intended outcome

Stream G Tasks 4-7 appear intended to add daemon-local Reality Check infrastructure: crash-recoverable state files, additive Reality Check protocol/notification types, drift-risk scoring from index/event projections, and session lifecycle handlers for list/run/respond/snooze/reset. The implementation should keep Reality Check out of the MCP surface, preserve daemon startup on corrupt state, surface stale events-log mirrors in doctor output, and enforce the response-action semantics in the Stream G spec.

### Executive summary

The implementation is broadly coherent and the requested Gate B commands pass, but I found one material correctness/spec-compliance issue: `confirm` does not update the `memories.observed_at` signal that the scoring formula defines as authoritative. Instead, it updates `updated_at` and scoring falls back from a permanently-NULL `observed_at` to `updated_at`, which conflates observation freshness with unrelated metadata/content updates and leaves the contract unimplemented. This should be fixed before advancing because it directly affects the drift score and the Reality Check ritual's business outcome.

Validations run:

```bash
cargo test -p memoryd --test daemon_state_files --test doctor_mirror_health --test protocol_contract --test notification_channel --test scoring --test scheduling --test responses
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo fmt -p memoryd -- --check
```

All three commands passed.

### Findings

[Medium] [Correctness] `confirm` does not update the authoritative `observed_at` field used by drift scoring

- Evidence: `docs/specs/stream-g-observability-v0.1.md:851-854` defines staleness from `m.observed_at` and says `observed_at` is updated on each `confirm`; `docs/specs/stream-g-observability-v0.1.md:950-953` repeats that `confirm` sets `memory.observed_at = now`. The implementation in `crates/memoryd/src/handlers.rs:257-260` updates `frontmatter.updated_at` and confidence only. The index writer still binds `memories.observed_at` to NULL in `crates/memory-substrate/src/index/query.rs:613-617`, and scoring compensates with `COALESCE(observed_at, updated_at)` in `crates/memoryd/src/reality_check/scoring.rs:149-158`. The test named `test_confirm_updates_observed_at_and_bumps_confidence` only asserts `updated_at` changed in `crates/memoryd/tests/responses.rs:57-77`.
- Why it matters: Reality Check's core score is meant to answer "when was this memory last observed/confirmed by the user?" If the implementation uses `updated_at` as a proxy, any unrelated metadata or content update can reset staleness and suppress a memory from future drift review even though the user did not confirm it. Conversely, the intended observed-at audit signal remains absent from the index.
- Reasoning: The scoring formula's first and highest-weight component is 0.35 \* days_since_observed_norm. Because `observed_at` is never written and scoring falls back to `updated_at`, the formula is not using the specified source of truth. This is not just naming drift: it changes which memories are considered stale and makes future updates to tags, confidence, or other metadata look like Reality Check observations.
- Recommendation: Add a real observed-at write path before shipping Gate B. At minimum, model/index the observation timestamp so `confirm` writes the current time into the same source that `memories.observed_at` hydrates from, and make scoring read `observed_at` as the authoritative field rather than silently treating all NULLs as `updated_at` for post-Stream-G memories. Update the response test to assert the persisted/indexed `observed_at` value changes on confirm and add a regression test that an unrelated metadata update does not reset staleness.
- Confidence: High

[Low] [Observability] Corrupt or version-mismatched daemon state falls back silently

- Evidence: Spec §5.8 says missing, parse-error, or version-mismatched `state.json` should log a warning while defaulting safely (`docs/specs/stream-g-observability-v0.1.md:1148`). `DaemonState::load` currently calls `load_versioned_json(...).unwrap_or_default()` in `crates/memoryd/src/state.rs:31-34`; `load_versioned_json` collapses read errors, parse errors, and version mismatches to `None` without exposing a reason to the caller.
- Why it matters: The crash-recovery behavior is safe, but operators lose the only signal that a persisted snooze or last-completed timestamp was discarded. That can make Reality Check unexpectedly due/overdue after a manual edit or partial corruption with no diagnostic trail.
- Reasoning: The plan explicitly called for warning logs on fallback. The current API cannot log accurately because it erases the failure mode. This is not a startup blocker, but it weakens operability around state-file recovery.
- Recommendation: Return or log a small load status/reason from the state-file loader for parse/version failures while continuing to default. Keep missing files quiet if desired, but warn on corrupt/version-mismatched files as the spec requires.
- Confidence: Medium

### Non-blocking simplifications

- `RcScheduler` currently stores a parsed/default cron expression but due/overdue checks are simple weekly elapsed-time checks. If cron-time semantics are deferred intentionally, consider naming the helper around cadence rather than schedule parsing, or add a comment/test that the current Gate B scope only validates the fallback expression and weekly cadence. This would prevent future readers from assuming full cron evaluation exists here.

### Test gaps

- The confirm response test does not verify `observed_at`; despite its name, it only checks `updated_at` and confidence. Add coverage for the actual persisted/indexed observed-at signal.
- Add a scoring regression test proving that a non-confirm metadata update does not reduce `days_since_observed_norm`.
- Add a state-file test that corrupt/version-mismatched `state.json` produces an operator-visible warning while still allowing default startup behavior.

### Questions / uncertainties

- The substrate frontmatter/index model currently appears not to expose `observed_at` on `Frontmatter`, while the index schema has an `observed_at` column. It is unclear whether Stream G is expected to finish that substrate surface now or whether the spec should be amended to use `updated_at`. Given the current spec text, I treated `observed_at` as the intended contract.
- I did not review the later Stream G TUI/web/dispatcher tasks; this review is limited to Tasks 4-7 and the requested Gate B files/commands.

### Positives

- State-file writes use a simple write/fsync/rename/fsync-dir pattern, and the tests cover missing, corrupt, stale, delete, and atomic-write behavior.
- The Reality Check protocol additions are additive, serde-covered, and blocked from the MCP forwarder with a stable protocol error.
- The scoring implementation avoids per-item `read_memory` calls and uses index/event projections with explicit tests for NULL harnesses, cycle-bounded supersession walks, sensitivity weights, invalid weights, and exclusion rules.
