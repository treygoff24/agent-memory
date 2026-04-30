# Stream E Passive Recall Implementation Plan

**Goal:** Build Stream E passive recall exactly from `docs/specs/stream-e-passive-recall-v0.5.md`: `memory_startup`, startup and delta recall blocks, deterministic ranking/budgeting, safe privacy handling, project/session binding, and additive recall status counters.

**Architecture:** Stream A remains the only persisted memory/index layer; Stream E adds additive query filters/index columns plus a public recall-index read API over Stream A's existing tag/entity/alias projections so startup recall does not hydrate every active envelope. Stream D remains the only privacy classifier/reveal authority; Stream E consumes a new `safe_plaintext_fragment` helper that classifies under the strict `PrivacyNamespace::Me` namespace and never decrypts. The `memoryd` crate owns the daemon/MCP/CLI surfaces and a new internal `recall` module split into small deterministic units for binding, project config, candidate collection, entity matching, ranking, budgeting, rendering, deltas, and counters.

**Tech Stack:** Rust 2021 workspace, `tokio`, `serde`/`serde_json`, `serde_yaml`, `yaml-rust2` or equivalent low-level YAML event parser for duplicate-key rejection, `chrono`, Stream A `memory-substrate`, Stream D `memory-privacy`, `memoryd` Unix-socket protocol/MCP bridge/CLI, hand-rolled XML escaping in `memoryd::recall::render` with no new XML dependency, vertical TDD integration tests and release-gate perf probes.

---

## Plan Revision History

- **v0.1 / 2026-04-30:** Initial Stream E implementation plan grounded in `docs/specs/stream-e-passive-recall-v0.3.md`, current Stream C/D shipped code, and the live workspace structure.
- **v0.2 / 2026-04-30:** Incorporated adversarial plan review fixes before implementation: Stream A recall-index read API, concrete SQLite v2 migration/backfill, strict `PrivacyNamespace::Me` fragment classification, daemon-routed CLI counters, no hot-path doctor, lockfile ownership correction, and sharper output/perf/security acceptance.
- **v0.3 / 2026-04-30:** Repointed source contract from `stream-e-passive-recall-v0.3.md` to `stream-e-passive-recall-v0.4.md` after Trey-approved spec patch closed two correctness gaps: (1) §4.2 git-remote canonicalization is now URL-form-agnostic so SSH/HTTPS clones of the same upstream produce identical `canonical_id`; (2) §3.3 `RecallOmission` gains optional `alias` + `colliding_ids` fields and §7 alias-collision rule emits exactly one omission per `(section, alias)` collision. Plan updates: Task 1 contract map drops the two open clarification questions; Task 6 DTO list notes the new `RecallOmission` fields; Task 7 binding tests gain SSH↔HTTPS / case / `.git`-suffix equivalence cases; Task 8 entity resolution emits the v0.4 collision shape; Task 18 contract review checks JSON-additive serde defaults for the new fields; all `stream-e-v0.3` policy strings become `stream-e-v0.4`.
- **v0.4 / 2026-04-30:** Repointed source contract from `stream-e-passive-recall-v0.4.md` to `stream-e-passive-recall-v0.5.md` after fresh-context plan-reviewer pass surfaced three pre-build blockers grounded in shipped code: (1) `crates/memoryd/src/handlers.rs:1553` already defines a private `fn safe_plaintext_fragment(text: &str) -> bool` with seven call sites that collide with the new public Stream D helper of the same name; (2) `RecallIndexRow.index_body` cannot be served from the index without either a new column or forbidden JSON extraction, since `retrieval_policy.index_body` lives only in `frontmatter_json`; (3) v0.4 §9.5 listed `<pending-attention>` as carrying a doctor-derived repair-finding count, but Task 10 correctly forbids running `Substrate::doctor()` in the startup hot path and no daemon-cached doctor projection exists. Resolutions: (a) spec v0.5 removes the doctor-count line from §9.5 and explicitly defers it to a post-Stream-E follow-up; (b) Task 2 gains a second new column `index_body INTEGER NOT NULL DEFAULT 1` with the same migration-and-backfill pattern as `passive_recall` and corresponding tests; (c) Task 10 now owns renaming the existing `handlers.rs` private `safe_plaintext_fragment` to `is_safe_plaintext_for_indexing` (with all seven call sites) before importing the new Stream D public helper. Nit cleanups absorbed in the same revision: Task 9 review checklist version string, Task 1 `oxfmt` markdown caveat, Task 7 `cargo update` ordering, Task 11 manual-smoke daemon prerequisite, and a global `stream-e-v0.4` → `stream-e-v0.5` sweep in Task 18's checklist.

## Source Contract And Dirty-Tree Baseline

Normative contract:

- `docs/specs/stream-e-passive-recall-v0.5.md`
- Existing shipped surfaces in:
  - `crates/memory-substrate/src/model.rs`
  - `crates/memory-substrate/src/index/{schema.rs,migrations.rs,query.rs}`
  - `crates/memory-substrate/src/api.rs`
  - `crates/memory-privacy/src/{classifier.rs,decision.rs,lib.rs}`
  - `crates/memoryd/src/{protocol.rs,mcp.rs,handlers.rs,cli.rs,main.rs,server.rs}`
  - existing Stream C/D docs under `docs/api/`

Current dirty-tree baseline at plan time:

- Untracked specs:
  - `docs/specs/stream-e-passive-recall-v0.1.md` (historical)
  - `docs/specs/stream-e-passive-recall-v0.2.md` (historical)
  - `docs/specs/stream-e-passive-recall-v0.3.md` (historical, superseded by v0.4)
  - `docs/specs/stream-e-passive-recall-v0.4.md` (historical, superseded by v0.5)
  - `docs/specs/stream-e-passive-recall-v0.5.md` (live contract)

Do not overwrite or normalize those spec files unless Trey explicitly asks. This plan creates only `docs/plans/2026-04-30-stream-e-passive-recall.md`.

## Skill Survey And Routing

The root orchestrator surveyed the active skills available in this Codex session. Relevant build skills:

- `rust-engineer` — mandatory for every Rust/Cargo implementation and every Rust code review.
- `clean-code` — mandatory for all implementation and review subagents; use to keep recall assembly modular and readable.
- `tdd` and `tdd-workflow` — mandatory for implementation; use vertical TDD, not horizontal all-tests-first.
- `writing-plans` — used to author this file.
- `spec-quality-checklist` — use in Task 1 only to sanity-check v0.3 requirements before coding.
- `debugging-systematic` / `diagnose` — reserve for failing gates or non-obvious perf/test regressions.
- `claude-review` / `claude-second-opinion` — optional external adversarial review only if Trey explicitly asks; not required for the default Codex-only execution.
- Vercel, Slack, web/search, and image skills are not relevant to this Rust workspace implementation.

Every implementation, QA, performance, security, docs, and review subagent prompt must include:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
```

When invoking Codex skills, map `tdd` to both the concise `tdd` behavior-test discipline and the stricter `tdd-workflow` red/green/refactor checkpoints. Rust workers must load `/Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md`.

## Orchestrator Operating Model

The main agent is the orchestrator. Subagents do all substantive implementation/review work. The orchestrator may only:

1. Create/update the task DAG.
2. Spawn bounded subagents with non-overlapping owned files per parallel batch.
3. Integrate completed subagent branches/worktrees in dependency order.
4. Resolve merge conflicts caused by integration.
5. Run narrow and full gates.
6. Spawn review/fix subagents before moving to the next phase.

The orchestrator must not casually implement feature code directly. If an issue is found in review, spawn a fix subagent scoped to the reviewed owned files, then rerun the same narrow gate and review check before advancing.

### Required Subagent Prompt Preamble

Use this preamble for every implementation, test, docs, review, security, and performance subagent:

```text
Mandatory skills: clean-code, tdd, rust-engineer.
Repository: /Users/treygoff/Code/agent-memory.
You are implementing Stream E passive recall from docs/specs/stream-e-passive-recall-v0.5.md. Treat Stream A as the only persisted substrate, Stream C as the authoritative governance lifecycle, and Stream D as the only privacy/reveal authority. Use vertical TDD: write one failing behavior test, run it and record the RED failure, implement the smallest correct slice, rerun the narrow gate to GREEN, then refactor only while green. Do not touch files outside your Owned files. Do not overwrite untracked Stream E spec files.
```

## Parallelization Map

- **Phase 0:** Task 1 sequential. Locks contract and test harness plan.
- **Phase 1:** Tasks 2 and 3 can run in parallel after Task 1. They touch separate crates (`memory-substrate` and `memory-privacy`).
- **Review Gate A:** Tasks 4 and 5 run after Phase 1 integration and fixes.
- **Phase 2:** Tasks 6, 7, and 8 run sequentially after Review Gate A. They share `memoryd::recall` module aggregation and several deterministic fixture tests, so parallelizing them would create unnecessary ownership collisions.
- **Review Gate B:** Task 9 runs after Phase 2 integration and fixes.
- **Phase 3:** Tasks 10 and 11 are sequential because protocol/MCP/handler wiring depends on recall core DTOs and counters.
- **Review Gate C1:** Task 12 reviews protocol/MCP/CLI after Phase 3 and fixes.
- **Phase 4A:** Task 14 creates the privacy acceptance test before the privacy/security review so that review has a real target to run.
- **Review Gate C2:** Task 13 runs after Task 14 and any privacy fixes.
- **Phase 4B:** Task 15 runs after Task 13 because it shares startup rendering paths with Task 14. Task 16 can run in parallel with Task 15 only if the orchestrator confirms its owned files remain limited to bench/perf support.
- **Review Gate D:** Tasks 17, 18, and 19 run before final docs/gates.
- **Phase 5:** Tasks 20 and 21 close docs and full verification.

Before spawning any parallel batch, run an owned-file duplicate check on the relevant task block:

```bash
rg '\*\*Owned files:\*\*' docs/plans/2026-04-30-stream-e-passive-recall.md \
  | sed 's/.*\*\*Owned files:\*\* *//' \
  | tr ',' '\n' \
  | sed 's/`//g' \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | rg -v '^$' \
  | sort \
  | uniq -d
```

Expected full-plan duplicates: shared aggregator files such as `crates/memoryd/src/lib.rs`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/main.rs`, sequential recall files, docs, and this plan. Duplicates are not allowed inside any parallel implementation batch.

Before spawning a parallel batch, the orchestrator must write a temporary batch-owned-file list and run the same duplicate check against only that list. Parallel batches in this plan are:

- Phase 1: Tasks 2 and 3.
- Review Gate A: Tasks 4 and 5.
- Review Gate C1: Task 12 only, no parallel collision check needed.
- Phase 4B optional: Task 16 may run beside Task 15 only if its batch-specific owned-file list has no duplicate with Task 15.
- Review Gate D: Tasks 17, 18, and 19.

Example batch check:

```bash
cat > /tmp/stream-e-batch-owned-files.txt <<'EOF'
Task 2: crates/memory-substrate/src/model.rs
Task 2: crates/memory-substrate/src/index/schema.rs
Task 3: crates/memory-privacy/src/decision.rs
EOF
cut -d: -f2- /tmp/stream-e-batch-owned-files.txt \
  | sed 's/^[[:space:]]*//;s/[[:space:]]*$//' \
  | sort \
  | uniq -d
```

Expected: no output for the batch being spawned.

---

### Task 1: Contract Lock, API Map, And Test Matrix

**Subagent type:** `backend_arch`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer. Also use `spec-quality-checklist`.
**Parallel:** no
**Blocked by:** none
**Owned files:** `docs/plans/2026-04-30-stream-e-passive-recall.md`, `docs/reviews/stream-e-contract-map.md`
**Invariants:** Do not edit Stream E spec files. Do not soften v0.3 requirements.
**Out of scope:** No production code changes.

**Files:**

- Create: `docs/reviews/stream-e-contract-map.md`
- Modify: `docs/plans/2026-04-30-stream-e-passive-recall.md` only if the map exposes a plan/spec mismatch

**Step 1: Write the contract map**

Create `docs/reviews/stream-e-contract-map.md` with a table mapping each v0.3 section to implementation/test/docs tasks:

- Stream A `MemoryQuery` extension and index migration.
- Stream D `safe_plaintext_fragment`.
- MCP/daemon protocol DTOs.
- CLI startup/delta output and exit codes.
- session/project binding.
- candidate collection filters.
- entity/alias resolution.
- ranking/budgeting/rendering/explanations.
- privacy invariants.
- observability counters.
- acceptance tests/docs.

The map must also explicitly verify that the two v0.3 → v0.4 spec deltas have concrete owners and tests:

- §4.2 URL-form-agnostic git-remote canonicalization (SSH/HTTPS/git/file/bare-path normalization, hostname lowercasing, `.git` and trailing-slash stripping, repeated-slash collapse) is implemented in Task 7 and covered by Task 7's binding tests asserting SSH↔HTTPS, case, and `.git`-suffix equivalence on the same upstream.
- §3.3 `RecallOmission` extended fields (`alias: Option<String>`, `colliding_ids: Vec<String>`, both `skip_serializing_if`-default) live in the DTO defined by Task 6 and are populated by Task 8's entity resolver; Task 8 ranking tests assert exactly one omission per `(section, alias)` collision with `id = None` and `colliding_ids` sorted lexicographically; Task 18 API contract review verifies JSON additivity for tolerant clients (omissions without those fields still deserialize, omissions with default values still skip them on the wire).

**Step 2: Verify no hidden scope**

Run:

```bash
rg -n "Stream E|memory_startup|startup-block|delta-block|safe_plaintext_fragment|MemoryQuery|RecallExplanation|StatusResponse|passive_recall" \
  docs/specs/stream-e-passive-recall-v0.5.md docs/api README.md CLAUDE.md crates
```

Expected: current code still has `memory_startup` short-circuiting as `not_implemented`; plan tasks cover every required replacement.

**Verification plan:**

- Primary command: `rg -n "TODO|TBD|not implemented|Stream E" docs/reviews/stream-e-contract-map.md docs/plans/2026-04-30-stream-e-passive-recall.md` plus an orchestrator read-through. `oxfmt` is a JS/TS formatter and either silently skips Markdown or fails with an "excluded by ignore rules" diagnostic, so it is not used as a primary content gate here. If a future workspace-level Markdown linter is added, this verification plan should be updated; until then, content correctness is a human-readable review, not a formatter pass.
- Secondary check: orchestrator reads the map before spawning Task 2/3.

---

### Task 2: Stream A `MemoryQuery` Extension, Recall Index API, And Migration Semantics

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Phase 1
**Blocked by:** Task 1
**Owned files:** `crates/memory-substrate/src/model.rs`, `crates/memory-substrate/src/error.rs`, `crates/memory-substrate/src/index/schema.rs`, `crates/memory-substrate/src/index/migrations.rs`, `crates/memory-substrate/src/index/query.rs`, `crates/memory-substrate/src/api.rs`, `crates/memory-substrate/tests/memory_query_extension.rs`, `docs/api/stream-a-public-api.md`
**Invariants:** Defaults preserve current `MemoryQuery` behavior. No full-table envelope hydration for new filters, startup entity matching, or ranking fields. No JSON extraction from `frontmatter_json` in hot-path recall queries — `passive_recall` and `index_body` must each be served from indexed columns. JSON extraction is acceptable only inside the one-time v2 migration/backfill.
**Out of scope:** Do not implement recall block assembly.

**Files:**

- Modify: `crates/memory-substrate/src/model.rs`
- Modify: `crates/memory-substrate/src/error.rs`
- Modify: `crates/memory-substrate/src/index/schema.rs`
- Modify: `crates/memory-substrate/src/index/migrations.rs`
- Modify: `crates/memory-substrate/src/index/query.rs`
- Modify: `crates/memory-substrate/src/api.rs`
- Test: `crates/memory-substrate/tests/memory_query_extension.rs`
- Docs: `docs/api/stream-a-public-api.md`

**Step 1: Write the failing query-extension and recall-index tests**

Create `crates/memory-substrate/tests/memory_query_extension.rs` with vertical cases:

1. Default `MemoryQuery::default()` still returns existing active plaintext records as before.
2. `status: Some(MemoryStatus::Pinned)` returns only pinned rows.
3. `passive_recall_only: true` excludes `retrieval_policy.passive_recall = false`.
4. `updated_since` uses inclusive `>=` semantics.
5. `namespace_prefix = "me"` maps to `scope = "user"`.
6. `namespace_prefix = "agent"` maps to `scope = "agent"`.
7. `namespace_prefix = "project:proj_alpha"` maps to `scope = "project"` plus `canonical_namespace_id = "proj_alpha"`.
8. `namespace_prefix = "org:org_alpha"` maps to `scope = "org"` plus `canonical_namespace_id = "org_alpha"`.
9. invalid prefixes return a typed Stream A `invalid_query` substrate error.
10. upgraded v1 index databases gain `passive_recall` and `index_body` columns exactly once and reopen successfully on a second daemon start.
11. upgraded v1 rows with `frontmatter.retrieval_policy.passive_recall = false` are backfilled to `passive_recall = 0` before any recall query.
12. upgraded v1 rows with `frontmatter.retrieval_policy.index_body = false` are backfilled to `index_body = 0` before any recall query, so snippet rendering decisions stay correct without envelope hydration.
13. the recall-index API returns ranked-row fields without calling `read_memory_envelope`: id, path, summary, status, scope, canonical namespace id, updated_at, confidence, source kind, sensitivity, passive_recall, index_body, tags, aliases, entities, and entity aliases.
14. the recall-index API can match entity id, entity label/alias, memory alias, and tag through the existing `memory_entities`, `memory_entity_aliases`, `memory_aliases`, and `memory_tags` tables.

Run:

```bash
cargo test -p memory-substrate --test memory_query_extension
```

Expected: compile failure because the new fields are absent.

**Step 2: Add DTO fields**

Extend `MemoryQuery` in `crates/memory-substrate/src/model.rs`:

```rust
pub struct MemoryQuery {
    pub id: Option<MemoryId>,
    pub tag: Option<String>,
    pub include_metadata_only: bool,
    pub status: Option<MemoryStatus>,
    pub namespace_prefix: Option<String>,
    pub passive_recall_only: bool,
    pub updated_since: Option<DateTime<Utc>>,
}
```

Ensure `Default` sets new fields to `None`/`false`.

Also add a Stream A public recall-index query surface so Stream E can rank and entity-match without hydrating every surviving envelope. Prefer explicit DTOs in `model.rs`, for example:

```rust
pub struct RecallIndexQuery {
    pub namespace_prefix: Option<String>,
    pub statuses: Vec<MemoryStatus>,
    pub passive_recall_only: bool,
    pub updated_since: Option<DateTime<Utc>>,
    pub match_terms: Vec<String>,
}

pub struct RecallIndexRow {
    pub id: MemoryId,
    pub path: RepoPath,
    pub summary: String,
    pub status: MemoryStatus,
    pub scope: Scope,
    pub canonical_namespace_id: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub confidence: f64,
    pub source_kind: SourceKind,
    pub sensitivity: Sensitivity,
    pub passive_recall: bool,
    pub index_body: bool,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub entities: Vec<Entity>,
}
```

If exact existing type names differ, use the shipped model types rather than strings. Implement a `Substrate::query_recall_index` or equivalently named API backed by SQLite joins/aggregates over the `memories`, `memory_tags`, `memory_aliases`, `memory_entities`, and `memory_entity_aliases` tables. It must be deterministic, sorted by memory id before Stream E scoring, and must not call `read_memory_envelope`.

**Step 3: Add index columns, indexes, and a concrete v2 migration**

Update `SCHEMA_SQL` with:

- `passive_recall INTEGER NOT NULL DEFAULT 1` in `memories`.
- `index_body INTEGER NOT NULL DEFAULT 1` in `memories`. This second column lets the recall-index API project `retrieval_policy.index_body` for §5 snippet-rendering decisions without hydrating the envelope or doing JSON extraction in the hot path. Both new columns are populated identically: from `SCHEMA_SQL` for fresh databases, via `ALTER TABLE` for upgraded ones, and from `frontmatter.retrieval_policy.<field>` on every upsert (Step 4).
- `idx_memories_status_passive_updated` on `(status, passive_recall, updated_at)`.
- `idx_memories_scope_canon_status_passive_updated` on `(scope, canonical_namespace_id, status, passive_recall, updated_at DESC)`.

Use a versioned migration. Do not rely on rerunnable bare `ALTER TABLE`; SQLite has no `ADD COLUMN IF NOT EXISTS` and rerunning a completed migration would fail.

Required migration pattern:

1. Bump `INDEX_SUPPORTED_SCHEMA_VERSION` from `1` to `2`.
2. After `execute_batch(SCHEMA_SQL)`, read `MAX(schema_migrations.version)`: if it is greater than 2, return `IndexSchemaVersionUnsupported`; if it is less than 2, run the v2 migration inside a single transaction so partial-failure leaves the database at v1.
3. Use `PRAGMA table_info(memories)` to check whether `passive_recall` and `index_body` are already present before running each `ALTER TABLE memories ADD COLUMN <name> INTEGER NOT NULL DEFAULT 1`. Both columns must be checked independently — a fresh-from-`SCHEMA_SQL` database has both, but if a future stream lands one column outside the v2 migration the `PRAGMA` check still skips correctly.
4. Backfill existing rows before recording version 2. Use one `UPDATE` per column so the SQL stays readable and each column's backfill is independently auditable:

   ```sql
   UPDATE memories
   SET passive_recall =
     CASE
       WHEN json_extract(frontmatter_json, '$.retrieval_policy.passive_recall') = 0 THEN 0
       ELSE 1
     END;

   UPDATE memories
   SET index_body =
     CASE
       WHEN json_extract(frontmatter_json, '$.retrieval_policy.index_body') = 0 THEN 0
       ELSE 1
     END;
   ```

   `rusqlite` is built with `serde_json`/bundled SQLite in this workspace and `frontmatter_json` already has a `json_valid` check; this JSON use is acceptable only inside the one-time migration/backfill, not in hot-path recall queries.
5. Create the new indexes with `CREATE INDEX IF NOT EXISTS`.
6. Insert `schema_migrations(version, applied_at)` for version 2 only after DDL and both backfills succeed inside the same transaction. If any step in the transaction fails, the rollback leaves the database at v1 and the next open re-attempts the full v2 migration cleanly.
7. Add tests that open an old v1 fixture, validate that `passive_recall = false` rows and `index_body = false` rows are both excluded/projected correctly immediately after upgrade, close/reopen the DB, and validate the second open does not rerun or fail either `ALTER TABLE`.

**Step 4: Populate `passive_recall` and `index_body` on every upsert/reindex**

Find the existing index upsert path in `crates/memory-substrate/src/index/query.rs` and persist `frontmatter.retrieval_policy.passive_recall` and `frontmatter.retrieval_policy.index_body` as `0/1` columns. Both columns are non-nullable; the upsert must always supply a value, never leave them defaulted.

**Step 5: Implement dynamic SQL filters and recall-index joins**

Refactor `query_memory` to build selective SQL with only active predicates:

- `id`
- `tag`
- `metadata_only`
- `status`
- `namespace_prefix`
- `passive_recall`
- `updated_since`

Do not build `(? IS NULL OR ...)` clauses.

Implement the recall-index API using selective SQL over the existing auxiliary tables. It must support:

- namespace/status/passive/updated filters from `MemoryQuery`;
- optional match terms against entity id, entity label, entity alias, memory alias, and tags;
- deterministic aggregation of tags/aliases/entities so Task 8 can perform collision handling and scoring without envelope hydration;
- the ranking metadata needed by spec §8.2: status, scope, updated_at, confidence, source_kind, and sensitivity.

**Step 6: Add the typed invalid-query error**

Add an explicit Stream A error surface for invalid query filters, preferably `SubstrateError::InvalidQuery { field, value, message }` or the nearest existing typed equivalent if the crate already has a stable validation variant. The serialized/logged reason must preserve the stable code `invalid_query` so Task 10 can map invalid namespace prefixes to daemon `invalid_request`.

**Step 7: Update docs**

Update `docs/api/stream-a-public-api.md` with the new fields, namespace semantics, migration/backfill behavior, recall-index API shape, auxiliary-table matching guarantees, and the explicit guarantee that Stream E ranking/entity matching does not hydrate every active envelope.

**Verification plan:**

- Primary command: `cargo test -p memory-substrate --test memory_query_extension`
- Regression command: `cargo test -p memory-substrate api_write_read reindex_reconciliation`
- Static checks:
  - `cargo fmt --all -- --check`
  - `cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings`

---

### Task 3: Stream D `safe_plaintext_fragment` Helper

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Phase 1
**Blocked by:** Task 1
**Owned files:** `crates/memory-privacy/src/decision.rs`, `crates/memory-privacy/src/lib.rs`, `crates/memory-privacy/tests/safe_plaintext_fragment.rs`, `docs/api/stream-d-privacy-api.md`
**Invariants:** Helper is deterministic, allocates no persistent state, classifies under the strict `PrivacyNamespace::Me` default tier, and never calls reveal/decrypt logic.
**Out of scope:** Do not alter storage routing semantics except to expose the helper.

**Files:**

- Modify: `crates/memory-privacy/src/decision.rs`
- Modify: `crates/memory-privacy/src/lib.rs`
- Test: `crates/memory-privacy/tests/safe_plaintext_fragment.rs`
- Docs: `docs/api/stream-d-privacy-api.md`

**Step 1: Write failing helper tests**

Create tests for:

- benign text returns `SafeFragmentDecision::Allow`;
- URL/date-only spans return `Allow`;
- high-entropy secret/JWT/private key/credential-like text returns `OmitEncryptedBodyHidden`;
- private email/phone/address/person/account returns `OmitReviewPending`;
- stricter result wins across mixed labels;
- tests assert the helper resolves policy with `PrivacyNamespace::Me`; do not use `Project` or `Agent` as a "neutral" namespace because those default to weaker `Internal` routing;
- repeated calls return identical decisions.

Run:

```bash
cargo test -p memory-privacy --test safe_plaintext_fragment
```

Expected: compile failure because the helper does not exist.

**Step 2: Implement public enum and helper**

Add:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeFragmentDecision {
    Allow,
    OmitEncryptedBodyHidden,
    OmitReviewPending,
}
```

Implement `safe_plaintext_fragment(classifier, fragment)` by classifying with `PrivacyNamespace::Me`, the strictest shipped namespace default, and mapping exactly as v0.3 specifies:

- final `PrivacyStorageAction::Refuse` or `PrivacyLabel::Secret` -> `OmitEncryptedBodyHidden`;
- final `PrivacyStorageAction::EncryptAtRest` or private/account labels -> `OmitReviewPending`;
- final plaintext, URL-only, date-only, or no spans -> `Allow`;
- strictest wins.

**Step 3: Update exports and docs**

Export the enum and helper from `crates/memory-privacy/src/lib.rs`; document in `docs/api/stream-d-privacy-api.md`.

**Verification plan:**

- Primary command: `cargo test -p memory-privacy --test safe_plaintext_fragment`
- Regression command: `cargo test -p memory-privacy`
- Static checks:
  - `cargo fmt --all -- --check`
  - `cargo clippy -p memory-privacy --all-targets --all-features -- -D warnings`

---

### Task 4: Review Gate A — Stream A Query Extension Review

**Subagent type:** `reviewer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Review Gate A
**Blocked by:** Task 2
**Owned files:** `docs/reviews/stream-e-query-extension-review.md`
**Invariants:** Read-only review except for writing the review report.
**Out of scope:** Do not fix code in this task.

**Files:**

- Create: `docs/reviews/stream-e-query-extension-review.md`

**Review checklist:**

- New filters are served from SQLite columns/indexes, not full hydration.
- The recall-index API reads Stream A's auxiliary entity/tag/alias tables and exposes all fields needed by Stream E ranking without envelope hydration.
- `index_body` is served from a real indexed column, not from a `json_extract` over `frontmatter_json`, in the recall-index hot path.
- Defaults preserve old behavior.
- Invalid namespace prefixes fail closed.
- Migration is safe for existing workspaces, backfills both `passive_recall` and `index_body` inside a single transaction before first post-upgrade recall query, and is safe on a second reopen (no double-`ALTER TABLE`, no partial-state v1.5).
- Tests actually execute and do not pass with zero cases.

**Verification plan:**

- Run: `cargo test -p memory-substrate --test memory_query_extension`
- Run: `cargo clippy -p memory-substrate --all-targets --all-features -- -D warnings`

---

### Task 5: Review Gate A — Stream D Helper Review

**Subagent type:** `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Review Gate A
**Blocked by:** Task 3
**Owned files:** `docs/reviews/stream-e-safe-fragment-security-review.md`
**Invariants:** Read-only review except for writing the review report.
**Out of scope:** Do not fix code in this task.

**Files:**

- Create: `docs/reviews/stream-e-safe-fragment-security-review.md`

**Review checklist:**

- No reveal/decrypt path is reachable.
- Classification is explicitly performed under `PrivacyNamespace::Me`, not a weaker project/agent namespace.
- Secret/high-risk fragments never become `Allow`.
- Review-pending private fragments are distinguishable from hard-hidden secrets.
- The helper cannot panic on arbitrary UTF-8.
- Docs match implementation.

**Verification plan:**

- Run: `cargo test -p memory-privacy --test safe_plaintext_fragment`
- Run: `cargo clippy -p memory-privacy --all-targets --all-features -- -D warnings`

---

### Task 6: Recall DTOs, Errors, Budgeting, And Stable Rendering Primitives

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Tasks 2, 3, 4, 5 and all Review Gate A fixes
**Owned files:** `crates/memoryd/src/lib.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/src/recall/error.rs`, `crates/memoryd/src/recall/types.rs`, `crates/memoryd/src/recall/budget.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/tests/startup_recall_determinism.rs`
**Invariants:** Deterministic byte-identical output for identical inputs. Token estimator is exactly `ceil(utf8_byte_len / 4)`. XML escaping is hand-rolled in `render.rs`; Task 6 must not add an XML crate or touch lockfiles.
**Out of scope:** No substrate queries, no MCP wiring.

**Files:**

- Modify: `crates/memoryd/src/lib.rs`
- Create: `crates/memoryd/src/recall/mod.rs`
- Create: `crates/memoryd/src/recall/error.rs`
- Create: `crates/memoryd/src/recall/types.rs`
- Create: `crates/memoryd/src/recall/budget.rs`
- Create: `crates/memoryd/src/recall/render.rs`
- Test: `crates/memoryd/tests/startup_recall_determinism.rs`

**Step 1: Write failing deterministic primitive tests**

In `startup_recall_determinism.rs`, start with tests for:

- `estimated_tokens("") == 0`, `estimated_tokens("abcd") == 1`, `estimated_tokens("abcde") == 2`;
- `truncate_utf8_bytes` handles CJK + emoji and appends `…` only on truncation;
- rendered entry truncation places `…` inside the summary/snippet field before the fixed `(updated …; source …; confidence …)` suffix, never at the end of the whole line;
- empty startup frame always contains all required section tags in order;
- omission list truncates to 64 entries and carries `omitted_truncated_count`;
- a `RecallOmission` with `reason = AmbiguousAlias`, `alias = Some("foo")`, `colliding_ids = vec!["a", "b"]`, `id = None` round-trips through serde and serializes with `alias` and `colliding_ids` keys present;
- a `RecallOmission` with `reason = BudgetExhausted` and default `alias`/`colliding_ids` serializes without `alias` or `colliding_ids` keys (additive JSON contract for tolerant clients);
- omissions sort by `(section, reason, alias.unwrap_or(""), id.unwrap_or(""))` so `ambiguous_alias` entries with `id = None` sort deterministically alongside id-keyed entries.

Run:

```bash
cargo test -p memoryd --test startup_recall_determinism
```

Expected: compile failure because `memoryd::recall` does not exist.

**Step 2: Add types and errors**

Implement Stream E DTOs:

- `StartupRequest`
- `StartupResponse`
- `SessionBinding`
- `ProjectBinding`
- `ProjectBindingSource`
- `RecallExplanation`
- `RecallSectionExplanation`
- `RecallOmission` — must include the v0.4 optional fields `alias: Option<String>` and `colliding_ids: Vec<String>`, both with `#[serde(skip_serializing_if = "Option::is_none")]` / `#[serde(default, skip_serializing_if = "Vec::is_empty")]` so non-collision omissions stay JSON-clean for tolerant clients.
- `OmissionReason`
- `RecallSectionName`
- internal `RecallError` mapping to protocol codes/exit codes.

**Step 3: Add budget and rendering primitives**

Implement:

- `estimated_tokens(&str) -> usize`;
- `truncate_utf8_bytes(value: &str, max_bytes: usize) -> TruncatedText`;
- stable XML escaping for attributes/text;
- no new dependency for XML escaping;
- section wrappers always in v0.3 order;
- explanation omission sorting `(section, reason, id)`.

**Verification plan:**

- Primary command: `cargo test -p memoryd --test startup_recall_determinism`
- Static command: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

### Task 7: Session Binding And Project Binding

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 6
**Owned files:** `Cargo.toml`, `crates/memoryd/Cargo.toml`, `crates/memoryd/src/recall/binding.rs`, `crates/memoryd/src/recall/project.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/tests/startup_recall_project_binding.rs`
**Invariants:** Project binding is recomputed per request; no cache. Malformed `.memory-project.yaml` fails closed. Missing git remote is not an error.
**Out of scope:** No recall ranking or MCP wiring.

**Files:**

- Modify: `Cargo.toml` if adding a low-level YAML parser workspace dependency
- Modify: `crates/memoryd/Cargo.toml`
- Create: `crates/memoryd/src/recall/binding.rs`
- Create: `crates/memoryd/src/recall/project.rs`
- Modify: `crates/memoryd/src/recall/mod.rs`
- Test: `crates/memoryd/tests/startup_recall_project_binding.rs`

**Step 1: Write failing project-binding tests**

Cover:

- absolute `cwd` validates and is canonicalized;
- relative/missing `cwd` returns `invalid_request`;
- empty/over-128-byte `session_id`, `harness`, and `harness_version` fail;
- `session_id`, `harness`, and `harness_version` are trimmed before both validation and persistence in `SessionBinding`; tests assert no leading/trailing whitespace survives;
- `.memory-project.yaml` wins over git remote;
- empty `.memory-project.yaml` fails;
- non-mapping YAML root fails;
- duplicate YAML keys fail before serde;
- unknown fields fail;
- unsupported scalar types fail;
- empty, too-short, and too-long `canonical_id` fail;
- `canonical_id` containing `:` fails;
- `canonical_id` containing illegal ASCII punctuation fails;
- non-ASCII `canonical_id` fails;
- alias over 128 bytes fails;
- no git remote degrades to namespaces `["me", "agent"]`;
- project binding namespaces are ordered `me`, `project:<id>`, `agent`;
- git-remote canonicalization (spec §4.2, introduced in v0.4 and inherited by v0.5): SSH (`git@github.com:foo/bar.git`) and HTTPS (`https://github.com/foo/bar.git`) clone-URL forms of the same upstream produce identical `canonical_id` and identical `namespaces_in_scope`;
- git-remote canonicalization (spec §4.2, introduced in v0.4 and inherited by v0.5): hostname case differences (`GitHub.com` vs `github.com`) produce identical `canonical_id`;
- git-remote canonicalization (spec §4.2, introduced in v0.4 and inherited by v0.5): trailing `.git`, trailing `/`, and repeated `/` runs in the path produce identical `canonical_id`;
- git-remote canonicalization (spec §4.2, introduced in v0.4 and inherited by v0.5): `git://` and `https://` forms of the same `host/path` produce identical `canonical_id`;
- git-remote canonicalization (spec §4.2, introduced in v0.4 and inherited by v0.5): bare-path and `file://` URLs canonicalize via `std::fs::canonicalize` and produce identical `canonical_id` for symlinked-but-equivalent paths.

Run:

```bash
cargo test -p memoryd --test startup_recall_project_binding
```

Expected: compile failure until binding/project modules exist.

**Step 2: Add YAML duplicate-key parser**

Use a low-level parser such as `yaml-rust2` to inspect mapping keys before serde. Add dependency only if not already available in the workspace. Workers update only `Cargo.toml`; `Cargo.lock` is orchestrator-merged after the task integrates.

Lockfile integration order (orchestrator):

1. Merge the worker's `Cargo.toml` change into `main`.
2. Run `cargo update -p yaml-rust2 --precise <version>` to resolve the new crate into `Cargo.lock`. **Do not** run `cargo build --workspace --locked` before this step — `--locked` refuses to add unknown crates and the build will fail with a confusing lockfile error rather than the actual root cause.
3. Run `cargo build --workspace --locked` to verify the lockfile resolves cleanly with no further updates required.
4. Commit `Cargo.lock` as part of the integration commit, not as a separate worker commit.

If `yaml-rust2` is already in the workspace dependency graph (transitively through another crate), step 2 may be a no-op; verify with `cargo tree -p yaml-rust2` before resolving.

**Step 3: Implement binding**

Implement:

- `validate_startup_request`;
- `resolve_project_binding`;
- `.memory-project.yaml` walking from canonicalized `cwd` upward;
- nearest git worktree root and `git remote get-url origin`;
- `proj_` + lowercase SHA-256 of normalized remote URL;
- stable namespace vector.

**Verification plan:**

- Primary command: `cargo test -p memoryd --test startup_recall_project_binding`
- Regression command: `cargo test -p memoryd --test startup_recall_determinism`
- Static command: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

### Task 8: Candidate Collection, Entity/Alias Resolution, Ranking

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 7
**Owned files:** `crates/memoryd/src/recall/candidates.rs`, `crates/memoryd/src/recall/entity.rs`, `crates/memoryd/src/recall/rank.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/tests/startup_recall_governance.rs`, `crates/memoryd/tests/startup_recall_ranking.rs`
**Invariants:** Candidate set sorted by memory id before scoring. No LLM/network/random ranking. No raw Markdown parsing bypassing Stream A. Startup steady state must not hydrate every active/pinned envelope; use Task 2's Stream A recall-index API for entity matching and ranking fields.
**Out of scope:** No protocol/MCP/CLI wiring.

**Files:**

- Create: `crates/memoryd/src/recall/candidates.rs`
- Create: `crates/memoryd/src/recall/entity.rs`
- Create: `crates/memoryd/src/recall/rank.rs`
- Modify: `crates/memoryd/src/recall/mod.rs`
- Test: `crates/memoryd/tests/startup_recall_governance.rs`
- Test: `crates/memoryd/tests/startup_recall_ranking.rs`

**Step 1: Write failing governance-filter tests**

In `startup_recall_governance.rs`, cover:

- active/pinned records recall;
- candidate/quarantined/tombstoned/superseded/archived records do not recall as facts;
- `retrieval_policy.passive_recall = false` suppresses recall;
- `requires_user_confirmation`, `human_review_required`, and pending `review_state` suppress facts but can affect pending-attention counts.

Run:

```bash
cargo test -p memoryd --test startup_recall_governance
```

Expected: compile failure or failing assertions before implementation.

**Step 2: Write failing ranking tests**

In `startup_recall_ranking.rs`, cover:

- ranking formula weights from spec §8.2;
- ranking uses fields returned by Stream A recall-index rows, not per-candidate envelope hydration;
- tie-breakers: higher score, pinned before active, newer `updated_at`, lexicographic id;
- pre-shuffled candidates produce identical output;
- budget exhaustion produces stable omissions;
- alias-collision shape (spec §3.3 / §7, introduced in v0.4 and inherited by v0.5): an alias resolving to two or more entity ids in the same namespace emits exactly one `RecallOmission` per `(section, alias)` collision with `reason = AmbiguousAlias`, `alias = Some(<surface form>)`, `colliding_ids` containing every matched entity id sorted lexicographically, and `id = None`. The same alias colliding in two sections produces two separate omissions, one per section.

Run:

```bash
cargo test -p memoryd --test startup_recall_ranking
```

Expected: compile failure or failing assertions.

**Step 3: Implement collection**

Use Stream A:

- Task 2's recall-index API with `namespace_prefix`, `status`/statuses, `passive_recall_only`, `updated_since`, and optional entity/tag/alias match terms.
- `query_memory` remains available for simple ID/summary list behavior but is not sufficient for Stream E ranking.
- `read_memory_envelope` only for a bounded selected set after ranking when rendering needs safe plaintext snippets or fields not projected by the recall-index row. Tests and perf review must prove this is not one envelope read per active/pinned candidate.
- `query_chunks` only for delta/entity lookup terms.

Apply frontmatter filters before ranking:

- status active/pinned only;
- passive recall enabled;
- no unresolved review/confirmation;
- namespace visible;
- sensitivity compatible with max scope;
- encrypted records metadata-only.

If Task 2's recall-index API is missing any field required by this list or by spec §8.2 ranking, stop and file a Stream A API gap instead of falling back to blanket envelope hydration.

**Step 4: Implement entity resolution**

Normalize with NFKC only if a dependency already exists; otherwise document v0.4 ASCII case-folding + whitespace collapse. Treat hyphen/underscore/slash/space as equivalent. Ignore matches shorter than 3 alphanumeric chars unless exact entity id.

For alias collisions per spec §7 (v0.4 shape): when one alias surface form resolves to two or more entity ids in the same namespace within a section, emit exactly one `RecallOmission` for that `(section, alias)` pair with `reason = AmbiguousAlias`, `alias = Some(<surface form>)`, `colliding_ids = <every matched entity id, sorted lexicographically>`, and `id = None`. Do **not** emit one omission per colliding id. When the same alias collides in multiple sections, emit one omission per section so each section's explanation is self-contained.

**Step 5: Implement ranking**

Implement exact status/scope/entity/recency/confidence/source weights and tie-breakers.

**Verification plan:**

- Primary commands:
  - `cargo test -p memoryd --test startup_recall_governance`
  - `cargo test -p memoryd --test startup_recall_ranking`
- Static command: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

### Task 9: Review Gate B — Recall Core Correctness Review

**Subagent type:** `reviewer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Tasks 6, 7, 8 and all Phase 2 fixes
**Owned files:** `docs/reviews/stream-e-recall-core-correctness-review.md`
**Invariants:** Read-only review except report. No fixes in this task.
**Out of scope:** Protocol/MCP/CLI review, because those are not wired yet.

**Files:**

- Create: `docs/reviews/stream-e-recall-core-correctness-review.md`

**Review checklist:**

- DTOs exactly match v0.5 serialized shape.
- Budget estimator and UTF-8 truncation are deterministic.
- Project binding rejects malformed configs.
- Candidate collection relies on Stream A APIs and avoids raw Markdown scans for facts.
- Candidate collection and ranking rely on Stream A recall-index projections, with envelope reads bounded to selected/rendered memories rather than every active/pinned candidate.
- Entity/ranking tests exercise tie-breakers and stable omitted metadata.
- All review findings are labeled P0/P1/P2 with file paths and test evidence.

**Verification plan:**

- Run:
  - `cargo test -p memoryd --test startup_recall_determinism`
  - `cargo test -p memoryd --test startup_recall_project_binding`
  - `cargo test -p memoryd --test startup_recall_governance`
  - `cargo test -p memoryd --test startup_recall_ranking`

---

### Task 10: Daemon Protocol, Handler Wiring, Status Counters

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 9 and all Review Gate B fixes
**Owned files:** `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/server.rs`, `crates/memoryd/src/recall/counters.rs`, `crates/memoryd/src/recall/startup.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/tests/startup_recall_mcp.rs`, `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd/tests/server_smoke.rs`
**Invariants:** `memory_startup` must not return `not_implemented` except non-null `since_event_id`. Counters reset on daemon restart and are present on every `Status` response. No substrate doctor/fsck scan runs in the startup hot path. The new public `memory_privacy::safe_plaintext_fragment` (Task 3) cannot be imported into `handlers.rs` until the existing private free function of the same name is renamed; this task owns that rename so importing the Stream D helper does not silently shadow or collide with the legacy `bool`-returning helper.
**Out of scope:** CLI subcommands and MCP manifest shape are Task 11.

**Files:**

- Modify: `crates/memoryd/src/protocol.rs`
- Modify: `crates/memoryd/src/handlers.rs`
- Modify: `crates/memoryd/src/server.rs`
- Create: `crates/memoryd/src/recall/counters.rs`
- Create: `crates/memoryd/src/recall/startup.rs`
- Modify: `crates/memoryd/src/recall/mod.rs`
- Test: `crates/memoryd/tests/startup_recall_mcp.rs`
- Update existing tests: `crates/memoryd/tests/protocol_contract.rs`, `crates/memoryd/tests/server_smoke.rs`

**Step 0: Rename the existing private `safe_plaintext_fragment` helper before importing the new public Stream D helper**

`crates/memoryd/src/handlers.rs:1553` currently defines `fn safe_plaintext_fragment(text: &str) -> bool` — a private free function with seven call sites (lines 168, 1253, 1261, 1338, 1362, 1548, 1577 at plan time, but verify with `rg -n "safe_plaintext_fragment" crates/memoryd/src/handlers.rs` before editing because line numbers may drift across earlier task integrations). This name will collide with the new public `memory_privacy::safe_plaintext_fragment(classifier, &str) -> SafeFragmentDecision` that Task 3 ships, and the two functions have incompatible return types so naïve import would either fail to compile or shadow the wrong helper.

Required transformation, behavior-preserving:

1. Rename the private function to `is_safe_plaintext_for_indexing` (descriptive of its `bool` predicate purpose, disambiguated from the Stream D enum-returning helper).
2. Update every call site in `handlers.rs` (use `rg -n "safe_plaintext_fragment" crates/memoryd/src/handlers.rs` to enumerate; verify the count matches the function-definition count plus the call-site count before and after the rename).
3. Run the existing handlers tests to confirm no behavior change; this rename must be a pure rename, not a logic change.
4. Only after the rename lands does this task add `use memory_privacy::safe_plaintext_fragment;` (or equivalent qualified path) to `handlers.rs` for the new Stream D helper.

This step is owned by Task 10 because Task 10 already owns `handlers.rs` and is the integration point where the new helper is wired. Tasks 3 and 6 may not touch `handlers.rs`.

**Step 1: Write failing protocol/handler tests**

Create or update tests for:

- `RequestPayload::Startup { cwd, session_id, harness, harness_version, include_recent, since_event_id, budget_tokens }`;
- `ResponsePayload::Startup(StartupResponse)`;
- validation rejects missing/relative cwd, empty session id, empty harness, invalid budget;
- validation order is deterministic: syntactic field presence/trim checks, cwd absolute/canonicalization, session_id, harness, harness_version, budget, then `since_event_id`; tests cover a multi-error request so the returned code does not drift;
- non-null `since_event_id` returns `not_implemented`;
- success includes `session_binding`, `recall_block`, `budget_used_tokens`, `recall_explanation`, `guidance`;
- `StatusResponse.recall` counters start at zero, increment startup success, and increment startup failure map by code on invalid request.
- invalid `namespace_prefix` propagated from Stream A `invalid_query` maps to daemon `invalid_request`.
- `StatusResponse` remains JSON-additive: old status JSON without `recall` deserializes with zero/default counters while newly serialized status always includes `recall`.
- a regression test asserts that the public `memory_privacy::safe_plaintext_fragment` is the helper now imported into `handlers.rs` (e.g., a doc-comment lint or a focused test that exercises a fragment Stream D would refuse) and that the legacy `is_safe_plaintext_for_indexing` predicate still gates its seven original indexing call sites with unchanged behavior.

Run:

```bash
cargo test -p memoryd --test startup_recall_mcp
```

Expected: compile failure because protocol variants are missing.

**Step 2: Add protocol DTOs**

Prefer defining recall DTOs in `memoryd::recall::types` and re-exporting into protocol as needed. Ensure serde output matches v0.3 examples.

Before integration, run `rg -n "StatusResponse\s*\{" crates/memoryd crates -S` (the `\s*` accommodates both `StatusResponse {` and `::StatusResponse {` literal forms) and update every constructor/test helper to set `recall` explicitly or through a zero-counter constructor. At plan time the known sites are `crates/memoryd/src/server.rs::healthy_status()`, `crates/memoryd/src/handlers.rs::status_response()`, and any test fixtures; verify the rg output matches before editing because earlier task integrations may have added or removed sites.

**Step 3: Wire handler**

Add `RequestPayload::Startup` dispatch to:

- validate/bind request;
- rely on typed errors from Stream A query/read operations for readiness; map index/IO failures to `substrate_error` or `recall_unavailable`;
- do **not** call `doctor_response`, Stream A fsck, or any full-index repair scan inside the request hot path;
- assemble startup block through `recall::startup`;
- map errors to stable protocol codes;
- update counters.

**Step 4: Add counters**

Use a small in-process counter state. If `handlers::handle_request` currently receives only `&Substrate`, introduce a minimal shared server state if necessary; keep standalone `healthy_status()` able to emit zero counters. Avoid globals unless a review explicitly accepts them.

**Verification plan:**

- Primary command: `cargo test -p memoryd --test startup_recall_mcp`
- Regression commands:
  - `cargo test -p memoryd --test protocol_contract`
  - `cargo test -p memoryd --test server_smoke`
- Static command: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

### Task 11: MCP Manifest, MCP Forwarding, CLI Startup/Delta Commands

**Subagent type:** `heavy_worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 10
**Owned files:** `crates/memoryd/src/mcp.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/recall/delta.rs`, `crates/memoryd/src/recall/mod.rs`, `crates/memoryd/tests/mcp_forward.rs`, `crates/memoryd/tests/mcp_governance_forward.rs`, `crates/memoryd/tests/mcp_manifest.rs`, `crates/memoryd/tests/recall_cli.rs`
**Invariants:** CLI recall commands print only recall XML to stdout on success. Diagnostics go to stderr. `delta-block` no-match emits `<memory-delta empty="true" />`.
**Out of scope:** Do not alter governance write/reveal MCP behavior except expected startup test updates.

**Files:**

- Modify: `crates/memoryd/src/mcp.rs`
- Modify: `crates/memoryd/src/cli.rs`
- Modify: `crates/memoryd/src/main.rs`
- Create: `crates/memoryd/src/recall/delta.rs`
- Modify: `crates/memoryd/src/recall/mod.rs`
- Update tests: `crates/memoryd/tests/mcp_forward.rs`
- Update tests: `crates/memoryd/tests/mcp_governance_forward.rs`
- Update tests: `crates/memoryd/tests/mcp_manifest.rs`
- Test: `crates/memoryd/tests/recall_cli.rs`

**Step 1: Write failing MCP tests**

Update `mcp_forward` and `mcp_governance_forward` to remove old "startup short-circuits with not_implemented" assertions. Add tests that `memory_startup` forwards to daemon with required fields.

Run:

```bash
cargo test -p memoryd --test mcp_forward --test mcp_governance_forward --test mcp_manifest
```

Expected: fail while `StartupRequest` still has only `include_recent`.

**Step 2: Write failing CLI tests**

Create `crates/memoryd/tests/recall_cli.rs`:

- `memoryd recall startup-block ...` prints exactly one `<memory-recall>` block to stdout;
- `memoryd recall delta-block ... --message "no match"` prints `<memory-delta empty="true" />`;
- CLI errors map to exit codes 1/2/3/4 and keep diagnostics on stderr.
- CLI startup and delta commands route through the running daemon socket by default and update the same in-process recall counters surfaced by `StatusResponse.recall`.
- if no daemon socket is available, the command fails quickly with a typed `recall_unavailable`/exit 2 diagnostic on stderr; no transparent direct-substrate fallback in v0.3 hook mode.

Run:

```bash
cargo test -p memoryd --test recall_cli
```

Expected: compile/failure because `recall` subcommand is absent.

**Step 3: Implement MCP schema**

Replace `StartupRequest` with v0.3 required shape:

- `cwd`
- `session_id`
- `harness`
- optional `harness_version`
- default `include_recent = true`
- optional/null `since_event_id`
- optional `budget_tokens`

Update manifest input/output schemas.

**Step 4: Implement CLI**

Add:

- `memoryd recall startup-block --repo --runtime --cwd --session-id --harness --harness-version --budget-tokens`;
- `memoryd recall delta-block --repo --runtime --cwd --session-id --harness --message --budget-tokens`;
- default CLI execution connects to the daemon socket under `--runtime` and sends the same daemon protocol requests as MCP, so hook invocations are counted in `StatusResponse.recall`;
- no direct-substrate fallback for hook commands in Stream E v0.3. If a future offline/debug subcommand is added, it must be explicit in the CLI name or flag and documented as not contributing to daemon counters.

**Step 5: Implement delta assembly**

Implement request-local delta:

- seed entity matching from startup seeds plus message tokens/quoted phrases/exact memory ids;
- use `query_chunks` only for terms;
- on no match print exactly `<memory-delta empty="true" />`;
- default budget 400;
- no facts from excluded lifecycle statuses.

**Step 6: Add delta counter acceptance**

After delta wiring exists, add tests that exercise the daemon path and assert:

- `delta_invoked_total` increments on a successful delta request;
- `delta_failed_total{code}` increments on an invalid delta request;
- `budget_exhausted_total{section}` increments when startup or delta rendering omits content due to budget exhaustion;
- maps use stable snake_case section/error keys.

**Verification plan:**

- Primary commands:
  - `cargo test -p memoryd --test recall_cli`
  - `cargo test -p memoryd --test mcp_forward --test mcp_governance_forward --test mcp_manifest`
- Manual smoke after tests requires a running daemon at `--runtime .memoryd`. Start it in another shell first (`cargo run -p memoryd -- start --runtime .memoryd` or whatever the shipped start subcommand is at the time of this task) before issuing: `cargo run -p memoryd -- recall delta-block --repo . --runtime .memoryd --cwd "$PWD" --session-id smoke --harness codex --message "definitely-no-match" --budget-tokens 512`. If the daemon is not running, the CLI must fail fast with `recall_unavailable` (exit 2) on stderr per Task 11 invariants — that failure is itself a valid smoke result, just not the success path.
- Static command: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

### Task 12: Review Gate C — Protocol/MCP/CLI Review

**Subagent type:** `reviewer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Review Gate C1
**Blocked by:** Tasks 10, 11
**Owned files:** `docs/reviews/stream-e-protocol-cli-review.md`
**Invariants:** Read-only review except report.
**Out of scope:** Do not fix code in this task.

**Files:**

- Create: `docs/reviews/stream-e-protocol-cli-review.md`

**Review checklist:**

- MCP legacy shape is removed.
- `since_event_id` is the only remaining startup `not_implemented` path.
- CLI stdout is parseable XML only on success.
- Exit codes match spec.
- `StatusResponse.recall` is additive and does not break old tests except expected struct updates.
- CLI recall commands use daemon protocol by default so hook invocations increment daemon counters; there is no silent direct-substrate counter bypass.
- No debug logs leak to stdout.

**Verification plan:**

- Run:
  - `cargo test -p memoryd --test startup_recall_mcp`
  - `cargo test -p memoryd --test recall_cli`
  - `cargo test -p memoryd --test mcp_forward --test mcp_governance_forward --test mcp_manifest`

---

### Task 13: Review Gate C — Security/Privacy Review

**Subagent type:** `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 14 and all Task 14 privacy fixes
**Owned files:** `docs/reviews/stream-e-security-privacy-review.md`
**Invariants:** Read-only review except report.
**Out of scope:** Do not fix code in this task.

**Files:**

- Create: `docs/reviews/stream-e-security-privacy-review.md`

**Review checklist:**

- Review every implementation change made under Task 14, not just the pre-existing recall surface; Task 14 intentionally lands privacy acceptance fixes before this review so the auditor has concrete code and tests to inspect.
- `memory_startup` and delta never call `memory_reveal`.
- `MemoryContent::Ciphertext` bytes are never rendered.
- Candidate/quarantine claim bodies are not emitted.
- Error messages do not echo secret-like CLI/MCP input unsafely.
- `safe_plaintext_fragment` is applied to generated explanation prose and diagnostics.
- Encrypted metadata fields that classify unsafe are omitted with the correct reason.

**Verification plan:**

- Run:
  - `cargo test -p memoryd --test startup_recall_privacy`
  - `cargo test -p memory-privacy --test safe_plaintext_fragment`
  - `cargo test -p memoryd --test recall_cli`

---

### Task 14: Privacy, Encrypted-Memory, And Pending-Attention Acceptance

**Subagent type:** `test_hardener`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 12 and all Review Gate C1 fixes
**Owned files:** `crates/memoryd/tests/startup_recall_privacy.rs`, `crates/memoryd/src/recall/candidates.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/src/recall/startup.rs`
**Invariants:** No decrypted or ciphertext body bytes in recall. Candidate/quarantine can only appear as counts/ids where allowed.
**Out of scope:** Do not change Stream D classifier semantics.

**Files:**

- Test: `crates/memoryd/tests/startup_recall_privacy.rs`
- Modify only if tests expose gaps:
  - `crates/memoryd/src/recall/candidates.rs`
  - `crates/memoryd/src/recall/render.rs`
  - `crates/memoryd/src/recall/startup.rs`

**Step 1: Write failing privacy acceptance tests**

Cover:

- encrypted records are descriptor-findable but never body-recalled;
- `memory_startup` does not reveal ciphertext;
- candidate/quarantined encrypted review items affect pending-attention counts without leaking claim text;
- unsafe metadata fragments are omitted and explained as `review_pending` or `encrypted_body_hidden`.

Run:

```bash
cargo test -p memoryd --test startup_recall_privacy
```

Expected: fail until gaps are fixed.

**Step 2: Fix only the failing privacy gaps**

Keep changes scoped to recall candidate/render/startup modules.

**Verification plan:**

- Primary command: `cargo test -p memoryd --test startup_recall_privacy`
- Regression commands:
  - `cargo test -p memoryd --test startup_recall_governance`
  - `cargo test -p memoryd --test startup_recall_determinism`

---

### Task 15: Full Startup Recall Acceptance And Output Shape

**Subagent type:** `test_hardener`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 13 and all Review Gate C2 fixes
**Owned files:** `crates/memoryd/tests/startup_recall_mcp.rs`, `crates/memoryd/tests/startup_recall_determinism.rs`, `crates/memoryd/src/recall/startup.rs`, `crates/memoryd/src/recall/render.rs`, `crates/memoryd/src/recall/types.rs`
**Invariants:** Byte-identical output under fixed clock/repo/request. Always-on section order.
**Out of scope:** No CLI changes.

**Files:**

- Update: `crates/memoryd/tests/startup_recall_mcp.rs`
- Update: `crates/memoryd/tests/startup_recall_determinism.rs`
- Modify only if tests expose gaps:
  - `crates/memoryd/src/recall/startup.rs`
  - `crates/memoryd/src/recall/render.rs`
  - `crates/memoryd/src/recall/types.rs`

**Step 1: Add end-to-end startup output tests**

Cover:

- response serialized shape exactly includes `startup`;
- recall block top-level XML attributes include version/harness/session;
- section tags are always present and ordered;
- summaries capped at 240 UTF-8 bytes and snippets at 360;
- budget includes wrapper tags;
- explanation metadata has accurate selected IDs, matched entities, omitted counts, and `omitted_truncated_count`.

Run:

```bash
cargo test -p memoryd --test startup_recall_mcp --test startup_recall_determinism
```

Expected: fail if implementation is missing output details.

**Step 2: Fix only output-shape gaps**

Keep rendering logic deterministic; do not introduce clocks except through a fixtureable time source.

**Verification plan:**

- Primary command: `cargo test -p memoryd --test startup_recall_mcp --test startup_recall_determinism`
- Regression command: `cargo test -p memoryd --test startup_recall_ranking`

---

### Task 16: Performance Probe And Release-Gate Fixture

**Subagent type:** `performance_engineer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Phase 4B after Task 14 if file ownership remains disjoint from Task 15
**Blocked by:** Task 13 and all Review Gate C2 fixes
**Owned files:** `crates/memory-test-support/src/perf.rs`, `crates/memoryd/src/bin/stream_e_recall_bench.rs`, `crates/memoryd/Cargo.toml`, `scripts/stream-e-recall-bench.sh`, `bench/stream-e-recall-results.darwin-arm64.json`, `docs/reviews/stream-e-performance-review.md`
**Invariants:** Bench fixture must be deterministic and record all spec-required fields. Do not weaken existing Stream A bench gates.
**Out of scope:** No ranking changes unless perf data proves a bottleneck and review approves.

**Files:**

- Modify: `crates/memory-test-support/src/perf.rs`
- Create: `crates/memoryd/src/bin/stream_e_recall_bench.rs`
- Modify: `crates/memoryd/Cargo.toml`
- Create: `scripts/stream-e-recall-bench.sh`
- Create/update: `bench/stream-e-recall-results.darwin-arm64.json`
- Create: `docs/reviews/stream-e-performance-review.md`

**Step 1: Write failing perf smoke**

Add a small smoke test or bench mode for:

- 200 memories startup;
- 1,000 memories startup warm;
- 1,000 memories delta no-match;
- 1,000 memories delta five-entity match.

Run:

```bash
cargo run -p memoryd --bin stream_e_recall_bench -- --sizes 200,1000 --warm-runs 3 --smoke
```

Expected: fail before bench binary exists.

**Step 2: Implement deterministic fixture**

Use `memory-test-support` to build real Stream A memories with:

- active/pinned/candidate/quarantined/tombstoned/superseded statuses;
- encrypted metadata-only count;
- passive recall disabled subset;
- project/user/agent namespace distribution;
- entity aliases and deliberate ambiguous collisions.

**Step 3: Record metrics**

Bench JSON must include:

- memory count;
- encrypted metadata-only count;
- candidate/quarantine count;
- hardware profile;
- budget tokens;
- selected memory count;
- omitted memory count;
- cold/warm flag;
- p95 latency.

`scripts/stream-e-recall-bench.sh --release` must enforce the spec §13 caps and exit non-zero on any violation: startup warm 200 <= 80ms, startup warm 1,000 <= 250ms, cold-start 1,000 <= 600ms, delta no-match <= 60ms, and delta five-entity match <= 120ms. The smoke mode may use smaller/looser local sanity thresholds, but release mode is a real gate.

**Verification plan:**

- Primary command: `bash scripts/stream-e-recall-bench.sh --smoke`
- Required release command: `bash scripts/stream-e-recall-bench.sh --release`
- Static command: `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`

---

### Task 17: Review Gate D — Performance Review

**Subagent type:** `performance_engineer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Review Gate D
**Blocked by:** Task 16
**Owned files:** `docs/reviews/stream-e-performance-review.md`
**Invariants:** Review may append findings to the performance review doc; code fixes happen in follow-up fix subagents.
**Out of scope:** Do not edit implementation in this task.

**Files:**

- Update: `docs/reviews/stream-e-performance-review.md`

**Review checklist:**

- Startup warm p95 targets: 200 <= 80ms, 1,000 <= 250ms.
- Cold-start 1,000 <= 600ms.
- Delta no-match <= 60ms, five-entity match <= 120ms.
- Candidate collection does not hydrate every active memory in steady state and the bench/review can show envelope reads are bounded to selected/rendered memories.
- Bench records all required metadata.
- `bash scripts/stream-e-recall-bench.sh --release` fails non-zero when any spec p95 cap is exceeded.

**Verification plan:**

- Run: `bash scripts/stream-e-recall-bench.sh --smoke`
- If smoke passes, run: `bash scripts/stream-e-recall-bench.sh --release`

---

### Task 18: Review Gate D — API Contract Review

**Subagent type:** `reviewer`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Review Gate D
**Blocked by:** Tasks 14, 15, 16
**Owned files:** `docs/reviews/stream-e-api-contract-review.md`
**Invariants:** Read-only review except report.
**Out of scope:** Do not fix code.

**Files:**

- Create: `docs/reviews/stream-e-api-contract-review.md`

**Review checklist:**

- Rust DTOs, serde names, MCP schemas, CLI examples, and docs agree.
- `RecallSectionExplanation.omitted_count` and `RecallExplanation.omitted_truncated_count` match the spec.
- `RecallOmission.alias` (`skip_serializing_if = "Option::is_none"`) and `RecallOmission.colliding_ids` (`default`, `skip_serializing_if = "Vec::is_empty"`) are present in the DTO; non-collision omissions serialize without those keys; v0.3-shape omissions (no `alias`, no `colliding_ids`) still deserialize against the v0.4 DTO so the wire change is additive for tolerant clients.
- `StatusResponse.recall` is always present.
- Old `Status` JSON without `recall` deserializes into zero/default recall counters so the wire change is additive for tolerant clients.
- All pre-v0.5 policy/version strings (`stream-e-v0.3`, `stream-e-v0.4`) have been replaced with `stream-e-v0.5` in code, manifests, recall-block attributes, and docs. No legacy version string remains in any non-historical location.
- Error codes and retryable flags match the spec.
- Exit codes match CLI errors.

**Verification plan:**

- Run:
  - `cargo test -p memoryd --test protocol_contract`
  - `cargo test -p memoryd --test mcp_manifest`
  - `cargo test -p memoryd --test recall_cli`

---

### Task 19: Review Gate D — Final Security Review

**Subagent type:** `security_auditor`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** yes, Review Gate D
**Blocked by:** Tasks 14, 15, 16
**Owned files:** `docs/reviews/stream-e-final-security-review.md`
**Invariants:** Read-only review except report.
**Out of scope:** Do not fix code.

**Files:**

- Create: `docs/reviews/stream-e-final-security-review.md`

**Review checklist:**

- No hidden persistence layer.
- No decryption/reveal in recall.
- No secret-like diagnostics to stdout/stderr.
- CWD validation and project config failure modes are typed and safe.
- XML escaping prevents malformed block injection.
- Candidate/quarantine content cannot leak through explanation metadata.

**Verification plan:**

- Run:
  - `cargo test -p memoryd --test startup_recall_privacy`
  - `cargo test -p memoryd --test recall_cli`
  - `cargo test -p memory-privacy --test safe_plaintext_fragment`

---

### Task 20: API Docs, README, CLAUDE Updates

**Subagent type:** `worker`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Tasks 17, 18, 19 and all Review Gate D fixes
**Owned files:** `docs/api/stream-e-passive-recall-api.md`, `docs/api/stream-a-public-api.md`, `docs/api/stream-d-privacy-api.md`, `README.md`, `CLAUDE.md`
**Invariants:** Docs must describe shipped behavior, not planned behavior. Keep Stream C/D history accurate.
**Out of scope:** No production code changes.

**Files:**

- Create: `docs/api/stream-e-passive-recall-api.md`
- Update: `docs/api/stream-a-public-api.md`
- Update: `docs/api/stream-d-privacy-api.md`
- Update: `README.md`
- Update: `CLAUDE.md`

**Step 1: Write API docs**

Document:

- MCP `memory_startup` request/response examples;
- daemon protocol variant;
- CLI startup/delta examples and exit codes;
- CLI recall commands require a running daemon socket in Stream E v0.3 and contribute to daemon in-process counters;
- recall XML shape;
- explanation DTOs;
- privacy constraints;
- status counters.

**Step 2: Update repo-level docs**

Update README and CLAUDE from "Stream E not implemented" to "Stream E shipped" only after tests pass. If any acceptance remains incomplete, say exactly what is incomplete instead.

**Verification plan:**

- Primary command: `pnpm exec oxfmt --check docs/api/stream-e-passive-recall-api.md docs/api/stream-a-public-api.md docs/api/stream-d-privacy-api.md README.md CLAUDE.md`
- Fallback if oxfmt ignores docs paths due local ignore rules: run `rg -n "memory_startup|Stream E|not_implemented|safe_plaintext_fragment|MemoryQuery|recall" docs/api README.md CLAUDE.md` and manually verify no stale claims.

---

### Task 21: Final Gate, Boundary Check, And Completion Report

**Subagent type:** `test_hardener`
**Skills:** Mandatory skills: clean-code, tdd, rust-engineer.
**Parallel:** no
**Blocked by:** Task 20
**Owned files:** `docs/reviews/stream-e-final-gate-report.md`
**Invariants:** Do not edit implementation unless a gate fails; if it fails, spawn a scoped fix subagent and rerun the failed gate.
**Out of scope:** No opportunistic cleanup.

**Files:**

- Create: `docs/reviews/stream-e-final-gate-report.md`

**Final gate commands:**

Run in order:

```bash
cargo fmt --all -- --check
cargo test -p memory-substrate --test memory_query_extension
cargo test -p memory-privacy --test safe_plaintext_fragment
cargo test -p memoryd --test startup_recall_mcp
cargo test -p memoryd --test startup_recall_privacy
cargo test -p memoryd --test startup_recall_governance
cargo test -p memoryd --test startup_recall_ranking
cargo test -p memoryd --test startup_recall_determinism
cargo test -p memoryd --test startup_recall_project_binding
cargo test -p memoryd --test recall_cli
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
./scripts/rust-boundary-check.sh
pnpm exec oxfmt --check .
pnpm exec oxlint .
bash scripts/check.sh
git diff --check
```

Performance gate:

```bash
bash scripts/stream-e-recall-bench.sh --smoke
bash scripts/stream-e-recall-bench.sh --release
```

`--release` is required to call Stream E shipped. It must fail non-zero on any spec §13 p95 violation. If Trey explicitly skips it for wall-clock reasons, the completion report must say Stream E is not release-certified yet.

**Completion report must include:**

- commit/branch if committed;
- exact gate command outputs or summarized pass/fail with log path;
- any accepted deferrals that exactly match spec §15;
- reviewer findings and fix status;
- whether Stream E can be considered shipped.

---

## Review/Fix Policy

After each review gate:

1. Orchestrator reads every review report.
2. For every P0/P1 finding, spawn a fix subagent with the same mandatory skills and owned files limited to the affected implementation/test/doc files.
3. Rerun the narrow test that would have caught the issue.
4. Rerun the review subagent if the fix touches security, privacy, protocol shape, or indexing semantics.
5. Do not proceed to the next phase until all P0/P1 findings are fixed or Trey explicitly accepts a deferral.

P2 findings may be batched into a cleanup subagent before final gates, but do not let P2 cleanup expand scope.

## Non-goals And Deferrals To Enforce

Subagents must not implement:

- persistent recall-count or last-recalled mutation;
- live peer presence, claim locks, or subscriptions;
- embeddings/LLM/network ranking beyond existing Stream A query APIs;
- LLM summarization/compression;
- automatic hook installation across all harnesses;
- Stream F dream-question surfacing;
- dashboard visualizations.

If a worker believes one of these is required to pass tests, stop and file a spec/plan mismatch in the task report instead of adding it.
