# Stream A public API

`memory-substrate` exposes `Substrate` as the sole mutation boundary for repo files, event logs, and the derived SQLite index.

Key calls:

- `Substrate::init`, `Substrate::open`, `Substrate::adopt_clone`
- `write_memory`, `write_encrypted`, `tombstone_memory`
- `read_memory`, `read_path`, `query_memory`, `query_recall_index`, `query_chunks`
- `next_memory_id`, `reindex`, `durability_tier`
- `git_preflight`, `fetch_inspect`, `auto_commit`, `fetch_and_merge`, `push`
- `watch`

Every plaintext and encrypted write request carries an explicit `ClassificationOutcome`. `Secret` is refused before disk effects; `RequiresEncryption` is refused on the plaintext path; `Trusted` is accepted only for public/internal frontmatter sensitivity.

`WriteOutcome` preserves committed-state semantics so callers do not retry a canonical file write after any error with `committed = true`.

## Stream F noncanonical repo files

Stream A accepts Stream F's repo-synced file families as valid tree files, but they are not canonical memories:

- `substrate/<device_id>/<YYYY-MM-DD>.jsonl`
- `substrate/archive/<device_id>/<YYYY-MM>.jsonl`
- `encrypted/substrate/<device_id>/<YYYY-MM-DD>.jsonl`
- `dreams/journal/<scope_path>/<YYYY-MM-DD>.md`
- `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl`
- `dreams/cleanup/<device_id>/<YYYY-MM-DD>.json`
- `leases/journal.lease`

These paths use dedicated tree validators for path shape and JSON/JSONL well-formedness. They do not use canonical-memory frontmatter parsing. In particular, frontmatter-free `dreams/journal/**.md` files validate as Stream F dream output but are excluded from the canonical Markdown walker.

The merge-driver public surface remains `memory_substrate::merge::merge_markdown(MergeInput)`, because the git merge-driver binary already passes the repo path through `MergeInput::path`. Stream F path families route before canonical Markdown parsing:

- `substrate/**/*.jsonl` and `encrypted/substrate/**/*.jsonl`: append-only JSONL merge, de-duplicated by canonical row bytes and sorted by `id`.
- `dreams/questions/**/*.jsonl` and `leases/journal.lease`: append-only JSONL merge, de-duplicated by canonical row bytes and sorted by `(scope, ts, id)`. Legacy rows without `id` use `run_id` where present and otherwise a canonical-row fallback for deterministic ordering.
- `dreams/journal/**/*.md`: one-sided edits use normal last-writer-wins fast paths; contested same-scope same-date writes produce a quarantine/diagnostic marker preserving both sides for operator choice.
- `dreams/cleanup/**/*.json`: JSON-object last-writer-wins by `(device_id, date)`, using report timestamps when present and a deterministic canonical-row fallback when not.

`Substrate::read_path_envelope(&RepoPath)` returns `ReadError::NotACanonicalMemory { path }` for these valid-but-noncanonical paths before attempting frontmatter parsing. `Substrate::read_memory_envelope(&MemoryId)` remains ID-based and resolves only canonical memories.

`query_memory`, `query_recall_index`, and `query_chunks` are restricted to canonical memory index rows; Stream F dream/substrate/lease files are never indexed by those APIs.

## Query API

`MemoryQuery` remains default-compatible with the original Stream A behavior: `MemoryQuery::default()` returns all non-metadata-only indexed memories ordered by memory id. The Stream E extension adds index-side filters, and Stream G consumes the same covering index pattern for observability without adding a second persistence layer:

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

Filter semantics:

- `status` matches the indexed `memories.status` value.
- `updated_since` is inclusive (`updated_at >= updated_since`).
- `passive_recall_only` matches the indexed `memories.passive_recall = 1` projection.
- `namespace_prefix` is synthetic and stable:
  - `me` maps to `scope = "user"`;
  - `agent` maps to `scope = "agent"`;
  - `project:<canonical_id>` maps to `scope = "project"` and `canonical_namespace_id = <canonical_id>`;
  - `org:<canonical_id>` maps to `scope = "org"` and `canonical_namespace_id = <canonical_id>`.

Invalid namespace prefixes return `SubstrateError::InvalidQuery { field: "namespace_prefix", ... }`. The rendered message contains the stable code `invalid_query` so the daemon can map the error to `invalid_request`.

The SQLite query builder emits only active predicates for supplied filters. It does not use `(? IS NULL OR ...)` clauses.

## Recall index API

Stream E uses `Substrate::query_recall_index(RecallIndexQuery)` to collect ranking and entity-matching candidates without hydrating every active memory envelope and without calling `read_memory_envelope`.

```rust
pub struct RecallIndexQuery {
    pub namespace_prefix: Option<String>,
    pub statuses: Vec<MemoryStatus>,
    pub passive_recall_only: bool,
    pub updated_since: Option<DateTime<Utc>>,
    pub match_terms: Vec<String>,
}
```

`RecallIndexRow` is projected from the SQLite index plus existing auxiliary tables:

- `memories`: id, path, summary, status, scope, canonical namespace id, updated_at, confidence, source kind, sensitivity, `passive_recall`, `index_body`, `requires_user_confirmation`, `review_state`, `human_review_required`, and `max_scope`;
- `memory_tags`: deterministic tag list;
- `memory_aliases`: deterministic memory alias list;
- `memory_entities` and `memory_entity_aliases`: deterministic entity list, with entity aliases embedded in each `Entity`.

`match_terms` match existing index projections only: entity id, entity label, entity alias, memory alias, and tag. Rows are returned sorted by memory id so Stream E scoring starts from deterministic input.

## Index schema v2

Index schema version 2 adds two real `memories` columns:

- `passive_recall INTEGER NOT NULL DEFAULT 1`
- `index_body INTEGER NOT NULL DEFAULT 1`

Both are populated on every upsert and reindex from `frontmatter.retrieval_policy`. Stream E recall queries read these columns directly; hot-path recall does not extract `retrieval_policy` from `frontmatter_json`.

Opening an existing v1 index runs a single transaction that:

1. checks `PRAGMA table_info(memories)` independently for `passive_recall` and `index_body`;
2. adds any missing column with `ALTER TABLE`;
3. backfills `passive_recall` and `index_body` from `frontmatter_json` exactly during the migration;
4. creates supporting indexes on `(status, passive_recall, updated_at)` and `(scope, canonical_namespace_id, status, passive_recall, updated_at DESC)`;
5. records schema version 2 only after the DDL and backfills succeed.

Reopening an already-upgraded index is idempotent and does not rerun `ALTER TABLE`.

## Index schema v3

Index schema version 3 adds two real `memories` columns required by Stream E governance filtering:

- `human_review_required INTEGER NOT NULL DEFAULT 0`
- `max_scope TEXT NOT NULL DEFAULT 'agent'`

Both are populated on every upsert and reindex from `frontmatter.write_policy.human_review_required` and `frontmatter.retrieval_policy.max_scope`. `RecallIndexRow` reads these columns directly; hot-path recall does not extract governance policy from `frontmatter_json`.

Opening an existing v1/v2 index runs the v3 migration in a transaction that:

1. checks `PRAGMA table_info(memories)` independently for `human_review_required` and `max_scope`;
2. adds any missing column with `ALTER TABLE`;
3. backfills `human_review_required` and `max_scope` from `frontmatter_json` exactly during the migration;
4. records schema version 3 only after the DDL and backfills succeed.

## Stream G / Stream I substrate additions (schema v4)

Stream A remains the canonical substrate and index surface. Stream G observability and Stream I cross-session coordination add only additive surfaces here.

### EventKind additions

`memory_substrate::events::EventKind` includes five new serde-compatible variants using the existing adjacent tag shape (`{"kind":"...","data":{...}}`):

- `RecallHit { id, recalled_at }`
- `RealityCheckConfirmed { id, session_id }`
- `RealityCheckForgotten { id, session_id, reason }`
- `RealityCheckNotRelevant { id, session_id }`
- `ClaimLockContention { memory_id, holder, contender }`

Existing variants are unchanged. Stream E owns future `RecallHit` emission from recall rendering; Stream I owns future `ClaimLockContention` emission from claim-lock handling.

### `events_log` SQLite mirror

Canonical events remain the per-device JSONL logs under `events/<device_id>.jsonl`. Schema v4 adds a rebuildable SQLite mirror for SQL consumers:

```sql
CREATE TABLE IF NOT EXISTS events_log (
  event_id      TEXT PRIMARY KEY,
  device        TEXT NOT NULL,
  seq           INTEGER NOT NULL,
  kind          TEXT NOT NULL,
  memory_id     TEXT,
  ts            TEXT NOT NULL,
  payload_json  TEXT NOT NULL CHECK (json_valid(payload_json))
);
CREATE INDEX IF NOT EXISTS idx_events_log_kind_memory_ts
  ON events_log(kind, memory_id, ts);
```

`idx_events_log_kind_memory_ts` is the covering index for Stream G's recall-history and drift-risk reads: `RecallHit` count over 30 days, total recall count, and `MAX(ts)` for last-recalled can be answered from `(kind, memory_id, ts)` without hydrating memory files or scanning full JSONL. Stream G is an authorized consumer of this derived projection; JSONL remains canonical.

`event_id` is the mirror identity because `seq` is only monotonic within a device log. The mirror also stores `device` so rebuilding from multiple `events/<device_id>.jsonl` files preserves all events even when devices share sequence numbers. `Substrate` write paths append JSONL first. After the canonical append succeeds, Stream A best-effort mirrors the same event into SQLite. A mirror-write failure is logged and does not roll back JSONL. `Substrate::doctor_reindex_events_log()` rebuilds the mirror from JSONL, and `Substrate::events_log_mirror_health()` returns `EventsLogMirrorHealth { jsonl_max_seq, sqlite_max_seq, lag, jsonl_count, sqlite_count, missing_count }` so daemon doctor code can surface stale mirrors, including missing middle rows when max sequence still matches.

`Substrate::record_event_best_effort(kind)` and `Substrate::record_recall_hit(id)` expose a best-effort observability append that still uses Stream A's central sequence allocator and incremental mirror hook. Recall rendering uses this API for `RecallHit` events and does not rebuild the full mirror on the recall hot path.

Current architecture note: `open_index(path)` does not know the repository events directory, so migration v4 creates the table/index but repository-level JSONL backfill runs through `Substrate` open/reindex helpers, where both repo and runtime roots are available.

### Supersession projection

Schema v4 adds `memory_supersession(memory_id, supersedes_id)` as a derived projection from `Frontmatter.supersedes`:

```sql
CREATE TABLE IF NOT EXISTS memory_supersession (
  memory_id     TEXT NOT NULL,
  supersedes_id TEXT NOT NULL,
  PRIMARY KEY(memory_id, supersedes_id),
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE,
  FOREIGN KEY(supersedes_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_memory_supersession_supersedes_id
  ON memory_supersession(supersedes_id);
```

`Index::upsert_memory` replaces a memory's supersession edges wholesale from frontmatter, matching the tags/aliases/entities/evidence projection pattern. Frontmatter remains canonical; this table is rebuildable.

### Frontmatter and memories projection

`Frontmatter` adds `original_confidence: Option<f64>` with serde default and skip-when-none behavior. The `memories` table adds nullable `original_confidence REAL`, populated from the frontmatter field on upsert and backfilled from `frontmatter_json` during schema v4 migration when present.

### RecallIndexRow additions

`RecallIndexRow` now exposes:

- `indexed_at: DateTime<Utc>` from `memories.indexed_at` (typed RFC3339 parse; no epoch fallback).
- `source_device: Option<String>` from `memories.source_device`.

No new columns are introduced for these fields; both already existed in the Stream A `memories` table.
