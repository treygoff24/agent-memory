### Verdict

Approve

### Intended outcome

Review Gate A appears intended to validate the first Stream I slice before coordination-crate work begins: map the Stream I contract, surface existing Stream A recall-index columns (`indexed_at`, `source_device`) through `RecallIndexRow` without schema changes, and extend Stream E project binding parsing for optional `concurrent_session_mode`. The slice should preserve Stream G ownership of schema/EventKind work and keep claim-lock state out of canonical persistence.

### Executive summary

No material issues found. The scoped implementation exposes `RecallIndexRow::indexed_at` and `RecallIndexRow::source_device` from existing `memories` columns, parses `indexed_at` through the same typed timestamp path as `updated_at`, preserves nullable `source_device`, and does not require a new migration for this surface. The project parser updates both layers: the hand-rolled flat-mapping whitelist now accepts `concurrent_session_mode`, and the serde `deny_unknown_fields` target now carries an `Option<ConcurrentSessionMode>` with snake_case values, preserving absent-field behavior and rejecting unknown values. Targeted tests and diff whitespace checks pass. Residual risk is mostly from the broader dirty worktree containing Stream G/H changes outside this Gate A scope.

### Findings

No material issues found.

### Non-blocking simplifications

- Consider adding a focused assertion to `test_concurrent_session_mode_unknown_value_rejects` that the error string names `concurrent_session_mode` or the invalid value. The current test correctly verifies `InvalidRequest`, but a more specific assertion would better lock the public parser diagnostic promised by the spec.

### Test gaps

- `crates/memoryd/tests/project_binding_concurrent_mode.rs` covers accepted values, absent-field behavior, preparse whitelist rejection, and unknown-value rejection. It does not assert diagnostic clarity for the unknown enum value.
- The Gate A tests cover fresh writes through `upsert_memory` and `query_recall_index`; they do not cover hydration from a pre-existing SQLite row created by an older binary. That is acceptable for this slice because `indexed_at TEXT NOT NULL` and `source_device TEXT` are already present in the current shipped schema, but it remains a migration/backcompat assumption to preserve in broader gates.

### Questions / uncertainties

- The working tree contains unrelated Stream G/H changes in schema, EventKind, events-log mirror, recall-hit, original-confidence, and supersession areas. I treated those as outside the requested Gate A review except where needed for compile compatibility. This review does not approve those changes.
- I did not run full workspace fmt/clippy/test because the requested review gate names the narrower Task 1-3 surfaces and the tree contains unrelated in-flight streams.

### Positives

- The `RecallIndexRow` SELECT list and hydration order are updated together, and the test verifies `indexed_at` is local ingest time rather than authored `updated_at`.
- The parser seam is handled at both required layers, which avoids the known failure mode where the preparse whitelist rejects a valid Stream I key before serde can parse it.
- The contract map clearly records Stream G vs. Stream I ownership boundaries and preserves later review gates.

Verification run:

```text
cargo test -p memory-substrate --test recall_index_row_indexed_at
cargo test -p memory-substrate --test recall_index_row_source_device
cargo test -p memoryd --test project_binding_concurrent_mode
git diff --check -- docs/reviews/stream-i-contract-map.md crates/memory-substrate/src/model.rs crates/memory-substrate/src/index/query.rs crates/memory-substrate/tests/recall_index_row_indexed_at.rs crates/memory-substrate/tests/recall_index_row_source_device.rs crates/memoryd/src/recall/types.rs crates/memoryd/src/recall/project.rs crates/memoryd/src/recall/mod.rs crates/memoryd/tests/project_binding_concurrent_mode.rs
```
