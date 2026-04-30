# Review Gate A — Stream A Query Extension Review

**Date:** 2026-04-30
**Scope reviewed:** current uncommitted Task 2 Stream A query-extension changes in `crates/memory-substrate/**` and `docs/api/stream-a-public-api.md`. Other dirty Stream E/Stream D files were present but were not reviewed except where needed to understand the plan/spec boundary.

## Verdict

Approve with minor follow-ups.

No P0/P1 implementation blockers found for Task 2. The change appears to meet the Stream E v0.5 Stream A substrate requirements: new filters are served by SQLite predicates over indexed columns, the recall-index API projects ranking and entity data from Stream A index/auxiliary tables, `index_body` is a real column, defaults preserve old query behavior, invalid namespace prefixes return a typed fail-closed error, and the v1->v2 migration is transactional and idempotent.

Residual risk: the narrow tests pass and are not zero-case, but coverage is compressed into two large tests and does not independently prove every match-term source. The recall-index auxiliary reads are also not performance-tested here.

## Intended outcome

Task 2 is meant to extend Stream A just enough for Stream E passive recall to collect candidate memories without hydrating every envelope: add indexed `MemoryQuery` filters, add `passive_recall`/`index_body` projections and migration behavior, add a typed invalid-query error, and expose a recall-index read API over the existing SQLite index and auxiliary tag/alias/entity tables.

## Evidence commands

```bash
cargo test -p memory-substrate --test memory_query_extension
```

Result:

```text
running 2 tests
test v1_index_migration_backfills_passive_recall_and_index_body_once ... ok
test memory_query_filters_and_recall_index_use_stream_a_index_projections ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
```

```bash
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

Result:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.21s
```

## Findings

[P2] [Tests] Recall-index matching coverage can pass with a broken match source

- Evidence: `crates/memory-substrate/tests/memory_query_extension.rs:138-177` sends one `RecallIndexQuery` with four match terms against one fixture row that has an entity id, entity alias, memory alias, and tag. `crates/memory-substrate/src/index/query.rs:797-820` combines term clauses with `OR` across tag, memory alias, entity id/label, and entity alias. Because the test row matches all sources at once, removing one source from the implementation could still leave the test green through another matching source.
- Why it matters: Stream E ranking/entity resolution depends on each auxiliary table path being reliable. A regression in only `memory_aliases` or only `memory_entity_aliases` would silently reduce recall quality while the current narrow gate still passes.
- Reasoning: The plan explicitly requires the recall-index API to match entity id, entity label/alias, memory alias, and tag through the existing auxiliary tables. The current test proves that at least one matching route can include the row, but not that each route independently works.
- Recommendation: Add table-isolated cases or parameterized subcases where each query has exactly one match term and the fixture row matches only one source at a time: tag-only, memory-alias-only, entity-id-only, entity-label-only, and entity-alias-only. Keep these as behavior tests through `Substrate::query_recall_index` or `Index::query_recall_index` rather than private SQL tests.
- Confidence: High

## Requirement checklist

- New filters served from SQLite columns/indexes, not full hydration: Pass. `query_memory` builds SQL predicates in `crates/memory-substrate/src/index/query.rs:264-278`, with status/passive/updated/namespace predicates assembled in `crates/memory-substrate/src/index/query.rs:726-750`; no envelope read path is involved.
- Recall-index API reads Stream A auxiliary entity/tag/alias tables and exposes Stream E ranking fields without envelope hydration: Pass. Public DTO/API are in `crates/memory-substrate/src/model.rs:954-1001` and `crates/memory-substrate/src/api.rs:845-848`; projection reads memory row fields plus tags/aliases/entities in `crates/memory-substrate/src/index/query.rs:281-324` and auxiliary-table helpers in `crates/memory-substrate/src/index/query.rs:863-915`.
- `index_body` served from a real column, not a hot-path `json_extract`: Pass. Real columns are in `crates/memory-substrate/src/index/schema.rs:44-46`; upsert writes both projections in `crates/memory-substrate/src/index/query.rs:453-524`; recall-index selection reads `memories.index_body` in `crates/memory-substrate/src/index/query.rs:283-287` and maps it at `crates/memory-substrate/src/index/query.rs:321-322`.
- Defaults preserve old behavior: Pass. `MemoryQuery` still derives `Default` with new fields defaulting to `None`/`false` in `crates/memory-substrate/src/model.rs:924-940`; the behavior is asserted in `crates/memory-substrate/tests/memory_query_extension.rs:69-80`.
- Invalid namespace prefixes fail closed: Pass. Prefix parsing rejects unrecognized prefixes with `SubstrateError::InvalidQuery` in `crates/memory-substrate/src/index/query.rs:837-860`; the error path is asserted in `crates/memory-substrate/tests/memory_query_extension.rs:128-136`.
- Migration is safe for existing workspaces, backfills before first post-upgrade query, and is safe on second reopen: Pass. Supported version bumps to 2 at `crates/memory-substrate/src/index/migrations.rs:10-15`; migration runs in one transaction at `crates/memory-substrate/src/index/migrations.rs:59-86`; columns are checked independently at `crates/memory-substrate/src/index/migrations.rs:89-107`; backfills occur before inserting version 2 at `crates/memory-substrate/src/index/migrations.rs:63-86`; reopen idempotence is asserted in `crates/memory-substrate/tests/memory_query_extension.rs:180-227`.
- Tests execute and do not pass with zero cases: Pass. The requested test binary ran two tests and reported `2 passed; 0 failed`.

## Non-blocking simplifications

- Consider replacing the per-row auxiliary lookups in `row_to_recall_index_row` with grouped queries or SQL aggregation if a later Stream E perf probe shows startup recall spending meaningful time in repeated tag/alias/entity reads. This is not a blocker for Task 2 because the implementation avoids envelope hydration and the current gate has no perf acceptance target.

## Test gaps

- The P2 finding above: independent coverage for each recall-index match source is missing.
- No test asserts the two new supporting index names exist after fresh DB init and after v1 migration. The implementation creates them during migration (`crates/memory-substrate/src/index/migrations.rs:79-82`), but an index-name regression would currently be caught only by code review, not by a behavior/perf gate.
- No test exercises `query_recall_index` with `updated_since` and `statuses` independently from `query_memory`; current coverage proves the corresponding `query_memory` filters and one combined recall-index status/passive/namespace/match case.

## Questions / uncertainties

- I did not run broader workspace tests beyond the two commands requested for this review gate.
- I did not verify actual SQLite query plans with `EXPLAIN QUERY PLAN`; this review is based on source inspection and the presence of real columns/indexes.

## Positives

- The migration is cleanly transactional and checks the two added columns independently, which is the right failure model for mixed or partially evolved local workspaces.
- The new query builder avoids `(? IS NULL OR ...)` patterns and keeps invalid namespace handling centralized and typed.
- The public docs accurately describe the new query fields, recall-index projection, migration behavior, and no-hot-path-JSON guarantee.

## Changed path

- `docs/reviews/stream-e-query-extension-review.md`

## Fix status — 2026-04-30

Fixed the P2 test-coverage gap in `crates/memory-substrate/tests/memory_query_extension.rs` by adding public `Substrate::query_recall_index` coverage for isolated tag, memory-alias, entity-id, entity-label, and entity-alias match sources. Also added cheap assertions for the two recall-supporting index names on fresh index init and v1 migration, plus independent recall-index `statuses` and `updated_since` filter coverage.

## Rereview after governance projection fix — 2026-04-30

**Scope:** rereviewed only the new Stream A recall-index governance projection changes: `RecallIndexRow` governance fields, schema v3 migration, upsert/reindex population from `write_policy` / `retrieval_policy`, hot-path projection from real SQLite columns, and the targeted tests/docs.

**Verdict:** approve. No P0/P1/P2 blockers found in the governance projection fix.

### Evidence

- `RecallIndexRow` now exposes the governance projections Stream E needs: `requires_user_confirmation`, `review_state`, `human_review_required`, and `max_scope` in `crates/memory-substrate/src/model.rs`.
- The recall-index SQL selects those fields from real `memories` columns and maps them directly in `crates/memory-substrate/src/index/query.rs`; the hot path does not use `json_extract` or hydrate memory envelopes.
- Upsert/reindex population flows from `frontmatter.requires_user_confirmation`, `frontmatter.review_state`, `frontmatter.write_policy.human_review_required`, and `frontmatter.retrieval_policy.max_scope` in `crates/memory-substrate/src/index/query.rs`.
- Schema v3 adds and backfills `human_review_required` / `max_scope` in `crates/memory-substrate/src/index/migrations.rs`; fresh schema includes the columns in `crates/memory-substrate/src/index/schema.rs`.
- `crates/memory-substrate/tests/memory_query_extension.rs` covers fresh index creation, v1 migration to v3, recall-index projection of the governance fields, isolated match sources, and independent recall-index status / updated-since filters.
- `docs/api/stream-a-public-api.md` documents schema v3 and the no-hot-path-JSON guarantee.

### P0 findings

None.

### P1 findings

None.

### P2 findings

None.

### Gate results

```bash
cargo test -p memory-substrate --test memory_query_extension
```

Result: passed — 5 tests, 0 failed.

```bash
cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings
```

Result: passed.
