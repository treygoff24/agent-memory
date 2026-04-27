# Stream A public API

`memory-substrate` exposes `Substrate` as the sole mutation boundary for repo files, event logs, and the derived SQLite index.

Key calls:

- `Substrate::init`, `Substrate::open`, `Substrate::adopt_clone`
- `write_memory`, `write_encrypted`, `tombstone_memory`
- `read_memory`, `read_path`, `query_memory`, `query_chunks`
- `next_memory_id`, `reindex`, `durability_tier`
- `git_preflight`, `fetch_inspect`, `auto_commit`, `fetch_and_merge`, `push`
- `watch`

Every plaintext and encrypted write request carries an explicit `ClassificationOutcome`. `Secret` is refused before disk effects; `RequiresEncryption` is refused on the plaintext path; `Trusted` is accepted only for public/internal frontmatter sensitivity.

`WriteOutcome` preserves committed-state semantics so callers do not retry a canonical file write after any error with `committed = true`.
