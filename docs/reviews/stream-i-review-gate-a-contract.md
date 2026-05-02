# Stream I Review Gate A - API Contract Review

Date: 2026-05-01
Scope: Stream I Tasks 1-3 against `docs/specs/stream-i-cross-session-v0.1.md` §1.1, §8.2, §11 and `docs/plans/2026-05-01-stream-i-cross-session.md` Tasks 1-3.
Lane: API-contract review-only. No production code edited.

## Findings

### Severity 1 - Existing `RecallIndexRow` consumers in `memoryd` tests no longer compile

`RecallIndexRow` gained the required `indexed_at` and `source_device` fields, but existing downstream struct-literal fixtures in `memoryd` were not updated. This violates Task 2's invariant that existing `RecallIndexRow` consumers must continue to compile after the additive surface change, and it will block broader `memoryd` gates even though the required Gate A narrow commands pass.

Evidence:

- `crates/memoryd/tests/startup_recall_ranking.rs:220` constructs `RecallIndexRow` without `indexed_at` or `source_device`.
- `crates/memoryd/tests/startup_recall_governance.rs:183` constructs `RecallIndexRow` without `indexed_at` or `source_device`.
- Additional compile probe failed:

```text
$ cargo test -p memoryd --test startup_recall_ranking --test startup_recall_governance --no-run
error[E0063]: missing fields `indexed_at` and `source_device` in initializer of `RecallIndexRow`
   --> crates/memoryd/tests/startup_recall_ranking.rs:220:5
error[E0063]: missing fields `indexed_at` and `source_device` in initializer of `RecallIndexRow`
   --> crates/memoryd/tests/startup_recall_governance.rs:183:5
```

Recommended fix: update the affected test fixtures, preferably through a shared local `RecallIndexRow` test builder/helper, so all fixtures set a deterministic `indexed_at` and an explicit `source_device` (`None` unless a test needs attribution). Then rerun the failed compile probe plus the required Gate A commands.

### Severity 3 - Task 2 test coverage is thinner than the planned behavior matrix

The new Task 2 tests pass, but they do not implement the full planned matrix in `docs/plans/2026-05-01-stream-i-cross-session.md`:

- `recall_index_row_indexed_at.rs` has one combined test, not the three planned cases (`populated_on_recall_index_query`, `not_null_invariant`, `distinct_from_updated_at`). It checks local ingest time and distinctness for one row, but does not cover the planned two-row not-null invariant.
- `recall_index_row_source_device.rs` has one combined test with `Some("dev_a")` and `None`, not the three planned cases including distinct per-memory attribution across `dev_a`, `dev_b`, and omitted source device.

This is not a contract blocker for the production surface by itself, but it reduces regression confidence for the exact Task 2 behavior matrix. Recommended fix: either add the missing cases as named tests or update the plan/review rationale if the combined tests are intentionally accepted as equivalent.

## Contract checks

- `RecallIndexRow::indexed_at` and `source_device` surface: implemented in `crates/memory-substrate/src/model.rs` with public fields and doc comments; `query_recall_index` selects `memories.indexed_at` and `memories.source_device`; hydration parses `indexed_at` through the existing typed index-time parser and maps `source_device` from the nullable column.
- Schema migration boundary: `INDEX_SUPPORTED_SCHEMA_VERSION` is currently bumped to `4` in `crates/memory-substrate/src/index/migrations.rs`. The v4 migration adds Stream G/observability surfaces (`original_confidence`, `events_log`, `memory_supersession`) and does not add `indexed_at` or `source_device`. No `ALTER TABLE memories ADD COLUMN indexed_at` or `ADD COLUMN source_device` was found.
- Stream I Task 2 schema rule: no migration appears to have been added specifically for `indexed_at`/`source_device`; current base schema already contains `source_device TEXT` and `indexed_at TEXT NOT NULL`.
- `concurrent_session_mode` two-layer parser update: implemented in `crates/memoryd/src/recall/project.rs` by adding the key to the pre-parse whitelist and adding `Option<ConcurrentSessionMode>` to the serde target.
- `concurrent_session_mode` serde names: `ConcurrentSessionMode` uses `#[serde(rename_all = "snake_case")]` with variants `Collaborative`, `Minimal`, and `Default`, yielding the required strings `collaborative`, `minimal`, and `default`.
- Unknown `concurrent_session_mode`: rejects as `RecallError::InvalidRequest` through the project-binding parse path; covered by `test_concurrent_session_mode_unknown_value_rejects`.
- Unauthorized daemon protocol changes: no diff found in `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/mcp.rs`, or `crates/memoryd/src/client.rs`. The reviewed Task 3 change is limited to project-binding recall parser/types, which is authorized by §8.2.

## Required gates

```text
$ cargo test -p memory-substrate --test recall_index_row_indexed_at --test recall_index_row_source_device
result: PASS - 2 tests passed

$ cargo test -p memoryd --test project_binding_concurrent_mode
result: PASS - 6 tests passed

$ cargo test -p memoryd --test startup_recall_mcp
result: PASS - 5 tests passed
```

## Residual risks

- The worktree contains mixed Stream G/H/I changes, including a Stream G-owned schema-version bump. The checked DDL does not violate the Stream I `indexed_at`/`source_device` boundary, but integration should still separate ownership cleanly before merge.
- The required Gate A command set did not catch the downstream `RecallIndexRow` fixture compile break; broader package or workspace compile gates are still needed before advancing past the review gate.
