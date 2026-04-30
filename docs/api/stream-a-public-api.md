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

## Query API

`MemoryQuery` remains default-compatible with the original Stream A behavior: `MemoryQuery::default()` returns all non-metadata-only indexed memories ordered by memory id. The Stream E extension adds index-side filters:

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
