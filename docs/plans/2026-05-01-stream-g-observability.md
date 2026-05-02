# Stream G Observability Implementation Plan

**Goal:** Build Stream G observability from `docs/specs/stream-g-observability-v0.1.md`: two new crates (`memoryd-tui` and `memoryd-web`), Reality Check drift-risk scoring and session lifecycle, notification dispatcher with passive/OS/external channels, trust artifact rendering, CLI command additions (`memoryd ui`, `memoryd web`, `memoryd reality-check`), slash command integrations, and all cross-stream surface additions authorized by system-v0.2 §19 — including the `EventKind::RecallHit` variant, the `events_log` covering index migration, Stream E recall-hit emission, daemon state files with crash-recovery semantics, Reality Check daemon protocol wire shapes, and the seven-variant `NotificationEvent` broadcast channel.

**Architecture:** The main Codex CLI agent is the orchestrator. Subagents do all substantive implementation, test, docs, security, performance, and review work in bounded file scopes; the orchestrator integrates, runs gates, and dispatches review/fix loops. Stream A remains the canonical repository/index substrate. Stream B remains the daemon/MCP bridge. Stream C remains governance/review authority. Stream D remains privacy/masking/encryption authority. Stream E remains recall-block assembly. Stream G adds two new top-level crates plus additive touches to Stream A, B, and E surfaces authorized in system-v0.2 §19; it does not mutate canonical memory state, modify governance logic, or add MCP tools.

**Tech Stack:** Rust 2021 workspace, `tokio`, `serde`/`serde_json`, `chrono`, `ulid`, `thiserror`, `tempfile`, `ratatui`, `crossterm`, `axum`, `esbuild` (build-time JS bundling), `reqwest` (Slack/HTTP dispatch), `lettre` (SMTP), `cron` (schedule parsing), `rust-embed` (static asset embedding), Stream A `memory-substrate`, Stream C `memory-governance`, Stream D `memory-privacy`, Stream E `memoryd::recall`, Unix-socket daemon protocol, vertical TDD, and release-gate bench fixtures.

---

## Source Contract

Normative sources:

- `docs/specs/stream-g-observability-v0.1.md` (patched 2026-05-01 — resolves five plan-reviewer blockers: trust-score column ghost references, daemon protocol wire shapes, `NotificationEvent` enum completeness, `source_count` migration, daemon state file crash recovery)
- `docs/specs/system-v0.2.md` §16 (observability), §14.3 (admin CLI), §10 (harness tiers), §19 (cross-stream surface authorizations table), §22 (product name)
- `docs/specs/stream-a-core-substrate-v1.1.md` §5.2 (runtime layout — state file additions)
- `docs/specs/stream-e-passive-recall-v0.5.md` §1–§5 (pending-attention XML shape, recall block format)
- `docs/api/stream-c-governance-api.md` (review queue and policy surfaces consumed by Stream G UI)
- Shipped Stream A–F code and docs in this repo

Do not edit or overwrite spec files unless Trey explicitly asks. This plan creates `docs/plans/2026-05-01-stream-g-observability.md` and implementation work must treat `docs/specs/stream-g-observability-v0.1.md` as the active contract.

**Note on system-v0.2 §19 drift:** The system spec's `NotificationEvent` authorization row says "six variants" but the patched Stream G spec §1.3 defines exactly seven variants (it adds `DailySynthesisSummaryReady`). The Stream G spec is the implementation contract; the system spec row is a pre-patch approximation. Do not reduce to six variants.

## Codex CLI Orchestrator Contract

The main GPT agent running in Codex CLI is the orchestrator. The orchestrator may:

1. Maintain the task DAG and current status.
2. Spawn subagents for every implementation/review/docs/perf/security lane.
3. Enforce non-overlapping owned-file scopes for parallel batches.
4. Integrate subagent changes in dependency order.
5. Resolve integration conflicts caused by accepted subagent outputs.
6. Run narrow and full gates.
7. Spawn fix subagents for every blocking review finding.

The orchestrator must not casually implement feature code directly. If a gate fails or a review finding appears, create a bounded fix task and assign it to the correct subagent with the mandatory skills below. Tiny mechanical plan/doc integration edits are acceptable for the orchestrator; feature code is not.

## Mandatory Skills For Every Subagent

Every implementation, test, docs, review, security, performance, and QA subagent prompt must include this exact line:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
```

Every subagent must load the repo-local Rust skill:

```text
Load /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md.
```

Review subagents must also explicitly load `clean-code` and apply it as a review lens. Implementation subagents must use vertical TDD: one failing behavior test, narrow RED command, minimal implementation, narrow GREEN command, refactor only while green.

### Required Subagent Prompt Preamble

Use this preamble for every subagent:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Load /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md.
Repository: /Users/treygoff/Code/agent-memory.
You are working on Stream G observability from docs/specs/stream-g-observability-v0.1.md. Treat Stream A as the only canonical substrate/index, Stream B as daemon/MCP, Stream C as governance/review, Stream D as privacy/masking/encryption, Stream E as recall assembly, and Stream F as dreaming. Use vertical TDD: write one failing behavior test, run it and record the RED failure, implement the smallest correct slice, rerun the narrow gate to GREEN, then refactor only while green. Do not touch files outside your Owned files. Do not edit spec files unless the task explicitly owns a docs amendment.
```

For review subagents append:

```text
This is a review-only lane unless explicitly assigned a fix task. Lead with findings ordered by severity. Apply clean-code review criteria plus Rust correctness, async safety, privacy, test quality, and spec compliance. If there are no findings, say so and list residual risks.
```

## Parallelization And Review Cadence

Parallel work is allowed only when owned files do not overlap inside the batch. The orchestrator must run a batch-specific owned-file duplicate check before spawning any parallel implementation batch.

Full-plan owned-file duplicates are expected because sequential tasks touch shared choke points such as `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/cli.rs`, and docs. Duplicates are forbidden only inside a parallel batch.

Batch duplicate check template:

```bash
cat > /tmp/stream-g-batch-owned-files.txt <<'LIST'
Task X: path/to/file.rs
Task Y: path/to/other.rs
LIST
cut -d: -f2- /tmp/stream-g-batch-owned-files.txt \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | sort \
  | uniq -d
```

Expected for each parallel batch: no output.

### Review Gates

- **Review Gate A — Stream A + Stream E surfaces:** after Tasks 1–3. Clean-code + API-contract reviewers inspect the `EventKind::RecallHit` variant, covering index migration, and Stream E recall-hit emission before daemon/scoring work starts.
- **Review Gate B — Daemon protocol + RC scoring + state files:** after Tasks 4–7. Clean-code + security reviewers inspect daemon protocol wire shapes, state file crash recovery, and drift-score formula correctness.
- **Review Gate C — Notification dispatcher + TUI framework:** after Tasks 8–12. Clean-code + correctness reviewers inspect notification routing (no memory content in payloads), TUI rendering, and trust artifact widget.
- **Review Gate D — Web dashboard + CLI:** after Tasks 13–16. Clean-code + security reviewers inspect CSRF enforcement, localhost-only binding, and all CLI exit codes.
- **Final Review Gate E:** after Tasks 17–18. Independent clean-code, security, performance, API contract, and docs reviewers run before final gates.

Every review gate must produce a file in `docs/reviews/` or a concise orchestrator-captured report. All severity-1/2 findings must be fixed by scoped fix subagents, the same review lane must rerun, and severity-3 findings must either be fixed or logged with rationale before advancing.

---

## Task 1: Contract Map, Worktree Baseline, And Dirty-Tree Check

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Also use `spec-quality-checklist`.
**Parallel:** no
**Blocked by:** none
**Owned files:** `docs/reviews/stream-g-contract-map.md`, `docs/plans/2026-05-01-stream-g-observability.md`
**Invariants:** Do not edit `docs/specs/stream-g-observability-v0.1.md`. Do not weaken acceptance signals.
**Out of scope:** Production code.

**Files:**

- Create: `docs/reviews/stream-g-contract-map.md`
- Modify: `docs/plans/2026-05-01-stream-g-observability.md` only if this plan contradicts v0.1

**Steps:**

1. Write `docs/reviews/stream-g-contract-map.md` mapping every Stream G v0.1 acceptance bullet (§10) to an implementation task, owned files, and narrow gate. Record every cross-stream surface addition from §1.3 explicitly: which task lands it, which test covers it, what the narrow gate command is.
2. Capture current dirty-tree baseline:
   ```bash
   git status --short
   git log --oneline -5
   ```
3. Verify Stream G v0.1 key terms are present in the spec:
   ```bash
   rg -n "RecallHit|NotificationEvent|RealityCheckRequest|RealityCheckResponse|state\.json|reality-check-pending|reality-check-session|covering index|memoryd-tui|memoryd-web|drift.*score|score.*formula|CSRF|broadcast" docs/specs/stream-g-observability-v0.1.md
   ```
4. Check current code surfaces for choke points:
   ```bash
   rg -n "RequestPayload|ResponsePayload|StatusResponse|EventKind|recall_hit|RecallStatusCounters|NotificationEvent|RealityCheck" crates
   ```
5. Verify no `source_count` column reference remains in the spec (confirmed dropped per §1.3 and §12.3):
   ```bash
   rg -n "source_count" docs/specs/stream-g-observability-v0.1.md && echo "FOUND — spec still references dropped column" || echo "clean"
   ```

**Verification plan:**

- Primary: human-readable contract map covers all Stream G §10 acceptance bullets and all §1.3 cross-stream surface additions.
- Secondary: `rg -n "TBD|TODO|unclear|not covered" docs/reviews/stream-g-contract-map.md` returns no unresolved blockers except explicit implementation tasks.

---

## Task 2: Stream A Surface — Five New `EventKind` Variants, `events_log` + `memory_supersession` SQLite Mirror Tables, `original_confidence` Frontmatter Field, And Migration v4

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memory-substrate/src/events/log.rs`, `crates/memory-substrate/src/index/migrations.rs`, `crates/memory-substrate/src/index/schema.rs`, `crates/memory-substrate/src/index/query.rs` (only the `sync_auxiliary_tables` path — extend to sync supersession edges; do not modify any other function), `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/api.rs` (only if events_log dual-write hooks land here; also add `events_log_mirror_health` helper export), `crates/memory-substrate/tests/event_kind_new_variants.rs`, `crates/memory-substrate/tests/events_log_mirror.rs`, `crates/memory-substrate/tests/memory_supersession_projection.rs`, `crates/memory-substrate/tests/migration_v4.rs`, `crates/memory-substrate/tests/frontmatter_original_confidence.rs`, `docs/api/stream-a-public-api.md`
**Invariants:** All five new `EventKind` variants are additive — no existing variant is renamed or removed. The shipped per-device JSONL events log remains canonical; the new `events_log` SQLite table is a derived projection rebuildable by `memoryd doctor --reindex`. The `memory_supersession(memory_id, supersedes_id)` join table is also a derived projection from `Frontmatter.supersedes` — it is the supersession-chain primitive the rest of Stream G's drift scoring depends on. **Without this table, Task 6's recursive CTE has nothing to walk; both must land here.** Migration v4 bumps `INDEX_SUPPORTED_SCHEMA_VERSION` from 3 to 4 using the existing `schema_migrations` table pattern (NOT `PRAGMA user_version`), and uses `add_column_if_missing` for the new `original_confidence REAL` column on `memories` (matching the existing migration idiom — `SCHEMA_SQL` only governs fresh DB creation). The `Frontmatter::original_confidence` field is `Option<f64>` for backward compatibility; missing on read = `None`. The `Substrate::events_log_mirror_health()` helper exposes `(jsonl_max_seq, sqlite_max_seq, lag)` so `memoryd doctor` can detect a stale SQLite mirror — the dual-write fail-soft mode (JSONL succeeds, SQLite fails, `WARN` logged) leaves the mirror behind silently otherwise. Existing event log tests stay green.
**Out of scope:** Stream E emission of `RecallHit` (Task 3). Stream G scoring queries (Task 6). Stream I `EventKind::ClaimLockContention` is included in this task's variant additions per inter-stream coordination, but Stream I owns wiring its emission in their plan. Doctor-side wiring of the mirror-health finding into `DoctorResponse` lands in Task 4 (owns `main.rs`/`state.rs`); this task only exposes the substrate-side helper.

**Files:**

- Modify: `crates/memory-substrate/src/events/log.rs` (add five EventKind variants + dual-write to SQLite mirror)
- Modify: `crates/memory-substrate/src/index/migrations.rs` (add `migrate_v4` function + bump `INDEX_SUPPORTED_SCHEMA_VERSION = 4`; backfill `events_log` from JSONL + `memory_supersession` from existing memories' `frontmatter.supersedes`; `add_column_if_missing` for `original_confidence`)
- Modify: `crates/memory-substrate/src/index/schema.rs` (add `events_log` + `memory_supersession` tables + covering indexes in `SCHEMA_SQL`; idempotent for fresh DBs; add `original_confidence REAL` to `memories` for fresh DBs)
- Modify: `crates/memory-substrate/src/index/query.rs` (extend `sync_auxiliary_tables` to also sync supersession edges from `memory.frontmatter.supersedes`; add `query_events_log_mirror_health` helper)
- Modify: `crates/memory-substrate/src/model.rs` (add `original_confidence: Option<f64>` to `Frontmatter`)
- Modify: `crates/memory-substrate/src/api.rs` (export `events_log_mirror_health()` returning `EventsLogMirrorHealth { jsonl_max_seq: u64, sqlite_max_seq: u64, lag: u64 }`; if events_log dual-write seam belongs in the public API surface, land it here; else keep the seam inside `events::log::append`)
- Test: `crates/memory-substrate/tests/event_kind_new_variants.rs`
- Test: `crates/memory-substrate/tests/events_log_mirror.rs`
- Test: `crates/memory-substrate/tests/memory_supersession_projection.rs`
- Test: `crates/memory-substrate/tests/migration_v4.rs`
- Test: `crates/memory-substrate/tests/frontmatter_original_confidence.rs`
- Docs: `docs/api/stream-a-public-api.md`

**Step 1: RED tests**

Create `crates/memory-substrate/tests/event_kind_new_variants.rs`:

- `test_recall_hit_round_trips_serde`: serialize and deserialize `EventKind::RecallHit { id, recalled_at }`; assert field-level equality.
- `test_reality_check_variants_round_trip_serde`: same for `RealityCheckConfirmed { id, session_id }`, `RealityCheckForgotten { id, session_id, reason }`, `RealityCheckNotRelevant { id, session_id }`.
- `test_claim_lock_contention_round_trips_serde`: same for `ClaimLockContention { memory_id, holder, contender }` (Stream I's variant; included here per inter-stream coordination).
- `test_existing_event_kinds_unchanged`: assert `EventKind::WriteCommitted`, `EncryptedWriteCommitted`, `TombstoneCommitted`, `SubstrateFragmentWritten`, `EncryptedContentRevealed`, and the other shipped variants still serialize to their pre-Stream-G JSON shapes (use a snapshot fixture under `tests/fixtures/eventkind_pre_stream_g.json`).

Create `crates/memory-substrate/tests/events_log_mirror.rs`:

- `test_events_log_table_exists_after_migration`: open a fresh SQLite index; assert `events_log` table exists with columns `(seq, kind, memory_id, ts, payload_json)`.
- `test_covering_index_exists`: assert `PRAGMA index_list(events_log)` includes `idx_events_log_kind_memory_ts` whose columns are `(kind, memory_id, ts)` in that order.
- `test_dual_write_appends_to_both_jsonl_and_sqlite`: append a `RecallHit` event via `events::log::append`; assert both the JSONL file and the SQLite `events_log` table contain the row with matching `kind`, `memory_id`, and `ts`.
- `test_recall_hit_count_query_uses_covering_index`: run `EXPLAIN QUERY PLAN SELECT COUNT(*) FROM events_log WHERE kind = 'recall_hit' AND memory_id = ? AND ts > ?`; assert the plan uses the covering index (`SEARCH events_log USING INDEX idx_events_log_kind_memory_ts`).
- `test_doctor_reindex_rebuilds_sqlite_from_jsonl`: simulate a corrupt SQLite mirror (drop the table, re-create empty); run `Substrate::doctor_reindex_events_log()` (new API); assert the table is rebuilt from JSONL files in event-id order.
- `test_dual_write_failure_leaves_jsonl_canonical`: inject a SQLite-write failure (e.g., open the connection in read-only mode for the dual-write call site); append a `RecallHit`; assert (a) the JSONL line is present, (b) the SQLite `events_log` row is missing, (c) a WARN was emitted, (d) `events_log_mirror_health().lag` is exactly 1.
- `test_events_log_mirror_health_zero_when_in_sync`: append three events normally; assert `events_log_mirror_health()` returns `lag = 0`, `jsonl_max_seq == sqlite_max_seq`.
- `test_events_log_mirror_health_reports_lag_after_failed_dual_write`: append five events with the third dual-write failing as above; assert `lag = 3` (one failed + two later events that go through fine on JSONL but the SQLite seq stayed at the value before the failure if dual-write is per-event transactional, OR `lag = 1` if subsequent writes catch up — pick the semantics in implementation and lock the test to it).

Create `crates/memory-substrate/tests/memory_supersession_projection.rs`:

- `test_memory_supersession_table_exists_after_migration`: open a fresh SQLite index; assert `memory_supersession` table exists with columns `(memory_id TEXT NOT NULL, supersedes_id TEXT NOT NULL, PRIMARY KEY(memory_id, supersedes_id))` and the foreign-key constraints to `memories(id) ON DELETE CASCADE` are present on both columns.
- `test_index_on_supersedes_id_exists`: assert `idx_memory_supersession_supersedes_id ON memory_supersession(supersedes_id)` is present (so reverse lookups for "what memories supersede this one" are fast).
- `test_sync_auxiliary_tables_writes_supersession_edges`: build a `Memory` with `frontmatter.supersedes = vec![id_a, id_b]`; call `sync_auxiliary_tables`; assert two rows exist in `memory_supersession` with `(memory_id = self_id, supersedes_id ∈ {id_a, id_b})`.
- `test_sync_auxiliary_tables_replaces_supersession_edges`: write a memory twice — first with `supersedes = [id_a]`, then with `supersedes = [id_b]`; assert only the `(self_id, id_b)` row remains (wholesale replacement matches the tags/aliases/entities pattern).
- `test_migrate_v4_backfills_memory_supersession_from_existing_memories`: pre-seed three memories whose frontmatter has `supersedes` lists referring to each other; run migration v4; assert all expected `(memory_id, supersedes_id)` edges exist after migration.
- `test_memory_supersession_recursive_cte_walks_chain_depth_bounded`: build a chain a→b→c→d→...→j (10 deep); run the bounded recursive CTE specified in §5.1 of the Stream G spec with `WHERE depth < 8`; assert recursion stops at exactly 8 hops without panicking and without infinite-looping if the chain has a cycle (insert a→b→a edge to test the cycle case; recursion must terminate via the depth bound).

Create `crates/memory-substrate/tests/migration_v4.rs`:

- `test_index_supported_schema_version_is_4`: assert `INDEX_SUPPORTED_SCHEMA_VERSION == 4`.
- `test_migrate_v3_to_v4_creates_events_log`: open a v3 DB (insert `INTO schema_migrations(version) VALUES (3)`); call `migrate_schema`; assert `SELECT MAX(version) FROM schema_migrations` returns `4` AND `events_log` table exists.
- `test_migrate_v3_to_v4_creates_memory_supersession`: same setup; assert `memory_supersession` table exists post-migration.
- `test_migrate_v3_to_v4_adds_original_confidence_column`: same setup; assert `PRAGMA table_info(memories)` reports an `original_confidence` column with type `REAL` and `NOT NULL = 0` (nullable).
- `test_migrate_v3_to_v4_backfills_jsonl_events`: pre-seed `events/<device>.jsonl` with three events (one `WriteCommitted`, one `TombstoneCommitted`, one `RecallHit`); run migration; assert `SELECT COUNT(*) FROM events_log` returns 3 with matching `kind` values and original `ts` order preserved.
- `test_migrate_v3_to_v4_backfills_memory_supersession_from_frontmatter`: pre-seed two memories whose YAML frontmatter contains `supersedes:` lists; run migration; assert the corresponding edges exist in `memory_supersession`.
- `test_migrate_v4_idempotent`: run migration on an already-v4 DB; assert no errors, no duplicate rows in `events_log` or `memory_supersession`, no duplicate column-add error for `original_confidence` (the `add_column_if_missing` guard must be exercised).
- `test_migration_uses_schema_migrations_table_not_pragma`: assert `PRAGMA user_version` is unchanged (not used); `SELECT MAX(version) FROM schema_migrations` is `4`.

Create `crates/memory-substrate/tests/frontmatter_original_confidence.rs`:

- `test_frontmatter_round_trips_with_original_confidence`: serialize a `Frontmatter` with `original_confidence: Some(0.92)`; deserialize; assert equality.
- `test_frontmatter_round_trips_without_original_confidence`: serialize a YAML frontmatter that omits the field; deserialize; assert `original_confidence == None`.
- `test_pre_stream_g_memories_parse_with_none`: parse a fixture YAML at `tests/fixtures/pre_stream_g_memory.md` (no `original_confidence` key); assert `frontmatter.original_confidence == None`.

Run:

```bash
cargo test -p memory-substrate --test event_kind_new_variants
cargo test -p memory-substrate --test events_log_mirror
cargo test -p memory-substrate --test memory_supersession_projection
cargo test -p memory-substrate --test migration_v4
cargo test -p memory-substrate --test frontmatter_original_confidence
```

Expected: FAIL — none of the new variants, tables, or field exist yet.

**Step 2: GREEN implementation**

- **`events/log.rs`:** add the five new variants to `EventKind` (`RecallHit`, `RealityCheckConfirmed`, `RealityCheckForgotten`, `RealityCheckNotRelevant`, `ClaimLockContention`). Update `events::log::append` to dual-write: first append the JSON line to the per-device JSONL file (existing path, canonical), then upsert a row into the SQLite `events_log` table with `seq` as the JSONL sequence number, `kind` as the snake_case discriminant, `memory_id` extracted from the variant payload (or NULL for variants without one), `ts` as the event timestamp, and `payload_json` as the full JSON-encoded variant data. The dual-write must be transaction-bracketed: SQLite write inside a `BEGIN IMMEDIATE` transaction that commits only after the JSONL write succeeded; if SQLite write fails, log the error but do not roll back the JSONL append (JSONL is canonical and `doctor --reindex` recovers SQLite from JSONL). The fail-soft mode produces a stale mirror, which is the motivation for `events_log_mirror_health()`.
- **`index/schema.rs`:** add `CREATE TABLE IF NOT EXISTS events_log (seq INTEGER PRIMARY KEY, kind TEXT NOT NULL, memory_id TEXT, ts TEXT NOT NULL, payload_json TEXT NOT NULL CHECK (json_valid(payload_json)));`, `CREATE INDEX IF NOT EXISTS idx_events_log_kind_memory_ts ON events_log(kind, memory_id, ts);`, `CREATE TABLE IF NOT EXISTS memory_supersession (memory_id TEXT NOT NULL, supersedes_id TEXT NOT NULL, PRIMARY KEY(memory_id, supersedes_id), FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE, FOREIGN KEY(supersedes_id) REFERENCES memories(id) ON DELETE CASCADE);`, and `CREATE INDEX IF NOT EXISTS idx_memory_supersession_supersedes_id ON memory_supersession(supersedes_id);` to `SCHEMA_SQL`. Add `original_confidence REAL` to the `memories` column list (nullable; positioned after `confidence REAL NOT NULL`). All idempotent for fresh DBs.
- **`index/query.rs`:** extend `sync_auxiliary_tables` to also delete-then-insert supersession edges from `memory.frontmatter.supersedes` (mirrors the `sync_tags`/`sync_aliases`/`sync_entities`/`sync_evidence` pattern). Update the doc-comment that currently says "Deferred: memory_supersession, memory_related, memory_regressions tables" to drop `memory_supersession` from the deferred list and reference Stream G plan Task 2. Add `pub fn query_events_log_mirror_health(connection: &Connection, jsonl_max_seq: u64) -> rusqlite::Result<EventsLogMirrorHealth>` that returns `EventsLogMirrorHealth { jsonl_max_seq, sqlite_max_seq, lag: jsonl_max_seq.saturating_sub(sqlite_max_seq) }`. Update the `memories` SELECT/INSERT projection to read/write `original_confidence` (NULL on read = `None` on the Frontmatter; `Some(f)` on the Frontmatter writes the REAL value).
- **`index/migrations.rs`:** bump `INDEX_SUPPORTED_SCHEMA_VERSION` from 3 to 4. Add `migrate_v4(connection: &mut Connection) -> rusqlite::Result<()>`: (a) `add_column_if_missing(&tx, "original_confidence", "REAL")` — call the existing 3-argument helper at `crates/memory-substrate/src/index/migrations.rs:121` (signature is `fn add_column_if_missing(tx: &Transaction<'_>, column: &'static str, definition: &'static str)`; the helper is hardcoded to the `memories` table, do NOT pass a table-name argument and do NOT generalize the helper here — it works for `memories` and that's all this migration needs). The helper guards re-runs on a v4 DB so the column-add is idempotent; (b) `CREATE TABLE IF NOT EXISTS events_log (...)` + covering index (idempotent guards); (c) `CREATE TABLE IF NOT EXISTS memory_supersession (...)` + reverse-lookup index; (d) backfill `events_log` by iterating every `events/*.jsonl` file in the runtime root via the existing JSONL reader API, parse each event, insert into `events_log`; (e) backfill `memory_supersession` by iterating `memories` and parsing each row's `frontmatter_json` for `supersedes` (the column is already populated post-Stream-A); (f) backfill `memories.original_confidence` from each row's `frontmatter_json` (where present); (g) `INSERT OR IGNORE INTO schema_migrations(version) VALUES (4)` and commit. Add the dispatch line `if found < 4 { migrate_v4(connection)?; }` in `migrate_schema`.
- **`model.rs`:** add `pub original_confidence: Option<f64>` to `Frontmatter` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- **`api.rs`:** add `pub fn events_log_mirror_health(&self) -> Result<EventsLogMirrorHealth, ApiError>` on `Substrate`. The implementation reads JSONL max sequence from the existing event-log machinery and SQLite max sequence via `query_events_log_mirror_health`. Export `EventsLogMirrorHealth` as a public type. Task 4 (Daemon State Files) wires this into `DoctorResponse` so a stale mirror surfaces as a `events_log_mirror_lag` `DoctorFinding` rather than being silently invisible.
- **`docs/api/stream-a-public-api.md`:** document the five new variants, the `events_log` mirror table, the `memory_supersession` derived projection, the `original_confidence` field, the `events_log_mirror_health` helper, and the dual-write semantics. Reference system-v0.2 §19 as the authorization.

**Step 3: GREEN command**

```bash
cargo test -p memory-substrate --test event_kind_new_variants
cargo test -p memory-substrate --test events_log_mirror
cargo test -p memory-substrate --test memory_supersession_projection
cargo test -p memory-substrate --test migration_v4
cargo test -p memory-substrate --test frontmatter_original_confidence
cargo test -p memory-substrate --test event_log_identity
cargo test -p memory-substrate --test event_log_recovery
```

**Verification plan:**

- Primary: the five new test files plus `event_log_identity` and `event_log_recovery` (must stay green).
- Secondary: `cargo test -p memory-substrate --test event_kind_schema` (existing — must stay green).
- Tertiary: `bash scripts/check.sh` is reserved for trunk integration; do not run inside this worktree.

---

## Task 3: Stream E Surface — Emit `RecallHit` From Recall Response Builders

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 2
**Owned files:** `crates/memoryd/src/recall/startup.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/tests/recall_hit_emission.rs`
**Invariants:** Startup recall output XML is byte-for-byte unchanged from the Stream E baseline (same `<memory-recall version="stream-e-v0.5">` header, same item ordering). Emission is deduplicated within a single response — a memory cited twice in one block produces one `RecallHit` event. Encrypted or body-disabled memories included in the recall block still emit `RecallHit`. This is an additive change to Stream E code; Stream E's hot-path behavior is preserved exactly.
**Out of scope:** Scoring queries over `RecallHit` events (Task 6). Any modification to recall block XML shapes.

**Files:**

- Modify: `crates/memoryd/src/recall/startup.rs`
- Modify: `crates/memoryd/src/recall/render.rs`
- Test: `crates/memoryd/tests/recall_hit_emission.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/recall_hit_emission.rs`:

- `test_startup_recall_emits_recall_hit_per_memory`: mock a startup recall response with 3 included memories; assert the event log receives exactly 3 `RecallHit` events, one per memory id.
- `test_delta_recall_emits_recall_hit_per_memory`: mock a delta recall response with 2 included memories; assert 2 `RecallHit` events.
- `test_recall_hit_deduped_within_response`: mock a scenario where the same memory id appears twice in a single recall block (e.g., in different sections); assert only 1 `RecallHit` event emitted.
- `test_encrypted_memory_emits_recall_hit`: a memory marked `encrypted: true` included in recall; assert `RecallHit` still emitted (no content needed, just the id).
- `test_recall_output_xml_unchanged`: run a startup recall with 5 memories; assert the XML output is byte-identical to the Stream E baseline (use an existing determinism fixture if one exists, or add a minimal fixture).

Run:

```bash
cargo test -p memoryd --test recall_hit_emission
```

Expected: FAIL because `RecallHit` emission is not yet wired.

**Step 2: GREEN implementation**

- After the final `Vec<MemoryId>` of included memories is assembled by `render.rs` / `startup.rs`, collect the deduplicated set and emit one `EventKind::RecallHit { id, recalled_at: Utc::now() }` per id via the substrate event log. Deduplication: use a `HashSet<MemoryId>` over the included list before emitting.
- Emission is fire-and-forget (best-effort): if the event log write fails, log a `WARN` and continue. Do not fail the recall response.
- Do not modify the XML output shape or the Stream E version string.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test recall_hit_emission
cargo test -p memoryd --test startup_recall_mcp
cargo test -p memoryd --test startup_recall_determinism
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test recall_hit_emission`
- Secondary: `cargo test -p memoryd --test startup_recall_privacy --test recall_cli`

---

## Review Gate A: Stream A + Stream E Cross-Stream Surface Review

**Subagent types:** `reviewer`, `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 1–3 integrated and green
**Owned files:** `docs/reviews/stream-g-review-gate-a-clean-code.md`, `docs/reviews/stream-g-review-gate-a-contract.md`
**Invariants:** Review only. Do not edit production code.
**Out of scope:** Daemon protocol, scoring, TUI, web.

**Review lanes:**

1. **Clean-code/Rust review:** inspect Tasks 2–3 diffs for naming, module boundaries, error types, no ad hoc IO, no overbroad functions. Verify `RecallHit` emission is fire-and-forget and does not block the recall hot path.
2. **Contract/API review:** verify every Stream A/E §1.3 invariant is represented in tests — particularly: `RecallHit` variant round-trips correctly, covering index query plan does not full-scan, recall XML unchanged, deduplication within response, encrypted memories still emit. Cross-check against system-v0.2 §19 authorization table.

**Commands reviewers should run:**

```bash
cargo test -p memory-substrate --test event_kind_recall_hit --test index_covering_index
cargo test -p memoryd --test recall_hit_emission --test startup_recall_mcp --test startup_recall_determinism
cargo fmt --all -- --check
cargo clippy -p memory-substrate -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 4.

---

## Task 4: Daemon State Files, Crash Recovery Primitives, And Mirror-Health Doctor Wiring

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Review Gate A
**Owned files:** `crates/memoryd/src/state.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/handlers.rs` (only the existing `doctor_response` function — append a single mirror-health finding; do not modify any other handler), `crates/memoryd/tests/daemon_state_files.rs`, `crates/memoryd/tests/doctor_mirror_health.rs`
**Invariants:** Missing, corrupt, or version-mismatched state files never prevent daemon startup. All three state files use `tempfile-then-rename` atomic writes. Files are per-device, not in the git-synced memory repo. Session files older than 7 days are auto-discarded on startup. **`doctor_response` calls `Substrate::events_log_mirror_health()` (added in Task 2); when `lag > 0` it appends a `DoctorFinding { code: "events_log_mirror_lag", message: "<n> events not mirrored to SQLite — drift scoring may be stale; run `memoryd doctor --reindex`", repair: Some("memoryd doctor --reindex") }` and sets `healthy = false`.** Without this wiring, the dual-write fail-soft mode produces a silently stale mirror that corrupts Task 6's drift scores. See spec §5.8 for full crash-recovery semantics.
**Out of scope:** Reality Check scoring and session handler logic (Tasks 6–7). Notification dispatcher (Task 8). Adding new request variants to handlers (Task 7 owns that).

**Files:**

- Create: `crates/memoryd/src/state.rs`
- Modify: `crates/memoryd/src/main.rs`
- Modify: `crates/memoryd/src/handlers.rs` (only `doctor_response` — append the mirror-health finding emission described in invariants)
- Test: `crates/memoryd/tests/daemon_state_files.rs`
- Test: `crates/memoryd/tests/doctor_mirror_health.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/daemon_state_files.rs`:

- `test_state_json_loads_cleanly`: write a valid `state.json` v1 schema; load via `DaemonState::load`; assert `last_completed_at` and `snooze_until` fields correct.
- `test_state_json_missing_treated_as_defaults`: call `DaemonState::load` with no file present; assert `last_completed_at = None`, `snooze_until = None`, no panic.
- `test_state_json_corrupt_treated_as_defaults`: write a `state.json` containing invalid JSON; assert load returns defaults, daemon does not panic.
- `test_state_json_version_mismatch_treated_as_defaults`: write `{ "version": 99, ... }`; assert load returns defaults, logs a warning.
- `test_state_json_write_atomic`: call `DaemonState::save`; verify no `.tmp` file remains after write and the file is fully written.
- `test_pending_json_stale_triggers_recompute`: write `reality-check-pending.json` with `computed_at` 31 minutes ago; assert `RcPendingCache::is_fresh()` returns false.
- `test_pending_json_fresh_returns_cached`: write with `computed_at` 5 minutes ago; assert `is_fresh()` returns true.
- `test_session_json_old_auto_discarded`: write `reality-check-session.json` with `started_at` 8 days ago; call `RcSessionStore::load_if_recent()`; assert returns `None` and file is renamed to `.corrupt-<timestamp>` or deleted.
- `test_session_json_corrupt_renamed`: write invalid JSON to session file; assert load returns `None` and original file is renamed to `reality-check-session.json.corrupt-<timestamp>`.
- `test_session_json_valid_loaded`: write valid v1 session; assert `RcSessionStore::load_if_recent()` returns the session.

Create `crates/memoryd/tests/doctor_mirror_health.rs`:

- `test_doctor_emits_no_finding_when_mirror_in_sync`: build a substrate with three events appended through normal dual-write; call `doctor_response`; assert `healthy = true` and no finding has `code = "events_log_mirror_lag"`.
- `test_doctor_emits_finding_when_mirror_lag_positive`: append three events normally, then directly delete the most-recent row from the SQLite `events_log` (simulating dual-write failure); call `doctor_response`; assert `healthy = false` AND a `DoctorFinding { code: "events_log_mirror_lag", repair: Some("memoryd doctor --reindex") }` is present whose `message` contains the lag count.
- `test_doctor_finding_lag_message_includes_lag_count`: assert the message text contains the string `"1 event"` for `lag = 1` and `"3 events"` for `lag = 3` (operator-readable).

Run:

```bash
cargo test -p memoryd --test daemon_state_files
cargo test -p memoryd --test doctor_mirror_health
```

Expected: FAIL because `state.rs` does not exist and `doctor_response` does not yet emit the mirror-health finding.

**Step 2: GREEN implementation**

- Create `crates/memoryd/src/state.rs` with:
  - `DaemonState` struct (schema §5.8 `state.json`), `load`, `save` methods.
  - `RcPendingCache` struct (schema §5.8 `reality-check-pending.json`), `load`, `save`, `is_fresh` (30-minute window).
  - `RcSessionStore` struct (schema §5.8 `reality-check-session.json`), `load_if_recent` (returns None if older than 7 days or corrupt; renames corrupt files), `save`, `delete`.
  - All writes use `tempfile-then-rename` atomic pattern from Stream A.
  - State file root: `<runtime_root>/state/` (runtime root from daemon config, not memory repo root).
- Wire `DaemonState::load` into `main.rs` startup (load state, log warning on fallback, do not fail startup).
- Add `.gitignore` patterns for `state/` under the runtime root if not already present.
- In `handlers.rs::doctor_response`, after collecting the existing findings, call `substrate.events_log_mirror_health()`; on `lag > 0` append a `DoctorFinding { code: "events_log_mirror_lag", message: format!("{lag} event{} not mirrored to SQLite — drift scoring may be stale; run `memoryd doctor --reindex`", if lag == 1 {""} else {"s"}), repair: Some("memoryd doctor --reindex".into()) }` and set `healthy = false`. This is the only `handlers.rs` change in this task; do not touch any other handler.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test daemon_state_files
cargo test -p memoryd --test doctor_mirror_health
cargo test -p memoryd --test server_smoke
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test daemon_state_files && cargo test -p memoryd --test doctor_mirror_health`
- Secondary: `cargo test -p memoryd --test daemon_e2e` (must stay green)

---

## Task 5: Daemon Protocol Additions — `RealityCheck*` Variants And `NotificationEvent` Channel

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 4
**Owned files:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/client.rs`, `crates/memoryd/src/mcp.rs`, `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd/tests/notification_channel.rs`
**Invariants:** `RequestPayload` and `ResponsePayload` additions are additive only — existing variants unchanged. The seven `NotificationEvent` variants match spec §1.3 exactly. **The `MethodNotAllowedOnMcp` error variant is *added by this task* on the daemon's protocol error enum — it does not exist in the shipped code.** Today's MCP forwarder uses `UnknownToolName` for unrecognized tools and has no rejection path for known-but-admin variants because admin commands are CLI-only by construction; this task introduces the variant and wires its return for `RealityCheck*` variants. (Stream I and Stream H reuse the same variant; their plans depend on this one shipping first or rebase against it.) `NotificationEvent` channel is internal to `memoryd`; it is not persisted, not MCP-exposed, and does not cross process boundaries.
**Out of scope:** Handler logic for the new request variants (Task 7). Notification dispatch routing (Task 8).

**Files:**

- Modify: `crates/memoryd/src/protocol.rs`
- Modify: `crates/memoryd/src/client.rs`
- Modify: `crates/memoryd/src/mcp.rs`
- Test: `crates/memoryd/tests/protocol_contract.rs`
- Test: `crates/memoryd/tests/notification_channel.rs`

**Step 1: RED protocol tests**

In `protocol_contract.rs`, add tests:

- `test_reality_check_request_list_round_trips_serde`: serialize and deserialize `RequestPayload::RealityCheck(RealityCheckRequest::List { namespace: None, limit: Some(12) })`; assert field equality.
- `test_reality_check_request_run_round_trips_serde`: serialize `RealityCheckRequest::Run { session_id: None, namespace: Some("me".into()) }`; assert fields.
- `test_reality_check_request_respond_round_trips_serde`: serialize `RealityCheckRequest::Respond { session_id: "s1".into(), memory_id: <id>, action: RealityCheckAction::Confirm }`; assert round-trip.
- `test_reality_check_response_pending_round_trips_serde`: serialize `RealityCheckResponse::Pending { session_id: None, items: vec![], total_scored: 5, last_completed_at: None }`; assert fields.
- `test_reality_check_item_component_scores_round_trips_serde`: serialize `RealityCheckItem` with full `ComponentScores`; assert all five component score fields present in JSON with snake_case names (per spec §5.7 wire shape contract).
- `test_existing_protocol_variants_unchanged`: assert `RequestPayload::Status`, `RequestPayload::Search`, `RequestPayload::Startup`, `RequestPayload::Write`, `RequestPayload::Supersede`, `RequestPayload::Forget` still serialize to their pre-Stream-G JSON shapes.

Create `crates/memoryd/tests/notification_channel.rs`:

- `test_notification_event_all_seven_variants_constructible`: construct all seven `NotificationEvent` variants; assert each compiles and can be sent on a `broadcast::Sender<NotificationEvent>`.
- `test_notification_event_mcp_rejected`: send `RequestPayload::RealityCheck(RealityCheckRequest::List { namespace: None, limit: None })` through the MCP forwarder mock; assert response is `MethodNotAllowedOnMcp`.

Run:

```bash
cargo test -p memoryd --test protocol_contract
cargo test -p memoryd --test notification_channel
```

Expected: FAIL because `RealityCheck*` types and `NotificationEvent` are not yet defined.

**Step 2: GREEN implementation**

- Add `RealityCheckRequest`, `RealityCheckResponse`, `RealityCheckAction`, `RealityCheckItem`, `ComponentScores`, `RealityCheckCompletion`, `RespondRefusalKind` to `protocol.rs` matching spec §5.7 exactly.
- Add `RequestPayload::RealityCheck(RealityCheckRequest)` and `ResponsePayload::RealityCheck(RealityCheckResponse)` variants.
- Add `NotificationEvent` enum with exactly the seven variants from spec §1.3: `LeakedSecretDetected`, `BlockingMergeConflict`, `ReviewQueueOverThreshold`, `DreamRunCompleted`, `RealityCheckDue`, `RealityCheckOverdue`, `DailySynthesisSummaryReady`.
- Create a `tokio::sync::broadcast::channel::<NotificationEvent>(256)` and store the sender in `memoryd`'s shared state. The channel capacity 256 is sufficient for burst; the dispatcher logs `Lagged(n)` on overflow (per spec §6.3).
- In `protocol.rs`, add `MethodNotAllowedOnMcp` to the protocol error enum (alongside the shipped `UnknownToolName`, etc.). Cite system-v0.2 §19's authorization in the doc comment.
- In `mcp.rs`, add `RequestPayload::RealityCheck(_)` to the MCP-rejected match arm returning `MethodNotAllowedOnMcp`. Document in the doc comment that Stream I (peer-state variants) and Stream H (`TestInjectEvent` test-utils variant) will land additional rejected match arms in their plans, all returning the same error variant.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test protocol_contract
cargo test -p memoryd --test notification_channel
cargo test -p memoryd --test mcp_manifest
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test protocol_contract && cargo test -p memoryd --test notification_channel`
- Secondary: `cargo test -p memoryd --test server_smoke --test daemon_e2e`

---

## Task 6: Reality Check Scoring Library — Drift-Risk Formula

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 5
**Owned files:** `crates/memoryd/src/reality_check/mod.rs`, `crates/memoryd/src/reality_check/scoring.rs`, `crates/memoryd/src/reality_check/types.rs`, `crates/memoryd/tests/scoring.rs`
**Invariants:** `recall_count_30d` is derived at score time via SQL against the `events_log` SQLite mirror table (added in Task 2 with covering index). `distinct_sources` is derived from `memories.source_harness` joined recursively through the `memory_supersession(memory_id, supersedes_id)` join table (added in Task 2; `memories.supersedes_ids` does not exist as a column — supersession is a derived projection sourced from `Frontmatter.supersedes`). The recursive walk uses an explicitly bounded CTE shape: `WITH RECURSIVE chain(memory_id, depth) AS (SELECT ?, 0 UNION ALL SELECT ms.supersedes_id, c.depth + 1 FROM memory_supersession ms JOIN chain c ON ms.memory_id = c.memory_id WHERE c.depth < 8) SELECT COUNT(DISTINCT m.source_harness) FROM chain JOIN memories m ON chain.memory_id = m.id` — the `WHERE c.depth < 8` predicate is the depth bound and is also the cycle guard (see §10.3 spec test for cycle case). **NULL `source_harness` is excluded from the distinct count by SQL convention; this is intentional — a memory written with no harness attribution (e.g., a `memory_note` whose frontmatter omitted `source.harness`) is treated as "unknown harness," not as a corroborating source.** Scoring inputs `observed_at`, `confidence`, `original_confidence`, `sensitivity` come from `memories` index columns only; no `Substrate::read_memory` call per item. `confidence_decay(m)` reads `original_confidence` from the v0.2 Frontmatter field added in Task 2 (`Option<f64>`); pre-v0.2 memories with `None` score 0.0 on this component. The five weight components must sum to 1.0; if `config.yaml` weights are invalid, the scoring function uses the locked defaults from spec §5.1. `secret` memories are never in the scoring pool (filtered out before score computation). See spec §5.1 for exact normalization functions and authoritative data sources.
**Out of scope:** Session lifecycle, scheduling, daemon handler dispatch (Tasks 7, 9).

**Files:**

- Create: `crates/memoryd/src/reality_check/mod.rs`
- Create: `crates/memoryd/src/reality_check/scoring.rs`
- Create: `crates/memoryd/src/reality_check/types.rs`
- Test: `crates/memoryd/tests/scoring.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/scoring.rs` matching spec §10.3:

- `test_score_formula_staleness_only`: memory with 90-day staleness, perfect recall frequency (recall_count = max), single source, no decay, `public` sensitivity; assert score ≈ 0.35 (weight only from staleness term).
- `test_score_formula_all_components`: known input for each component; assert final score matches formula to 4 decimal places.
- `test_score_saturation_at_90_days`: memory with 120-day staleness; assert `days_since_observed_norm` = 1.0 (saturates at 90).
- `test_score_below_90_days_proportional`: memory with 45-day staleness; assert `days_since_observed_norm` ≈ 0.5.
- `test_corroboration_requires_two_distinct_harnesses`: memory written by `claude-code` with one supersession also from `claude-code`; assert `cross_source_corroboration` = 0.0.
- `test_corroboration_satisfied_by_two_harnesses`: memory written by `claude-code` superseded by `codex`; assert `cross_source_corroboration` = 1.0 (data from `memories.source_harness`, NOT events log).
- `test_corroboration_walks_supersession_chain_depth_bounded`: memory with 10-deep supersession chain alternating harnesses (`claude-code`, `codex`, `claude-code`, `codex`, ...); assert recursion stops at depth 8 (spec §5.1) without panicking AND `cross_source_corroboration` = 1.0 (two distinct harnesses observed within the bounded chain).
- `test_corroboration_recursive_cte_handles_cycle_via_depth_bound`: insert a deliberate cycle into `memory_supersession` (`a → b`, `b → a`); assert the bounded CTE terminates via the depth bound without infinite-looping and the function returns a finite distinct count.
- `test_corroboration_null_source_harness_does_not_count_as_distinct`: memory `a` written via `memory_note` with `source.harness = None` (`memories.source_harness IS NULL`), superseded by memory `b` written by `claude-code`; assert `cross_source_corroboration` = 0.0 because `COUNT(DISTINCT source_harness)` excludes NULL by SQL semantics — only one non-NULL harness contributes. This is intentional per the §5.1 invariant: NULL means "unknown harness," not "a distinct source."
- `test_corroboration_two_non_null_harnesses_with_one_null_in_chain_yields_corroboration`: chain `a (NULL) → b (claude-code) → c (codex)`; assert `cross_source_corroboration` = 1.0 — the NULL is silently excluded, the two non-NULL harnesses corroborate.
- `test_sensitivity_weights_map_correctly`: four memories with `public`, `internal`, `confidential`, `personal`; assert weights 0.0, 0.3, 0.6, 1.0.
- `test_confidence_decay_clamped_to_zero`: memory with current confidence higher than original; assert `confidence_decay` = 0.0.
- `test_confidence_decay_none_baseline_yields_zero`: pre-v0.2 memory with `original_confidence = None`; assert `confidence_decay` = 0.0 (conservative floor — no baseline, no measurable drift).
- `test_encrypted_memory_scored_from_index_only`: memory with `encrypted: true`; assert scoring completes using only index fields, no body access.
- `test_top_n_selection_respects_cap`: 20 scored memories with varied scores; assert only 12 returned with default `top_n = 12`.
- `test_pinned_memories_always_included`: 12 high-score non-pinned memories + 1 pinned low-score memory; assert pinned memory is present in the top-12 list.
- `test_excluded_statuses_not_scored`: memories with status `candidate`, `quarantined`, `tombstoned`, `archived`, `superseded`; assert none appear in scoring pool.
- `test_passive_recall_false_excluded`: memory with `retrieval_policy.passive_recall = false`; assert excluded from scoring pool.
- `test_score_bounded_zero_to_one`: extreme inputs for all components simultaneously; assert final score is within `[0.0, 1.0]`.
- `test_invalid_weight_config_falls_back_to_defaults`: pass config with weights that sum to 1.1; assert scorer uses locked defaults (0.35, 0.20, 0.20, 0.15, 0.10).

Run:

```bash
cargo test -p memoryd --test scoring
```

Expected: FAIL because `reality_check::scoring` does not exist.

**Step 2: GREEN implementation**

- Create `crates/memoryd/src/reality_check/types.rs` with `ScoringConfig`, `ScoredMemory`, and helper enums.
- Create `crates/memoryd/src/reality_check/scoring.rs` with:
  - `score_memories(pool: &[RecallIndexRow], substrate: &Substrate, config: &ScoringConfig) -> Vec<ScoredMemory>` — single pass over index rows, sort, take top_n.
  - Individual component functions: `days_since_observed_norm`, `recall_frequency_norm`, `cross_source_corroboration`, `confidence_decay`, `sensitivity_weight`.
  - **Two aggregate queries** prepared once per scoring run:
    - `recall_count_30d`: `SELECT memory_id, COUNT(*) FROM events_log WHERE kind = 'recall_hit' AND ts > ? GROUP BY memory_id` — one full scan over the events_log mirror, indexed on `(kind, memory_id, ts)`. Result is a `HashMap<MemoryId, u32>`; lookup per pool member is O(1).
    - `distinct_sources`: an explicitly bounded recursive CTE walking the `memory_supersession(memory_id, supersedes_id)` join table created in Task 2: `WITH RECURSIVE chain(memory_id, depth) AS (SELECT ?, 0 UNION ALL SELECT ms.supersedes_id, c.depth + 1 FROM memory_supersession ms JOIN chain c ON ms.memory_id = c.memory_id WHERE c.depth < 8) SELECT COUNT(DISTINCT m.source_harness) FROM chain JOIN memories m ON chain.memory_id = m.id`. The `WHERE c.depth < 8` predicate is BOTH the depth bound AND the cycle guard. NULL `source_harness` is excluded from the distinct count by SQL convention; this is intentional (see invariant). One full scan over the pool, no events_log involvement.
  - `confidence_decay`: reads `frontmatter.original_confidence` from the index row's hydrated frontmatter (already populated by Task 2's index hydration). `None` baseline → 0.0.
- `mod.rs` re-exports public API.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test scoring
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test scoring`
- Secondary: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

## Task 7: Reality Check Session Lifecycle Handlers

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 6
**Owned files:** `crates/memoryd/src/reality_check/session.rs`, `crates/memoryd/src/reality_check/scheduling.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/scheduling.rs`, `crates/memoryd/tests/responses.rs`
**Invariants:** `confirm` is metadata-only (no governance gate). `correct` issues full `memory_supersede` through the governance pipeline; if governance refuses, the session item is not advanced. `forget` requires a reason of at least 3 characters. `not_relevant` sets `passive_recall = false` and adds `reality_check_not_relevant` tag; does NOT tombstone. `skip_this_week` defers without frontmatter mutation. Session state is persisted via `RcSessionStore` from Task 4 (tempfile-then-rename). Scheduling checks run on daemon startup and once per hour; invalid cron expression falls back to `0 9 * * SUN` with a warning log. The `RealityCheckDue` notification event is fired on the `NotificationEvent` broadcast channel when due. MCP forwarder rejects all `RealityCheck*` requests.
**Out of scope:** TUI rendering (Task 10). Notification dispatch channels (Task 8). CLI surface (Task 16).

**Files:**

- Create: `crates/memoryd/src/reality_check/session.rs`
- Create: `crates/memoryd/src/reality_check/scheduling.rs`
- Modify: `crates/memoryd/src/handlers.rs`
- Test: `crates/memoryd/tests/scheduling.rs`
- Test: `crates/memoryd/tests/responses.rs`

**Step 1: RED scheduling tests**

Create `crates/memoryd/tests/scheduling.rs` matching spec §10.3:

- `test_due_after_7_days`: mock `last_completed_at` 8 days ago; assert `is_due()` returns true.
- `test_not_due_within_7_days`: mock 5 days ago; assert `is_due()` returns false.
- `test_snoozed_not_due`: mock due but snooze active until tomorrow; assert `is_due()` returns false.
- `test_overdue_after_21_days`: mock 22 days ago; assert `is_overdue()` returns true.
- `test_invalid_cron_falls_back_to_default`: pass an invalid cron string to scheduler; assert no panic and default `0 9 * * SUN` schedule is used.
- `test_notification_event_fired_when_due`: trigger `check_and_fire_if_due`; assert `NotificationEvent::RealityCheckDue` is sent on the broadcast channel.
- `test_notification_event_not_fired_when_not_due`: trigger check with recent `last_completed_at`; assert no `RealityCheckDue` event.

Run:

```bash
cargo test -p memoryd --test scheduling
```

Expected: FAIL.

**Step 2: RED response action tests**

Create `crates/memoryd/tests/responses.rs` matching spec §10.3:

- `test_confirm_updates_observed_at_and_bumps_confidence`: mock confirm action; assert `observed_at = now`, `confidence` bumped by 0.02 (capped at 1.0), `EventKind::RealityCheckConfirmed` appended to event log.
- `test_not_relevant_sets_passive_recall_false`: mock not-relevant action; assert `retrieval_policy.passive_recall = false`, tag `reality_check_not_relevant` added.
- `test_not_relevant_does_not_tombstone`: assert memory status remains `active`, no tombstone event emitted.
- `test_forget_requires_reason_minimum_length`: reason < 3 chars; assert `RespondRefused { kind: InvalidAction }` returned, no tombstone.
- `test_forget_with_valid_reason_tombstones`: reason >= 3 chars; assert `memory_forget` issued, session item marked reviewed.
- `test_correct_issues_supersession`: mock correct with new body; assert `memory_supersede` called with `source_kind: "user"`, `explicit_user_context: true`.
- `test_correct_governance_refusal_does_not_advance_session`: mock governance refusing the supersede; assert `RespondRefused { kind: GovernanceRefused }` returned, session `current_index` unchanged.
- `test_skip_this_week_defers_without_frontmatter_mutation`: skip action; assert `deferred_this_week` updated in session state, no frontmatter write or governance call.
- `test_session_complete_updates_state_json`: all items reviewed; assert `DaemonState.reality_check.last_completed_at` updated, session file deleted.
- `test_abandoned_session_offered_for_resumption`: `RealityCheckRequest::Run` when session file exists; assert `Pending { session_id: Some("existing_id"), ... }` returned with existing items.

Run:

```bash
cargo test -p memoryd --test responses
```

Expected: FAIL.

**Step 3: GREEN implementation**

- Create `scheduling.rs` with `RcScheduler::is_due`, `is_overdue`, `check_and_fire_if_due` using the `cron` crate for schedule parsing.
- Create `session.rs` with `RcSessionHandler` dispatching `RealityCheckRequest` variants to scoring, state-file reads, governance calls, and session persistence.
- Wire `RequestPayload::RealityCheck` into `handlers.rs` dispatch.

**Step 4: GREEN commands**

```bash
cargo test -p memoryd --test scheduling
cargo test -p memoryd --test responses
cargo test -p memoryd --test protocol_contract
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test scheduling && cargo test -p memoryd --test responses`
- Secondary: `cargo test -p memoryd --test governance_e2e`

---

## Review Gate B: Daemon Protocol, State Files, Scoring, And Session Handlers

**Subagent types:** `reviewer`, `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 4–7 integrated and green
**Owned files:** `docs/reviews/stream-g-review-gate-b-clean-code.md`, `docs/reviews/stream-g-review-gate-b-security.md`
**Invariants:** Review only.
**Out of scope:** TUI, web, notification channels.

**Review focus:**

- State file crash recovery follows spec §5.8 exactly: missing/corrupt files never prevent daemon startup; atomic writes via tempfile-then-rename.
- `RealityCheck*` variants are fully blocked from MCP forwarder.
- Scoring never calls `Substrate::read_memory` per-item; all inputs from index/events.
- `confirm` is truly metadata-only — no governance pipeline call.
- `not_relevant` does not tombstone.
- `skip_this_week` does not mutate frontmatter.
- `forget` reason length validation is pre-governance.
- No memory content in notification event payloads.
- Handler functions are small, named, and testable.

**Commands:**

```bash
cargo test -p memoryd --test daemon_state_files --test scoring --test scheduling --test responses --test notification_channel --test protocol_contract
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 8.

---

## Task 8: Notification Dispatcher — Passive, OS, And External Channels

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Review Gate B
**Owned files:** `crates/memoryd/src/notifications/mod.rs`, `crates/memoryd/src/notifications/dispatcher.rs`, `crates/memoryd/src/notifications/passive.rs`, `crates/memoryd/src/notifications/os.rs`, `crates/memoryd/src/notifications/external.rs`, `crates/memoryd/src/notifications/config.rs`, `crates/memoryd/Cargo.toml`, `crates/memoryd/tests/dispatcher.rs`
**Invariants:** Slack/email payloads contain NO memory content — no titles, no bodies, no entity names (spec §6.4). Passive queue is capped at 100 entries (FIFO drop when full). OS notifications are best-effort; failure is logged at DEBUG, not propagated. External retry: 3 attempts with backoff 30s/120s/600s; after final failure append to passive queue (spec §6.5). Dispatcher tolerates `Lagged(n)` events from the broadcast channel without crashing (spec §6.3). SMTP password read from env var by name (`smtp_password_env`), never from config file value.
**Out of scope:** TUI integration of notifications (Task 12). Stream E pending-attention hook (Task 9). CLI notification config commands.

**Files:**

- Create: `crates/memoryd/src/notifications/mod.rs`
- Create: `crates/memoryd/src/notifications/dispatcher.rs`
- Create: `crates/memoryd/src/notifications/passive.rs`
- Create: `crates/memoryd/src/notifications/os.rs`
- Create: `crates/memoryd/src/notifications/external.rs`
- Create: `crates/memoryd/src/notifications/config.rs`
- Modify: `crates/memoryd/Cargo.toml`
- Test: `crates/memoryd/tests/dispatcher.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/dispatcher.rs` matching spec §10.4:

- `test_passive_queue_receives_all_events`: fire each of the seven `NotificationEvent` variants; assert passive queue has one entry per event.
- `test_passive_queue_drops_oldest_when_full`: fill queue to 100; fire one more; assert first item dropped.
- `test_os_notification_not_fired_when_disabled`: send `LeakedSecretDetected` with `os.enabled: false`; assert no `osascript` / `notify-send` call.
- `test_os_notification_fires_when_enabled_and_trigger_matches`: `os.enabled: true`, matching trigger; assert OS command called.
- `test_slack_webhook_retried_on_failure`: mock Slack endpoint returning 500; assert retry up to `retry_max = 3` times.
- `test_slack_webhook_falls_back_to_passive_on_final_failure`: exhausted retries; assert passive queue contains failure note.
- `test_slack_payload_contains_no_memory_content`: fire `RealityCheckDue { due_at: <ts> }`; capture Slack payload; assert no memory titles, bodies, or entity names.
- `test_lagged_dispatcher_logs_warning_and_continues`: overfill the broadcast channel beyond its capacity; assert WARN log emitted, dispatcher loop continues.
- `test_smtp_password_read_from_env_var`: configure `smtp_password_env: "TEST_SMTP_PW"`; set env var; assert SMTP client uses its value, not a hardcoded string in config.
- `test_smtp_password_missing_env_var_logs_error_and_disables`: env var not set; assert `ERROR` log and email delivery disabled (no panic).

Run:

```bash
cargo test -p memoryd --test dispatcher
```

Expected: FAIL because `notifications` module does not exist.

**Step 2: GREEN implementation**

- Add `reqwest` and `lettre` to `crates/memoryd/Cargo.toml`.
- Implement `notifications::passive::PassiveQueue` (100-entry ring buffer, `Vec<String>` items, drain via `memoryd status`).
- Implement `notifications::os::OsNotifier` (detect `osascript`/`notify-send` at startup; best-effort fire; log DEBUG on failure).
- Implement `notifications::external::ExternalNotifier` (Slack webhook POST with spec §6.4 payload shape; SMTP via `lettre`; exponential backoff retry; passive fallback on exhaustion).
- Implement `notifications::dispatcher::NotificationDispatcher` Tokio task (subscribes to broadcast channel; routes per spec §6.2 trigger definitions; handles `Lagged(n)` with WARN log).
- Spawn dispatcher task in `memoryd` startup alongside the server.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test dispatcher
cargo test -p memoryd --test server_smoke
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test dispatcher`
- Secondary: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

## Task 9: Stream E `<pending-attention>` Reality Check Integration

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 8
**Owned files:** `crates/memoryd/src/recall/startup.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/tests/reality_check_pending_attention.rs`
**Invariants:** `<pending-attention>` output XML is additive — existing items unchanged. The `reality_check_due` item emits at most once per 7-day window. Suppressed if snoozed via `RcScheduler::is_snoozed`. The item text is a fixed string with no memory titles or body content. The item counts against the 6-total cap from Stream E v0.5 spec; if 6 slots are filled by higher-priority items, it is dropped silently (increments `omitted_count`). Startup recall XML version string `stream-e-v0.5` is unchanged.
**Out of scope:** Notification channel dispatch (Task 8 owns that). Session lifecycle (Task 7 owns that).

**Files:**

- Modify: `crates/memoryd/src/recall/startup.rs`
- Modify: `crates/memoryd/src/recall/render.rs`
- Test: `crates/memoryd/tests/reality_check_pending_attention.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/reality_check_pending_attention.rs`:

- `test_reality_check_due_item_appears_when_due`: mock `RcScheduler::is_due() = true`, no snooze; assert `<item kind="reality_check_due" count="1">` appears in `<pending-attention>` block.
- `test_reality_check_due_suppressed_when_not_due`: `is_due() = false`; assert no `reality_check_due` item.
- `test_reality_check_due_suppressed_when_snoozed`: `is_due() = true` but snoozed; assert item absent.
- `test_reality_check_item_text_contains_no_memory_content`: assert item text is the fixed string from spec §1.3 and contains no dynamic memory fields.
- `test_reality_check_item_dropped_when_6_total_cap_full`: mock 6 existing high-priority pending-attention items; assert `reality_check_due` is dropped into `omitted_count`.
- `test_reality_check_item_counts_against_cap`: mock 5 existing items; assert `reality_check_due` uses the 6th slot.
- `test_startup_xml_version_string_unchanged`: assert `<memory-recall version="stream-e-v0.5">` header is preserved.

Run:

```bash
cargo test -p memoryd --test reality_check_pending_attention
```

Expected: FAIL.

**Step 2: GREEN implementation**

- In the pending-attention assembly path in `startup.rs` / `render.rs`, add a check: if `RcScheduler::is_due()` and not snoozed and within the 7-day deduplication window, append the `reality_check_due` item before applying the 6-total cap.
- Use the fixed item text from spec §1.3 verbatim.
- Thread `RcScheduler` reference into the recall assembly path; the scheduler is already initialized in daemon startup.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test reality_check_pending_attention
cargo test -p memoryd --test startup_recall_mcp
cargo test -p memoryd --test recall_hit_emission
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test reality_check_pending_attention`
- Secondary: `cargo test -p memoryd --test startup_recall_determinism --test startup_recall_privacy`

---

## Task 10: `crates/memoryd-tui/` Skeleton, Ratatui Panel Framework, And 8-Panel Layout

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 9
**Owned files:** `crates/memoryd-tui/Cargo.toml`, `crates/memoryd-tui/src/main.rs`, `crates/memoryd-tui/src/app.rs`, `crates/memoryd-tui/src/panels/mod.rs`, `crates/memoryd-tui/src/panels/overview.rs`, `crates/memoryd-tui/src/panels/review_queue.rs`, `crates/memoryd-tui/src/panels/conflicts.rs`, `crates/memoryd-tui/src/panels/entities.rs`, `crates/memoryd-tui/src/panels/timeline.rs`, `crates/memoryd-tui/src/panels/namespace.rs`, `crates/memoryd-tui/src/panels/policy.rs`, `crates/memoryd-tui/src/panels/reality_check.rs`, `crates/memoryd-tui/src/client.rs`, `crates/memoryd-tui/src/config.rs`, `Cargo.toml`
**Invariants:** Both new crates are added to the workspace `Cargo.toml`. Neither crate has direct `memory-substrate` dependency — all reads go through the daemon socket via `memoryd::client`. `ratatui` and `crossterm` are dependencies of `memoryd-tui` only. Tick rate 16 ms, daemon poll rate 250 ms (configurable per spec §8). Below 80×24 shows warning banner. Socket unreachable shows the error box from spec §3.7 (not stale data).
**Out of scope:** Trust artifact widget (Task 12). TUI keymap exhaustiveness (Task 11). Reality Check interactive session in TUI (Task 11).

**Files:**

- Create: `crates/memoryd-tui/Cargo.toml`
- Create: `crates/memoryd-tui/src/main.rs`
- Create: `crates/memoryd-tui/src/app.rs`
- Create: `crates/memoryd-tui/src/panels/mod.rs` + 8 panel files
- Create: `crates/memoryd-tui/src/client.rs`
- Create: `crates/memoryd-tui/src/config.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: RED tests**

Create `crates/memoryd-tui/tests/panel_render.rs` with the snapshot tests from spec §10.1:

- `test_overview_panel_renders_daemon_status`
- `test_review_queue_renders_candidate_items`
- `test_review_queue_renders_dream_low_confidence`
- `test_conflicts_panel_renders_side_by_side`
- `test_entities_panel_search_renders_results`
- `test_timeline_panel_renders_events_by_kind`
- `test_namespace_tree_renders_hierarchy`
- `test_policy_panel_renders_active_policies`
- `test_reality_check_panel_renders_score_breakdown`

Each test: construct an `App` with a mocked daemon response, call `render()` on a fixed-size `TestBackend` (80×24), assert the rendered frame buffer contains expected text fragments.

Create `crates/memoryd-tui/tests/socket_unreachable.rs`:

- `test_tui_shows_unreachable_state_on_socket_failure`
- `test_tui_recovers_on_reconnection`

Create `crates/memoryd-tui/tests/resize.rs`:

- `test_below_minimum_shows_warning_banner`
- `test_resize_above_minimum_resumes`

Run:

```bash
cargo test -p memoryd-tui --test panel_render
cargo test -p memoryd-tui --test socket_unreachable
cargo test -p memoryd-tui --test resize
```

Expected: FAIL because the crate does not exist.

**Step 2: GREEN implementation**

- Add `memoryd-tui` to workspace `Cargo.toml`.
- Create `Cargo.toml` with `ratatui`, `crossterm`, `tokio`, `serde_json` dependencies and `memoryd` as a library dependency.
- Implement `App` struct with `PanelId` enum (1–8), panel state sub-structs, event loop (16 ms tick + 250 ms daemon poll), resize handling, socket-unreachable state.
- Implement 8 panel render functions; all can render placeholder content sufficient to pass snapshot tests.
- Implement `client.rs` as a thin wrapper over the Unix socket `memoryd::client::Client`.
- Implement `config.rs` reading `[ui]` section from config.yaml.

**Step 3: GREEN command**

```bash
cargo test -p memoryd-tui --test panel_render
cargo test -p memoryd-tui --test socket_unreachable
cargo test -p memoryd-tui --test resize
cargo build -p memoryd-tui
```

**Verification plan:**

- Primary: `cargo test -p memoryd-tui --test panel_render && cargo test -p memoryd-tui --test socket_unreachable && cargo test -p memoryd-tui --test resize`
- Secondary: `cargo build -p memoryd-tui`

---

## Task 11: TUI Keymap, Interactive Panel Behaviors, And Terminal Compatibility

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 10
**Owned files:** `crates/memoryd-tui/src/app.rs`, `crates/memoryd-tui/src/panels/review_queue.rs`, `crates/memoryd-tui/src/panels/conflicts.rs`, `crates/memoryd-tui/src/panels/entities.rs`, `crates/memoryd-tui/src/panels/timeline.rs`, `crates/memoryd-tui/src/panels/namespace.rs`, `crates/memoryd-tui/src/panels/policy.rs`, `crates/memoryd-tui/src/panels/reality_check.rs`, `crates/memoryd-tui/tests/keymap.rs`
**Invariants:** All global keys from spec §3.3 function in all 8 panels. Panel-local keys from spec §3.2 do not collide with global keys. At most one modal open at a time. `Esc` closes the top modal. The 1-second undo window fires before the daemon call (spec §3.2 Panel 2). Modals are closed on resize (spec §3.6). Reality Check active-run panel (spec §3.2 Panel 8) handles `c`, `k`, `f`, `n`, `space` actions.
**Out of scope:** Trust artifact modal (Task 12 owns the widget). The actual daemon calls for review actions are stubbed; full integration covered by Task 16 CLI wiring.

**Files:**

- Modify: `crates/memoryd-tui/src/app.rs`
- Modify: 7 panel files
- Test: `crates/memoryd-tui/tests/keymap.rs`

**Step 1: RED tests**

Create `crates/memoryd-tui/tests/keymap.rs` matching spec §10.1:

- `test_all_panels_handle_panel_switch_keys`: in each of 8 panels, send key events `1`–`8`; assert `App.active_panel` transitions to correct `PanelId`.
- `test_quit_with_pending_actions_prompts_confirmation`: stage a review action, send `q`; assert confirmation modal opens (modal state set).
- `test_escape_closes_modal`: open memory detail modal, send `Esc`; assert modal is `None` and underlying panel state unchanged.
- `test_undo_window_fires_before_daemon_call`: stage review `approve`, check 1-second window; assert pressing `u` within window sets `pending_action = None` and no daemon call is queued.
- `test_undo_window_expires_and_fires_daemon_call`: same setup but advance mock clock past 1000 ms; assert daemon call queued.
- `test_resize_closes_active_modal`: open a modal, send resize event; assert modal is closed.
- `test_help_overlay_opens_on_question_mark`: send `?`; assert help overlay modal visible.
- `test_ctrl_c_quits_immediately`: send `Ctrl-c`; assert quit signal without confirmation prompt.

Run:

```bash
cargo test -p memoryd-tui --test keymap
```

Expected: FAIL.

**Step 2: GREEN implementation**

- Add key dispatch in `app.rs`: global keys first, then active-panel keys.
- Implement modal management: `App.modal: Option<Modal>` where `Modal` is an enum of modal types (MemoryDetail, HelpOverlay, ConfirmQuit, ConfirmForget, etc.).
- Implement 1-second undo window: `App.pending_action: Option<(Instant, Action)>`; check on each tick whether window expired; if expired, fire daemon call.
- Wire panel-local navigation (j/k, h/l, Enter, /) for each panel.
- Wire Reality Check Panel 8 action keys (`c`, `k`, `f`, `n`, `space`).

**Step 3: GREEN command**

```bash
cargo test -p memoryd-tui --test keymap
cargo test -p memoryd-tui --test panel_render
```

**Verification plan:**

- Primary: `cargo test -p memoryd-tui --test keymap`
- Secondary: `cargo test -p memoryd-tui --test panel_render` (must stay green after key dispatch is added)

---

## Task 12: Trust Artifact Widget (Shared Between TUI And Web)

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 11
**Owned files:** `crates/memoryd-tui/src/widgets/mod.rs`, `crates/memoryd-tui/src/widgets/trust_artifact.rs`, `crates/memoryd-tui/src/widgets/memory_detail.rs`, `crates/memoryd-tui/src/widgets/diff_view.rs`, `crates/memoryd-tui/src/widgets/search_bar.rs`, `crates/memoryd/src/trust_artifact.rs`, `crates/memoryd/tests/trust_artifact.rs`
**Invariants:** All 8 trust artifact sections from spec §7.2 are present for plaintext memories. Encrypted memories show body redaction notice but all other sections render (spec §7.2). Provenance chain is rendered chronologically ascending. Policy decisions expand all 5 governance fields. Data sources per field match spec §7.1 — `recall_count_30d` and `last_recalled` are derived from the events log via the covering index, not from a `memories` column. Trust artifact data is fetched server-side (daemon handles assembly); the widget receives a pre-assembled `TrustArtifact` DTO.
**Out of scope:** Web dashboard rendering (Task 14 uses the same DTO from the API route). Sync state from Stream I claim-lock (deferred; renders "Stream I not active" placeholder if absent).

**Files:**

- Create: `crates/memoryd-tui/src/widgets/mod.rs`
- Create: `crates/memoryd-tui/src/widgets/trust_artifact.rs`
- Create: `crates/memoryd-tui/src/widgets/memory_detail.rs`
- Create: `crates/memoryd-tui/src/widgets/diff_view.rs`
- Create: `crates/memoryd-tui/src/widgets/search_bar.rs`
- Create: `crates/memoryd/src/trust_artifact.rs`
- Test: `crates/memoryd/tests/trust_artifact.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/trust_artifact.rs` matching spec §10.5:

- `test_all_sections_present_for_plaintext_memory`: build `TrustArtifact` for a plaintext memory with all fields populated; assert all 8 sections present (title/body, confidence, recall, provenance, policy decisions, privacy scan, supersession, sync state).
- `test_encrypted_memory_shows_content_redacted`: build trust artifact for an encrypted memory; assert body section contains redaction notice, all other sections present.
- `test_provenance_chain_correctly_ordered`: mock events inserted out of order; assert `provenance_chain` is sorted chronologically ascending by timestamp.
- `test_policy_decision_expands_all_fields`: assert all 5 governance decision fields rendered (`conf_floor_pass`, `grounding_satisfied`, `contradiction_result`, `tombstone_enforced`, `sensitivity_gate_result`).
- `test_recall_count_30d_derived_from_events_log`: set up mock substrate with 5 `RecallHit` events in last 30 days; assert `recall_count_30d = 5`.
- `test_last_recalled_derived_from_events_log`: set up `RecallHit` events; assert `last_recalled_at` = max timestamp.

Run:

```bash
cargo test -p memoryd --test trust_artifact
```

Expected: FAIL because `trust_artifact.rs` does not exist.

**Step 2: GREEN implementation**

- Create `crates/memoryd/src/trust_artifact.rs` with `TrustArtifact` DTO struct (matching the `GET /api/audit/:id` JSON shape from spec §4.3) and `TrustArtifactBuilder` that assembles from: substrate read, events log scans, governance history, privacy frontmatter.
- Create TUI widget files under `crates/memoryd-tui/src/widgets/`: `TrustArtifactWidget` renders the spec §7.2 TUI modal format; `MemoryDetailModal` wraps it with scrolling and action keys; `DiffViewWidget` for Panel 3 side-by-side conflict view; `SearchBarWidget` for typeahead.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test trust_artifact
cargo test -p memoryd-tui --test panel_render
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test trust_artifact`
- Secondary: `cargo test -p memoryd-tui --test panel_render` (memory detail modal panels must still render)

---

## Review Gate C: Notification Dispatcher, TUI Framework, And Trust Artifact

**Subagent types:** `reviewer`, `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 8–12 integrated and green
**Owned files:** `docs/reviews/stream-g-review-gate-c-clean-code.md`, `docs/reviews/stream-g-review-gate-c-security.md`
**Invariants:** Review only.
**Out of scope:** Web dashboard, CLI.

**Review focus:**

- Slack/email payloads contain zero memory content (titles, bodies, entity names).
- Passive queue is truly always-on and cannot be silenced.
- SMTP password never appears in config file value; reads from env var by name.
- TUI never shows stale data as live data when socket is unreachable (error box, not cached values).
- Trust artifact sections match spec §7.1 data sources exactly; no fabricated fields.
- `recall_count_30d` and `last_recalled` derived from events log, not a `memories` column.
- Encrypted memory trust artifacts show redaction notice, not empty or error.
- Module boundaries are clean: `memoryd-tui` has no direct substrate access.

**Commands:**

```bash
cargo test -p memoryd --test dispatcher --test trust_artifact
cargo test -p memoryd-tui --test panel_render --test keymap --test socket_unreachable --test resize
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 13.

---

## Task 13: `crates/memoryd-web/` Skeleton — Axum Router, CSRF, Auth, And Static Assets

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Review Gate C
**Owned files:** `crates/memoryd-web/Cargo.toml`, `crates/memoryd-web/src/main.rs`, `crates/memoryd-web/src/server.rs`, `crates/memoryd-web/src/auth.rs`, `crates/memoryd-web/src/config.rs`, `crates/memoryd-web/src/routes/mod.rs`, `crates/memoryd-web/static/index.html`, `crates/memoryd-web/static/app.js`, `crates/memoryd-web/static/style.css`, `crates/memoryd-web/tests/csrf.rs`, `Cargo.toml`
**Invariants:** Server binds to `127.0.0.1` only; `0.0.0.0` rejected at config load with ERROR (spec §8 `web.bind_address`). CSRF token is 32 random bytes generated at server start, served in `<meta name="csrf-token">` in initial HTML, required in `X-Memorum-CSRF` header for all POST routes. CSRF token rotates on server restart. Static assets embedded in binary via `rust-embed` (no runtime disk reads). All assets self-hosted; zero CDN or external network requests. The web server runs as a Tokio task inside `memoryd` when enabled, not a separate process.
**Out of scope:** The 4 API route sections content (Task 14). SSE notification stream (Task 14).

**Files:**

- Create: `crates/memoryd-web/Cargo.toml`
- Create: `crates/memoryd-web/src/main.rs`
- Create: `crates/memoryd-web/src/server.rs`
- Create: `crates/memoryd-web/src/auth.rs`
- Create: `crates/memoryd-web/src/config.rs`
- Create: `crates/memoryd-web/src/routes/mod.rs`
- Create: `crates/memoryd-web/static/index.html` (SPA shell)
- Create: `crates/memoryd-web/static/app.js` (bundled Preact+HTM placeholder)
- Create: `crates/memoryd-web/static/style.css`
- Modify: `Cargo.toml` (workspace members)
- Test: `crates/memoryd-web/tests/csrf.rs`

**Step 1: RED tests**

Create `crates/memoryd-web/tests/csrf.rs` matching spec §10.2:

- `test_post_without_csrf_header_returns_403`: POST to `/api/review/action` without `X-Memorum-CSRF`; assert 403.
- `test_post_with_wrong_csrf_token_returns_403`: POST with wrong token value; assert 403.
- `test_post_with_correct_csrf_token_succeeds`: fetch `/` first to get token, then POST with correct token; assert non-403 response.
- `test_csrf_token_in_initial_html`: GET `/`; assert `<meta name="csrf-token" content="...">` is present in response body.
- `test_bind_address_0_0_0_0_rejected_at_config`: configure `bind_address: "0.0.0.0"`; assert server refuses to start, logs ERROR.

Create `crates/memoryd-web/tests/concurrent_access.rs`:

- `test_concurrent_post_same_memory_second_returns_409`: simulate two simultaneous POST requests for the same memory id; assert first succeeds, second gets 409.

Run:

```bash
cargo test -p memoryd-web --test csrf
cargo test -p memoryd-web --test concurrent_access
```

Expected: FAIL because the crate does not exist.

**Step 2: GREEN implementation**

- Add `memoryd-web` to workspace `Cargo.toml`.
- Create `Cargo.toml` with `axum`, `tokio`, `rust-embed`, `rand` (CSRF token), `serde_json` dependencies and `memoryd` as library dependency.
- Implement `auth.rs` with CSRF token generation, middleware, and `X-Memorum-CSRF` header validation.
- Implement `server.rs` with axum router, localhost-only bind enforcement, graceful shutdown (drain up to 5 seconds), `rust-embed` static asset serving.
- Implement `config.rs` reading `[web]` section from config.yaml.
- Static assets: minimal `index.html` with `<meta name="csrf-token">` injection, placeholder `app.js`, empty `style.css`.

**Step 3: GREEN command**

```bash
cargo test -p memoryd-web --test csrf
cargo test -p memoryd-web --test concurrent_access
cargo build -p memoryd-web
```

**Verification plan:**

- Primary: `cargo test -p memoryd-web --test csrf && cargo test -p memoryd-web --test concurrent_access`
- Secondary: `cargo build -p memoryd-web`

---

## Task 14: Web Dashboard — 4 API Sections And SSE Notification Stream

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 13
**Owned files:** `crates/memoryd-web/src/routes/status.rs`, `crates/memoryd-web/src/routes/entity_graph.rs`, `crates/memoryd-web/src/routes/roi.rs`, `crates/memoryd-web/src/routes/reality_check.rs`, `crates/memoryd-web/src/routes/audit.rs`, `crates/memoryd-web/src/routes/review.rs`, `crates/memoryd-web/tests/api_contract.rs`
**Invariants:** All routes return `application/json`. All mutating POST routes enforce CSRF. Concurrent mutations to the same memory return 409 with `{ "error": "memory_not_in_review_state" }`. The audit route assembles the full `TrustArtifact` DTO from `memoryd::trust_artifact` (Task 12). `GET /api/audit/:id/temporal?at=<ts>` is read-only — no time-travel writes. Deferred web sections (§11.2 policy editor, §11.3 sync dashboard) are not implemented; respond to their future routes with 501 Not Implemented and a note in the response body.
**Out of scope:** Frontend JS/HTML implementation (out of scope for v1; the static SPA shell and placeholder `app.js` from Task 13 are the v1 frontend).

**Files:**

- Create: 6 route files under `crates/memoryd-web/src/routes/`
- Test: `crates/memoryd-web/tests/api_contract.rs`

**Step 1: RED tests**

Create `crates/memoryd-web/tests/api_contract.rs` matching spec §10.2:

- `test_get_status_returns_correct_shape`: mock daemon status; assert JSON shape matches spec §4.3 schema.
- `test_get_entity_graph_returns_nodes_and_edges`: mock entity data; assert response has `nodes[]` and `edges[]`.
- `test_post_review_action_approve_calls_daemon`: mock review action; assert daemon `review_approve` fired with correct id.
- `test_post_review_action_returns_409_on_wrong_state`: mock daemon returning wrong-state error; assert HTTP 409.
- `test_get_audit_returns_full_trust_artifact`: mock memory with all trust artifact fields; assert all sections from spec §4.3 audit shape present.
- `test_get_audit_temporal_returns_historical_state`: mock temporal query; assert `viewing_historical_state: true` in response.
- `test_get_roi_30d_returns_correct_window`: assert `window_days: 30`.
- `test_get_roi_365d_returns_correct_window`: assert `window_days: 365`.
- `test_get_reality_check_returns_pending_list`: mock scored items; assert response matches `RealityCheckResponse::Pending` shape.
- `test_post_reality_check_respond_dispatches_to_daemon`: mock confirm action; assert daemon `RealityCheckRequest::Respond` call fired.

Run:

```bash
cargo test -p memoryd-web --test api_contract
```

Expected: FAIL because routes are not implemented.

**Step 2: GREEN implementation**

- Implement all 6 route files following the spec §4.3 route table and JSON shapes.
- Wire routes into axum router in `server.rs`.
- `audit.rs` calls `memoryd::trust_artifact::TrustArtifactBuilder` from Task 12.
- `reality_check.rs` routes POST to daemon `RequestPayload::RealityCheck`.
- `review.rs` serializes 409 on wrong-state daemon error.

**Step 3: GREEN command**

```bash
cargo test -p memoryd-web --test api_contract
cargo test -p memoryd-web --test csrf
```

**Verification plan:**

- Primary: `cargo test -p memoryd-web --test api_contract`
- Secondary: `cargo test -p memoryd-web --test concurrent_access`

---

## Task 15: Slash Command Integrations

**Subagent type:** `cli_developer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 16 after Task 14
**Blocked by:** Task 14
**Owned files:** `crates/memoryd/src/slash_commands.rs`, `crates/memoryd/tests/slash_commands.rs`
**Invariants:** Slash commands are Tier 1 harness-only (Claude Code, Codex CLI). The `/memory-reality-check` command calls `memoryd reality-check run --json` and formats output for human reading, not agent consumption. If `safe_plaintext_fragment` returns `OmitEncryptedBodyHidden` for a title, render `[encrypted item, score: X.XX]`. Slash command output contains no raw memory bodies — only titles (via `safe_plaintext_fragment`), namespaces, and scores. If no items pending: emit the "No Reality Check items" message from spec §9.8.
**Out of scope:** `/memory-status`, `/memory-search` (those are Stream B surfaces and remain unchanged). New MCP tools — frozen.

**Files:**

- Create: `crates/memoryd/src/slash_commands.rs`
- Test: `crates/memoryd/tests/slash_commands.rs`

**Step 1: RED tests**

Create `crates/memoryd/tests/slash_commands.rs`:

- `test_slash_reality_check_formats_scored_list`: mock 3 scored items with known scores and titles; assert output contains each title (via `safe_plaintext_fragment`) and score in the format from spec §9.8.
- `test_slash_reality_check_encrypted_item_shown_as_encrypted`: mock scored item with `encrypted: true`; assert output shows `[encrypted item, score: X.XX]`.
- `test_slash_reality_check_no_items_pending`: mock empty scored list; assert output matches "No Reality Check items pending" message.
- `test_slash_reality_check_output_contains_no_raw_bodies`: mock items with body content; assert output does NOT contain raw body strings.

Run:

```bash
cargo test -p memoryd --test slash_commands
```

Expected: FAIL.

**Step 2: GREEN implementation**

- Create `slash_commands.rs` with `format_reality_check_output(items: &[RealityCheckItem]) -> String` applying `safe_plaintext_fragment` to titles and assembling the spec §9.8 format.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test slash_commands
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test slash_commands`
- Secondary: `rg -n "memory-reality-check\|safe_plaintext_fragment" crates/memoryd/src/slash_commands.rs`

---

## Task 16: CLI — `memoryd ui`, `memoryd web`, And `memoryd reality-check` Subcommands

**Subagent type:** `cli_developer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 15 after Task 14
**Blocked by:** Task 14
**Owned files:** `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/tests/cli_contract.rs`
**Invariants:** Exit codes match spec §9.1–§9.7 exactly. `memoryd ui` rejects non-TTY stdin. `memoryd web enable` enforces localhost-only binding. `memoryd reality-check run --json` prints scored list and exits without interactive prompts. `memoryd reality-check snooze --until` validates the date. All `memoryd reality-check` subcommands route to daemon `RequestPayload::RealityCheck` (not to the scoring library directly). `memoryd dream ...` commands (Stream F) are unchanged.
**Out of scope:** TUI main loop (Task 10 owns that). Web server lifecycle (Task 13 owns that). Slash commands (Task 15).

**Files:**

- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/main.rs`
- Modify: `crates/memoryd/src/handlers.rs`
- Test: `crates/memoryd/tests/cli_contract.rs`

**Step 1: RED tests**

Create/update `crates/memoryd/tests/cli_contract.rs`:

- `test_clap_parses_memoryd_ui_panel_flag`: `memoryd ui --panel 3`; assert `Cli::parse()` produces `Command::Ui { panel: 3 }`.
- `test_clap_rejects_panel_out_of_range`: `--panel 9`; assert clap error exit.
- `test_clap_parses_web_enable_with_port`: `memoryd web enable --port 7138`; assert `Command::WebEnable { port: 7138 }`.
- `test_clap_parses_web_disable`: `memoryd web disable`; assert `Command::WebDisable`.
- `test_clap_parses_web_status_json_flag`: `memoryd web status --json`; assert `Command::WebStatus { json: true }`.
- `test_clap_parses_reality_check_run`: `memoryd reality-check run --top-n 5 --namespace me`; assert correct fields.
- `test_clap_parses_reality_check_skip`: `memoryd reality-check skip`; assert `Command::RealityCheckSkip`.
- `test_clap_parses_reality_check_snooze_until`: `memoryd reality-check snooze --until 2026-05-10`; assert date parsed.
- `test_memoryd_ui_rejects_non_tty`: mock non-TTY stdin; assert exit code 2.
- `test_memoryd_web_enable_delegates_to_daemon`: mock daemon socket; assert `RequestPayload::WebEnable { port: 7137 }` sent.
- `test_memoryd_reality_check_run_json_exits_without_interactive`: `--json` flag; mock daemon returning scored list; assert output is JSON-parseable, exit 0.
- `test_memoryd_reality_check_snooze_invalid_date_exits_1`: pass non-date string to `--until`; assert exit 1.

Run:

```bash
cargo test -p memoryd --test cli_contract
```

Expected: FAIL because new CLI subcommands are not yet wired.

**Step 2: GREEN implementation**

- Add new `clap` subcommands to `cli.rs`: `ui`, `web {enable, disable, status}`, `reality-check {run, skip, snooze}`.
- Wire each command to its daemon `RequestPayload` variant in `main.rs` dispatch.
- `memoryd ui`: **execs the `memoryd-tui` binary as a subprocess** (not in-process). The two-binary model keeps `memoryd`'s core dependency tree free of `ratatui`/`crossterm`/terminal-event machinery, parallels how many CLIs work (`git ui` → `gitk`, etc.), and makes the TUI dependency footprint visible at install time. The exec passes through any flags after `ui` (e.g., `memoryd ui --panel 3` becomes `memoryd-tui --panel 3 --socket <path>`). Use `std::process::Command::exec` (or `tokio::process::Command::status` followed by `std::process::exit(status.code())`) so signals propagate naturally. If the `memoryd-tui` binary is not on PATH or not in the same parent directory as `memoryd`, exit 4 with a helpful message ("memoryd-tui binary not found; reinstall with `cargo install memoryd-tui` or ensure both binaries are in the same prefix"). Check TTY first; exit 2 if not.
- `memoryd web enable`: send `RequestPayload::WebEnable` to daemon; print URL on success.
- `memoryd reality-check run --json`: send `RequestPayload::RealityCheck(RealityCheckRequest::List { ... })`, print JSON, exit.
- `memoryd reality-check run` (interactive): route to slash command formatter or a minimal terminal interactive session.

**Step 3: GREEN command**

```bash
cargo test -p memoryd --test cli_contract
cargo test -p memoryd --test daemon_e2e
```

**Verification plan:**

- Primary: `cargo test -p memoryd --test cli_contract`
- Secondary: `cargo test -p memoryd --test server_smoke`

---

## Review Gate D: Web Dashboard, CLI, And Security Review

**Subagent types:** `reviewer`, `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Review agents must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 13–16 integrated and green
**Owned files:** `docs/reviews/stream-g-review-gate-d-clean-code.md`, `docs/reviews/stream-g-review-gate-d-security.md`
**Invariants:** Review only.
**Out of scope:** Performance gate (Task 17).

**Review focus:**

- Web server never binds to `0.0.0.0`; config enforcement is hard-wired (spec §8, §4.4).
- CSRF enforcement covers all POST routes; no bypass paths.
- 409 on concurrent mutation is correct and not racy (daemon single-writer model).
- CLI exit codes match spec §9.1–§9.7 precisely.
- `memoryd reality-check run --json` does NOT start an interactive session.
- Slash command output does not contain raw memory bodies.
- `TrustArtifact` assembly queries events log for `recall_count_30d`/`last_recalled` — no ghost `memories` columns.
- Deferred web sections return 501, not 404.
- Static assets are embedded; no CDN network calls in routes.

**Commands:**

```bash
cargo test -p memoryd-web --test csrf --test api_contract --test concurrent_access
cargo test -p memoryd --test cli_contract --test slash_commands --test trust_artifact
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents, same review lane rerun, and severity-3 findings either fixed or logged with rationale before Task 17.

---

## Task 17: Performance Gate — Scoring Budget, TUI Render Budget, And Web Latency

**Subagent type:** `performance_engineer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 18 after Review Gate D
**Blocked by:** Review Gate D
**Owned files:** `crates/memoryd/src/bin/stream_g_bench.rs`, `bench/stream-g-observability-results.darwin-arm64.json`, `docs/reviews/stream-g-bench-evidence.md`, `crates/memoryd/Cargo.toml`
**Invariants:** Bench fixture is deterministic. Assertion/smoke mode must not dirty the tree. Updating `bench/stream-g-observability-results.darwin-arm64.json` happens only through an explicit `--write-output` invocation and human-authored commit. Performance baselines at `bench/baseline.*.json` are never overwritten by this bench binary. Do not mask failing performance by raising thresholds — if scoring 10k memories exceeds 500 ms p95, report the failure and stop. See spec §12 for all budget values.
**Out of scope:** Product feature changes.

**Files:**

- Create: `crates/memoryd/src/bin/stream_g_bench.rs`
- Create: `bench/stream-g-observability-results.darwin-arm64.json`
- Create: `docs/reviews/stream-g-bench-evidence.md`
- Modify: `crates/memoryd/Cargo.toml`

**Steps:**

1. Add bench binary covering (per spec §12):
   - **Scoring budget** (spec §12.3): score computation for 10,000 memories with realistic `recall_count_30d` and `distinct_sources` values; assert p95 ≤ 500 ms. Top-N selection asserts ≤ 50 ms on top of scoring. Session resume from persisted state asserts ≤ 100 ms.
   - **TUI render budget** (spec §12.1): synthetic key-event-to-frame latency for panel switches (≤ 16 ms); memory detail modal open round-trip (≤ 32 ms); entity search typeahead including 100 ms debounce (≤ 100 ms).
   - **Web latency budget** (spec §12.2): `GET /api/entity-graph` serialization with 5,000 nodes (≤ 200 ms); `GET /api/status` p99 (≤ 50 ms).
   - **Notification dispatcher** (spec §12.4): passive queue append (≤ 1 ms); Slack dispatch first attempt with local mock (≤ 2 seconds).

2. **First-run bootstrap**: on the very first execution there is no `bench/stream-g-observability-results.darwin-arm64.json` to assert against. The bench binary must implement the same bootstrap path the existing Stream A bench harness uses: when `--assert` is invoked and the baseline file is missing or `runs: 0`, the binary writes a `bench/stream-g-observability-results.darwin-arm64.json.proposed` file with the measurement and exits 0 with a stderr message "first run — wrote .proposed; commit as baseline once verified." It does NOT overwrite the canonical baseline. This matches the Stream A precedent of `bench/baseline.linux-x86_64.json` (`runs: 0` placeholder, first-run emits `.proposed`).

3. RED/green via asserting command — once a baseline exists, exits nonzero on budget regression, does NOT write output file:

   ```bash
   cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
   ```

4. Separate explicit update command for release evidence (not used by routine gates; only invoked by an explicit human-authored commit per spec §17.6/§18.9 invariant):

   ```bash
   cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --write-output bench/stream-g-observability-results.darwin-arm64.json
   ```

5. Write `docs/reviews/stream-g-bench-evidence.md` with each spec §12 budget, measured p95, pass/fail, and residual risks. On the first task execution, this evidence file references the `.proposed` baseline and notes that Trey must commit the canonical baseline manually before the release gate.

**Verification plan:**

- Primary (after first commit of baseline): `cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json`
- Bootstrap (first execution): same command emits `.proposed` and exits 0 with bootstrap stderr; verify the `.proposed` file is well-formed JSON with all expected budget fields populated.
- Secondary: `jq . bench/stream-g-observability-results.darwin-arm64.json` (after the human commit lands the canonical baseline).

---

## Task 18: API Docs, Architecture Docs, And Runbooks

**Subagent type:** `docs_editor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes — parallel with Task 17 after Review Gate D
**Blocked by:** Review Gate D
**Owned files:** `docs/api/stream-g-observability-api.md`, `docs/dev/stream-g-architecture.md`, `docs/runbooks/reality-check.md`, `docs/api/stream-a-public-api.md`, `docs/api/stream-e-passive-recall-api.md`, `README.md`, `CLAUDE.md`
**Invariants:** Do not edit spec files. Document `EventKind::RecallHit` and the covering index in Stream A API docs. Document `<pending-attention kind="reality_check_due">` addition in Stream E API docs. Reality Check runbook must cover the full weekly ritual from invocation to session complete, including abandon/resume. CLAUDE.md status section must reflect "Streams A–G shipped" after this task's changes are accepted.
**Out of scope:** Spec version bumps.

**Files:**

- Create: `docs/api/stream-g-observability-api.md`
- Create: `docs/dev/stream-g-architecture.md`
- Create: `docs/runbooks/reality-check.md`
- Modify: `docs/api/stream-a-public-api.md`
- Modify: `docs/api/stream-e-passive-recall-api.md`
- Modify: `README.md`
- Modify: `CLAUDE.md`

**Steps:**

1. Create `docs/api/stream-g-observability-api.md` with:
   - TUI invocation (`memoryd ui`), all 8 panels summary, keymap reference.
   - Web dashboard routes (from spec §4.3), JSON shapes, CSRF requirement.
   - Reality Check CLI commands with exit codes (spec §9.5–§9.7).
   - `NotificationEvent` enum listing all 7 variants with trigger conditions.
   - Daemon protocol `RealityCheck*` wire shapes (spec §5.7) — admin-only, MCP-rejected.
   - Slash command `/memory-reality-check` output format.
   - Deferred sections (§11.2 policy editor, §11.3 sync dashboard) flagged explicitly as v1.1+.

2. Create `docs/dev/stream-g-architecture.md` with:
   - Crate split rationale (`memoryd-tui` / `memoryd-web` have disjoint dependency trees).
   - Data flow: TUI/web → Unix socket → daemon → substrate/events; no direct substrate access from UI crates.
   - Scoring pipeline: index scan + events log aggregate queries via covering index, not per-memory reads.
   - Notification dispatch: broadcast channel → passive queue + conditional OS/external routing.
   - State file layout and crash recovery summary (referencing spec §5.8).

3. Create `docs/runbooks/reality-check.md` with:
   - Weekly ritual walkthrough: due notification → `memoryd reality-check run` or TUI Panel 8 → per-item actions → session complete.
   - Session abandon and resume: what persists in `reality-check-session.json`, how to resume or discard.
   - Snooze vs. skip: difference, effect on next week's queue.
   - Overdue handling: 21-day threshold, re-sorting behavior.
   - Encrypted memories in Reality Check: what the user sees, how to reveal before correct/confirm.
   - Operator commands for resetting stuck state: `memoryd reality-check reset`.

4. Update `docs/api/stream-a-public-api.md` with `EventKind::RecallHit` and covering index addition, noting Stream G as authorized consumer.

5. Update `docs/api/stream-e-passive-recall-api.md` with `<pending-attention kind="reality_check_due">` addition, item text, cap interaction.

6. Update `README.md` and `CLAUDE.md` to reflect Streams A–G shipped.

7. Verify docs contain required phrases:

   ```bash
   rg -n "RecallHit|covering index|reality_check_due|NotificationEvent|RealityCheckRequest|CSRF|trust artifact|drift.risk\|score.*formula\|admin-only\|MCP-rejected\|deferred.*v1\.1" docs/api docs/dev docs/runbooks README.md CLAUDE.md
   ```

**Verification plan:**

- Primary: the `rg` command above returns results for all key phrases.
- Secondary: `git diff --check docs/api docs/dev docs/runbooks README.md CLAUDE.md`

---

## Final Review Gate E: Full Independent Review Swarm

**Subagent types:** `reviewer`, `security_auditor`, `performance_engineer`, `test_hardener`, `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Every review subagent must load clean-code.
**Parallel:** yes
**Blocked by:** Tasks 17–18
**Owned files:** `docs/reviews/stream-g-final-clean-code-review.md`, `docs/reviews/stream-g-final-security-review.md`, `docs/reviews/stream-g-final-performance-review.md`, `docs/reviews/stream-g-final-test-review.md`, `docs/reviews/stream-g-final-api-contract-review.md`
**Invariants:** Review-only. Findings must cite files, tests, and spec clause numbers.
**Out of scope:** New feature requests beyond v0.1.

**Review lanes:**

1. **Clean-code/Rust maintainability:** module boundaries between `memoryd-tui`, `memoryd-web`, `memoryd::reality_check`, `memoryd::notifications`; function size; error handling; async boundaries; no direct substrate access from UI crates.
2. **Security/privacy:** CSRF enforcement; `0.0.0.0` bind rejection; no memory content in Slack/email payloads; SMTP password never in config file; `RealityCheck*` variants MCP-blocked; state files never in git-synced tree; no ghost `memories` columns for trust artifact data.
3. **Performance:** bench fixture determinism; scoring 10k memories ≤ 500 ms p95; TUI render ≤ 16 ms input-to-frame; web `GET /api/entity-graph` ≤ 200 ms at 5k nodes; notification passive queue append ≤ 1 ms.
4. **Test hardening:** acceptance matrix coverage from spec §10; vertical TDD evidence in all implementation tasks; snapshot test fixture determinism; negative paths (corrupt state files, socket unreachable, governance refusal in RC session, wrong CSRF, concurrent 409).
5. **API contract:** protocol/CLI/web route docs match shipped DTOs; `ComponentScores` field names match spec §5.7 wire shape; `NotificationEvent` seven variants match spec §1.3 exactly; web JSON shapes match spec §4.3.

**Commands reviewers should run as relevant:**

```bash
cargo test --workspace --all-targets --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

**Exit criteria:** all severity-1/2 findings fixed by scoped fix subagents; severity-3 findings either fixed or explicitly documented as non-blocking with Trey-facing rationale.

---

## Task 19: Performance Baseline Capture And Final Release Gate

**Subagent type:** Orchestrator-run final gate. Optional `heavy_worker` may draft the gate report from captured output only after the orchestrator runs commands directly.
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer for any optional report-drafting subagent.
**Parallel:** no
**Blocked by:** Final Review Gate E and all fixes
**Owned files:** `docs/reviews/stream-g-final-gate-report.md`
**Invariants:** Do not declare done unless all required gates pass or a blocker is documented with exact command/output. Do not overwrite `bench/baseline.*.json` — only `bench/stream-g-observability-results.darwin-arm64.json` is updated here, and only via the explicit `--write-output` command. `bench/baseline.darwin-arm64.json` is never touched by this task.
**Out of scope:** Opportunistic refactors after final review.

**Steps:**

1. Run targeted Stream G acceptance suite:

   ```bash
   cargo test -p memory-substrate --test event_kind_recall_hit --test index_covering_index
   cargo test -p memoryd --test recall_hit_emission --test startup_recall_mcp --test startup_recall_determinism
   cargo test -p memoryd --test daemon_state_files --test protocol_contract --test notification_channel
   cargo test -p memoryd --test scoring --test scheduling --test responses
   cargo test -p memoryd --test dispatcher --test trust_artifact --test slash_commands --test cli_contract
   cargo test -p memoryd-tui --test panel_render --test keymap --test socket_unreachable --test resize
   cargo test -p memoryd-web --test csrf --test api_contract --test concurrent_access
   cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --assert --baseline bench/stream-g-observability-results.darwin-arm64.json
   ```

2. Run broader Rust gates:

   ```bash
   cargo test --workspace --all-targets --all-features
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   ```

3. Run repo boundary/docs gates:

   ```bash
   ./scripts/rust-boundary-check.sh
   pnpm exec oxfmt --check .
   pnpm exec oxlint .
   git diff --check
   ```

4. Capture performance evidence:

   ```bash
   cargo run -p memoryd --bin stream_g_bench -- --profile darwin-arm64 --write-output bench/stream-g-observability-results.darwin-arm64.json
   ```

5. Run full release gate:

   ```bash
   BENCH_PROFILE=darwin-arm64 bash scripts/check.sh
   ```

6. Write `docs/reviews/stream-g-final-gate-report.md` with exact commands, pass/fail status, bench results vs. spec §12 budgets, and any residual risks.

**Verification plan:**

- Primary: all commands above pass.
- Secondary: `git status --short` shows only intended Stream G changes.

---

## Execution DAG Summary

1. Task 1 — contract map, worktree baseline.
2. Task 2 — Stream A: `RecallHit` + covering index.
3. Task 3 — Stream E: emit `RecallHit` from recall builders.
4. Review Gate A — then fixes if needed.
5. Task 4 — daemon state files + crash recovery primitives.
6. Task 5 — daemon protocol: `RealityCheck*` variants + `NotificationEvent` channel.
7. Task 6 — Reality Check scoring library (drift-risk formula).
8. Task 7 — Reality Check session lifecycle handlers.
9. Review Gate B — then fixes if needed.
10. Task 8 — notification dispatcher (passive/OS/external).
11. Task 9 — Stream E pending-attention `reality_check_due` integration.
12. Task 10 — `memoryd-tui` crate skeleton, 8-panel layout.
13. Task 11 — TUI keymap, interactive behaviors.
14. Task 12 — trust artifact widget (shared DTO).
15. Review Gate C — then fixes if needed.
16. Task 13 — `memoryd-web` crate skeleton, axum router, CSRF.
17. Task 14 — web dashboard 4 API sections.
18. Parallel Phase 5: Task 15 (slash commands) + Task 16 (CLI subcommands).
19. Review Gate D — then fixes if needed.
20. Parallel Phase 6: Task 17 (performance gate) + Task 18 (docs).
21. Final Review Gate E — then fixes if needed.
22. Task 19 — orchestrator-run final release gate and handoff.

## Stop Conditions

Stop and ask Trey only if one of these occurs:

- The Stream G v0.1 spec contradicts shipped Stream A–F code in a way that cannot be resolved additively (e.g., a required index column does not exist and would require a non-additive schema change).
- A required dependency for `memoryd-tui` (ratatui, crossterm) or `memoryd-web` (axum, rust-embed) cannot be added because of an irreconcilable workspace version conflict.
- The `cron` crate used for Reality Check scheduling does not support the spec §5.2 cron format; an alternative is needed but the choice is not obvious.
- Final gates expose unrelated pre-existing failures that cannot be isolated from Stream G changes.
- System-v0.2 §19 authorization table is insufficient for a surface change required by the spec (e.g., a Stream A type needs to change non-additively).

Everything else should be handled by spawning scoped subagents, fixing findings, and rerunning gates.
