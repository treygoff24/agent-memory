### Verdict

Approve

### Intended outcome

This rerun verifies that the previous Gate A contract blocker from `Frontmatter::original_confidence` fixture fallout is closed after updating affected test fixtures. It also spot-checks that the fix did not regress Stream G Gate A's Stream A/EventKind/events-log/supersession/frontmatter surfaces or Stream E RecallHit emission path.

### Executive summary

The exact previously blocking gate now passes: `cargo clippy -p memory-substrate -p memoryd --all-targets --all-features -- -D warnings` completed successfully. Focused Stream G Gate A contract tests also pass, including the event-kind additions, events-log mirror behavior, supersession projection, migration v4, original-confidence frontmatter behavior, RecallIndexRow hydration, and memoryd RecallHit emission tests. The fixture fallout fix is mechanical and appropriate: existing fixture constructors now set `original_confidence: None`, preserving pre-Stream-G fixture semantics. No new material contract regression was found.

Validation commands run:

```bash
cargo clippy -p memory-substrate -p memoryd --all-targets --all-features -- -D warnings
```

Result: passed.

```bash
cargo test -p memory-substrate --test event_kind_new_variants --test events_log_mirror --test memory_supersession_projection --test migration_v4 --test frontmatter_original_confidence --test recall_index_row_indexed_at --test recall_index_row_source_device && cargo test -p memoryd --test recall_hit_emission
```

Result: passed.

- `event_kind_new_variants`: 3 passed; 0 ignored.
- `events_log_mirror`: 6 passed; 0 ignored.
- `frontmatter_original_confidence`: 3 passed; 0 ignored.
- `memory_supersession_projection`: 3 passed; 0 ignored.
- `migration_v4`: 4 passed; 0 ignored.
- `recall_index_row_indexed_at`: 1 passed; 0 ignored.
- `recall_index_row_source_device`: 1 passed; 0 ignored.
- `recall_hit_emission`: 6 passed; 0 ignored.

Additional spot checks:

```bash
rg -n "#\[ignore\]|ignore\s*=" crates/memory-substrate/tests/event_kind_new_variants.rs crates/memory-substrate/tests/events_log_mirror.rs crates/memory-substrate/tests/memory_supersession_projection.rs crates/memory-substrate/tests/migration_v4.rs crates/memory-substrate/tests/frontmatter_original_confidence.rs crates/memory-substrate/tests/recall_index_row_indexed_at.rs crates/memory-substrate/tests/recall_index_row_source_device.rs crates/memoryd/tests/recall_hit_emission.rs || true
```

Result: no ignored focused Gate A tests found.

```bash
git diff -- crates/memory-substrate/tests/api_phase5_surface.rs crates/memory-substrate/tests/crash_matrix.rs crates/memory-substrate/tests/startup_reconciliation.rs crates/memory-substrate/tests/vector_lifecycle.rs crates/memory-substrate/tests/reindex_reconciliation.rs
```

Result: the five fixture locations called out by the prior rerun now include `original_confidence: None` in existing `Frontmatter` literals.

### Findings

No material issues found.

### Non-blocking simplifications

The repeated direct `Frontmatter` literals across integration tests remain somewhat brittle for future additive public fields. A shared test fixture constructor/builder would reduce future API-fallout churn, but this is not a blocker for the current rerun because the all-target clippy gate now proves the current literals compile.

### Test gaps

No new blocking test gaps found in this rerun. The focused Gate A tests still verify the contract surfaces that were previously reviewed: multi-device events-log mirror identity, missing-middle-row health detection, covering-index query plan, v4 open/backfill behavior, supersession CTE bound, original-confidence serde behavior, and concurrent RecallHit sequence allocation.

### Questions / uncertainties

- I did not run the full workspace test suite; this rerun was scoped to the previously blocking all-target clippy command plus the focused Stream G Gate A contract tests.
- The working tree contains broad uncommitted Stream G/H/I changes outside this narrow rerun. This review only evaluates the P1 fixture fallout closure and spot-checks the Stream G Gate A contract surfaces touched by the prior review.

### Positives

- The blocker was fixed in the safest possible way for existing fixtures: `original_confidence: None` preserves backwards-compatible/pre-Stream-G semantics.
- The exact previously failing all-target clippy command now passes.
- Focused contract tests stayed green and are not ignored, which gives good confidence that the fixture repair did not paper over a Stream G Gate A regression.
