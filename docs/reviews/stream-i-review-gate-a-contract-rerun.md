# Stream I Review Gate A - API Contract Review Rerun

Date: 2026-05-01
Scope: Rerun after compile-compat fix for Stream I Tasks 1-3, focused on `RecallIndexRow::indexed_at` / `source_device` downstream consumers and the prior Gate A contract findings.
Lane: API-contract review-only. No production code edited by this rerun.

## Verdict

Approve for advancing past Gate A contract review.

The previous severity-1 compile blocker is closed. The two affected `memoryd` test fixtures now set deterministic `RecallIndexRow::indexed_at` values and explicit `source_device: None`, and the former compile-only probe now passes.

## Findings

### Severity 1 / 2

None.

### Severity 3 - Task 2 test coverage remains thinner than the planned behavior matrix

Status: still present, non-blocking.

The two Stream A surface tests still use one combined behavior test per field:

- `crates/memory-substrate/tests/recall_index_row_indexed_at.rs` verifies local ingest-time hydration and distinctness from `updated_at` for one row.
- `crates/memory-substrate/tests/recall_index_row_source_device.rs` verifies `Some("dev_a")` and `None` values across two rows.

This is enough to validate the current contract surface and the requested Gate A commands pass, but it remains thinner than the plan's named matrix:

- `indexed_at`: populated on recall-index query, not-null invariant across multiple rows, distinct from `updated_at`.
- `source_device`: populated when present, `None` when absent, distinct per-memory attribution across `dev_a`, `dev_b`, and omitted.

Recommendation: before final Stream I closeout, either expand these into the named matrix or amend the plan/review rationale explaining why the combined tests are accepted as equivalent.

## Previous blocking finding verification

Prior finding: `crates/memoryd/tests/startup_recall_ranking.rs` and `crates/memoryd/tests/startup_recall_governance.rs` constructed `RecallIndexRow` without the newly required `indexed_at` and `source_device` fields.

Current status: closed.

Evidence:

- `crates/memoryd/tests/startup_recall_ranking.rs` fixture now includes:
  - `indexed_at: instant("2026-04-20T12:00:00Z")`
  - `source_device: None`
- `crates/memoryd/tests/startup_recall_governance.rs` fixture now includes:
  - `indexed_at: instant("2026-04-30T12:00:00Z")`
  - `source_device: None`
- Static search for `RecallIndexRow {` found only the two updated test fixtures plus the production row hydration constructor.
- The previously failing compile probe passes.

## Contract checks rerun

- `RecallIndexRow::indexed_at` remains a public `DateTime<Utc>` field with Stream I local-observed-at documentation.
- `RecallIndexRow::source_device` remains a public `Option<String>` field.
- `query_recall_index` selects `memories.indexed_at` and `memories.source_device` and hydrates both into `RecallIndexRow`.
- `indexed_at` hydration uses the existing typed timestamp parser; there is no silent epoch fallback.
- `source_device` maps the nullable SQLite column directly to `Option<String>`.
- No Stream I-specific migration for `indexed_at` or `source_device` was found; the current base schema already contains `source_device TEXT` and `indexed_at TEXT NOT NULL`.
- `concurrent_session_mode` remains added to both project-binding parser layers: the pre-parse whitelist and the serde `ProjectFile` target.
- `ConcurrentSessionMode` uses snake_case serde variants for `collaborative`, `minimal`, and `default`.
- Unknown `concurrent_session_mode` values still reject as `RecallError::InvalidRequest` through the project-binding parse path.

## Required validation

```text
$ cargo test -p memoryd --test startup_recall_ranking --test startup_recall_governance --no-run
result: PASS - both test binaries compiled

$ cargo test -p memoryd --test startup_recall_ranking
result: PASS - 8 tests passed

$ cargo test -p memoryd --test startup_recall_governance
result: PASS - 6 tests passed

$ cargo test -p memory-substrate --test recall_index_row_indexed_at --test recall_index_row_source_device
result: PASS - 2 tests passed

$ cargo test -p memoryd --test project_binding_concurrent_mode
result: PASS - 6 tests passed
```

## Residual risks

- The worktree still contains broad mixed Stream G/H/I changes outside this Gate A scope. This rerun does not approve those unrelated changes.
- The Task 2 tests remain compact combined tests rather than the full named matrix from the implementation plan. This is non-blocking for Gate A because the contract surface compiles and the targeted behavior is covered, but broader closeout should either expand or explicitly accept that coverage.
