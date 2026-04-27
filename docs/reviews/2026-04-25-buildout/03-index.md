# Clean-code review — index (sqlite + fts + vector)

Reviewer: reviewer-index
Files reviewed:

- `crates/memory-substrate/src/index/mod.rs`
- `crates/memory-substrate/src/index/sqlite_vec.rs`
- `crates/memory-substrate/src/index/schema.rs`
- `crates/memory-substrate/src/index/migrations.rs`
- `crates/memory-substrate/src/index/chunking.rs`
- `crates/memory-substrate/src/index/vector.rs`
- `crates/memory-substrate/src/index/query.rs`

Anchored on spec `docs/specs/stream-a-core-substrate-v1.1.md` §10.1–10.6, §11.4, §15.

## Blockers

### B1. Default active embedding triple is a hardcoded `synthetic / stream-a-test / 32` — silent fallback when caller drops the configured triple

`index/query.rs:344-346` defines:

```rust
fn default_active_embedding() -> EmbeddingTriple {
    EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "stream-a-test".to_string(), dimension: 32 }
}
```

This is used by:

- `Index::new` (line 21): any caller that constructs `Index` without `with_active_embedding` silently inherits the synthetic triple.
- The free function `pub fn upsert_memory` (line 287): bypasses whatever the substrate configured and unconditionally enqueues pending-jobs against `synthetic / stream-a-test / 32`.

This violates spec §10.2.2 #5: _"No silent fallback. Stream A never embeds against a different model than the caller asked for."_ The active triple is supposed to come from `config.yaml` (§10.2.2 #2; §15.1). A test-fixture default living in production code is the textbook silent-fallback path the spec explicitly forbids. It is also a foot-gun for any future call-site that forgets the second constructor.

**Fix:** delete `default_active_embedding`. `Index::new` should require an explicit triple, or — if `Index::new` is purely a test convenience — gate it behind `#[cfg(test)]` and route all production construction through `with_active_embedding`. The free `pub fn upsert_memory` (line 286) is dead-code as far as I can find (callers route through `Index::upsert_memory`); delete it rather than letting it diverge.

### B2. `pending_embedding_jobs` schema is missing `content_hash`, breaking the §10.2.1 stale-job reconciliation contract

Spec §10.1 declares:

```
CREATE TABLE pending_embedding_jobs (
    chunk_id TEXT PRIMARY KEY ...,
    provider TEXT NOT NULL, model_ref TEXT NOT NULL, dimension INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    enqueued_at TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
```

`index/schema.rs:35-41` has:

```sql
CREATE TABLE IF NOT EXISTS pending_embedding_jobs(
  chunk_id TEXT NOT NULL,
  provider TEXT NOT NULL, model_ref TEXT NOT NULL, dimension INTEGER NOT NULL,
  PRIMARY KEY(chunk_id, provider, model_ref, dimension)
);
```

No `content_hash`, no `enqueued_at`, no `attempts`, no `last_error`. Spec §10.2.1 #6 third bullet requires:

> `pending_embedding_jobs` rows whose chunks no longer exist or **whose `content_hash` no longer matches** are dropped with `VectorReconciled`.

The current implementation cannot satisfy that bullet because the column does not exist. A worker draining a job will compute against a stale chunk*id row whose body has since changed; `update_embedding`'s `expected_chunk_hash` check (query.rs:138) catches that case at the \_update* site, but the spec requires the reconciliation pass to drop these jobs _before_ the worker wastes embedding compute on them. Today `reconcile_active_embedding_jobs` (line 357) only purges by chunk-id existence, not by hash.

`enqueued_at` and `attempts` are also load-bearing for retry/backoff in Stream B; their absence will be felt the moment an embedding worker exists.

**Fix:** add the four missing columns, populate `enqueued_at`/`content_hash` at insert sites (query.rs:329-337, vector.rs:43-49, query.rs:371-385), and extend the reconciliation pass to delete jobs whose `content_hash` does not match the current `memory_chunks.body_hash`.

### B3. `chunk_embedding_meta` is missing `embedded_at` and `vector_table`, schema-divergence from §10.1

Spec §10.1:

```
CREATE TABLE chunk_embedding_meta (
    chunk_id TEXT PRIMARY KEY REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE,
    provider TEXT NOT NULL, model_ref TEXT NOT NULL, dimension INTEGER NOT NULL,
    vector_table TEXT NOT NULL,
    embedded_at TEXT NOT NULL,
    content_hash TEXT NOT NULL
);
```

`index/schema.rs:51-58`:

```sql
CREATE TABLE IF NOT EXISTS chunk_embedding_meta(
  chunk_id TEXT NOT NULL,
  provider TEXT NOT NULL, model_ref TEXT NOT NULL, dimension INTEGER NOT NULL,
  chunk_hash TEXT NOT NULL,
  PRIMARY KEY(chunk_id, provider, model_ref, dimension)
);
```

Two issues:

1. The column rename `content_hash` → `chunk_hash` is harmless on its own but propagates: `update_embedding` (query.rs:169) writes `chunk_hash`, while `pending_embedding_jobs` doesn't have any hash column at all. Pick one name (the spec says `content_hash`) and use it everywhere — the inconsistency between `memory_chunks.body_hash`, `chunk_embedding_meta.chunk_hash`, and the model field `EmbeddingUpdate.expected_chunk_hash` makes it easy to confuse a per-memory body hash with a per-chunk content hash. They are different things and should not share the visual stem `_hash`.
2. `vector_table` and `embedded_at` are gone. `vector_table` is the contract handle that lets you locate the sqlite-vec virtual table for a given triple without recomputing `vector_table_name` (sqlite_vec.rs:34) on every read. `embedded_at` is required for Stream H eval reproducibility ("which embedding worker generation produced this vector?") and for Stream G's review UI.

**Fix:** add the two missing columns, populate them in `Index::update_embedding`. Rename `chunk_hash` → `content_hash` here and in `EmbeddingUpdate` for consistency with the spec. Adding `FOREIGN KEY(chunk_id) REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE` would also let the reconciliation pass simplify (today it manually purges `chunk_embedding_meta` rows whose `chunk_id` is gone).

### B4. `memories` table schema is dramatically narrower than the spec contract, dropping fields that gate query semantics

Spec §10.1's `memories` table has 27 columns including `schema_version`, `type`, `scope`, `namespace`, `canonical_namespace_id`, `confidence`, `trust_level`, `status`, `review_state`, `requires_user_confirmation`, `created_at`, `updated_at`, `observed_at`, `valid_from`, `valid_until`, `ttl`, `author`, `source_kind`, `source_harness`, `source_device`, `frontmatter_json`, `file_hash`, `file_mtime_ns`, `indexed_at`. `index/schema.rs:6-13` has only `id`, `path`, `summary`, `sensitivity`, `body_hash`, `metadata_only` — six columns.

Most consequential omissions:

- **`status`** (active/quarantined/tombstoned/...). Spec §10.4 hybrid query routing depends on filtering by status; Stream E will need it for "skip tombstoned." Today there is no way to write that filter.
- **`scope`, `canonical_namespace_id`, `type`** — three of the five compound-index keys (`idx_memories_scope_canon_status_sens_updated`) are not even columns. The query p95 acceptance signal (§10.6) cannot be measured against this schema.
- **`frontmatter_json`** — without it, `query_memory` cannot return a hydrated frontmatter for callers that need it; it has to re-read the file. That destroys the "SQLite is the read path" mental model.
- **`updated_at`** — needed for ordering by recency in every Stream E shape.
- **`requires_user_confirmation` / `review_state`** — Stream G's review UI cannot find pending memories without these.
- The five compound indexes from §10.1 are entirely absent (schema.rs has no `CREATE INDEX` at all on `memories`).

The spec is explicit that "SQLite is a derived projection with chunk-level search" (§10.1) and that the schema is the **stable contract** Stream B/E rely on. Shipping a six-column reduction is not a "stub" — it is a contract break. If this is intentionally Task-N-only scope, that should be noted in `mod.rs` so reviewers don't read it as the v1.0 schema.

**Fix:** either (a) align to the §10.1 schema in this task, or (b) put a `// SCAFFOLD: schema is task-N stub; full §10.1 schema lands in task M.` comment in `schema.rs:1` and update the plan to make the gap explicit. Right now the absence is silent and looks like an oversight.

### B5. Migrations runner has no schema-version gate — silent forward-compatibility violation

`index/migrations.rs:10-19`:

```rust
pub fn open_index(path: &Path) -> rusqlite::Result<Connection> {
    crate::index::sqlite_vec::register_extension();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(...) }?;
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.execute_batch(SCHEMA_SQL)?;
    Ok(connection)
}
```

The function unconditionally executes `SCHEMA_SQL` (which is all `IF NOT EXISTS`) and writes `INSERT OR IGNORE INTO schema_migrations(version) VALUES (1)`. Three problems:

1. **No upgrade detection.** If a future build with `schema_migrations.version = 2` opens a v1 database, `IF NOT EXISTS` silently leaves the v1 schema in place; the new build then runs against an old schema with no warning. Spec §15 (and the equivalent merge-driver gate at §14.2) requires explicit version comparison and refusal when _unsupported_. The merge driver got this right (`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` constant in `merge/mod.rs`); the index runner did not adopt the same pattern.
2. **No downgrade detection.** Open a v1 database with code that expects v2 — it just runs and corrupts state.
3. **`PRAGMA journal_mode = WAL`** from spec §10.1 is not set anywhere I can find. This is not a niche pragma — without WAL, every write blocks every read, which crushes the §10.6 query p95 target.

**Fix:** introduce `INDEX_SUPPORTED_SCHEMA_VERSION: u32 = 1` next to the merge constant, read `MAX(version)` from `schema_migrations` after the bootstrap insert, and refuse to open if it exceeds supported (typed `OpenError::SchemaVersionUnsupported`). Add `PRAGMA journal_mode = WAL` (and probably `PRAGMA synchronous = NORMAL` per WAL-on-SQLite best practice) to `open_index`. Note: WAL must be set _outside_ a transaction; `execute_batch(SCHEMA_SQL)` with its DDL is fine on a fresh connection but the WAL pragma should run before any DDL.

### B6. `update_embedding` writes the vector outside a transaction, then opens an `unchecked_transaction` for metadata — violating §10.2.1 step 4 ordering

`index/query.rs:153-186`:

The flow is:

1. `ensure_vector_table(...)` — DDL, autocommit (line 152).
2. `tx = self.connection.unchecked_transaction()` (line 155).
3. INSERT/REPLACE into the sqlite-vec virtual table inside `tx` (line 156-159).
4. INSERT into `chunk_vectors` inside `tx` (line 162).
5. INSERT into `chunk_embedding_meta` inside `tx` (line 168).
6. DELETE from `pending_embedding_jobs` inside `tx` (line 180).
7. `tx.commit()`.

Spec §10.2.1 third paragraph plus #4: _"`update_embedding` upserts the vector first, then in one SQLite transaction upserts `chunk_embedding_meta` and deletes the matching `pending_embedding_jobs` row."_

Two issues:

1. **The vector upsert is supposed to happen _before_ the SQLite transaction**, not inside it. The reason is in §10.2.1 paragraph 1: the vector store may not honor SQLite rollback. The implementation puts the vector upsert _inside_ `tx`, which means on rollback the SQLite metadata reverts but the vector remains — exactly the orphan that startup reconciliation later has to clean up. That is technically allowed by step 4 ("If SQLite commit fails after vector upsert, startup reconciliation sees an orphan"), but it makes the orphan path the _normal_ case rather than the exception, undermining the fast-path guarantees.
2. **`unchecked_transaction` defeats the borrow checker's reentrancy guarantee** — the function signature is `&self`, not `&mut self`, so two threads sharing this `Index` could each `unchecked_transaction()` on the same connection and observe undefined behavior. The other transactional method, `clear_memory_index`, takes `&mut self` and uses `self.connection.transaction()` correctly. Pick one discipline.

**Fix:** restructure to: (a) compute the vector blob, (b) execute the sqlite-vec INSERT outside any txn, (c) open a normal `transaction()` for the `chunk_vectors` + `chunk_embedding_meta` + `pending_embedding_jobs` writes. Take `&mut self` so the borrow checker prevents racing transactions. If the vector upsert succeeds and the SQLite txn fails, you've created exactly the orphan the spec expects reconciliation to handle.

## Risks

### R1. `chunk_id` derivation is robust against same-offset edits but loses information about position

`index/chunking.rs:46-55`:

```rust
fn chunk_id(memory_id: &str, start_byte: usize, chunk_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(memory_id.as_bytes());
    hasher.update(b"\0chunker-v1\0");
    hasher.update(start_byte.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(chunk_hash.as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("{memory_id}:{}", &digest[..16])
}
```

The spec (§10.3) says `chunk_id = chk_<sha256(memory_id || chunker_version || ordinal || chunk_hash)>`. The implementation uses `start_byte` instead of `ordinal`. That **does** satisfy the §10.3 acceptance ("Edits that change chunk text create new chunk IDs; stale embedding updates must fail by content hash") — same-offset edits change `chunk_hash`, different-offset edits change `start_byte`. So this is not a blocker.

But two minor concerns:

- The prefix `chk_` from the spec is replaced with `{memory_id}:` — that breaks the spec's stated identifier shape. It also makes chunk-ids non-uniform-length (memory IDs vary), which complicates downstream tooling that wants to recognize a chunk-id by its prefix.
- 16 hex chars (= 64 bits) of SHA-256 is fine for collision resistance within a single memory, but the spec says full `sha256(...)`. Truncating without justification is a needless deviation; a `chk_<full-hex>` schema is 71 chars, which is not a meaningful SQLite penalty.

**Recommendation:** match the spec format `chk_<full-sha256-hex>` and treat `chunker_version` as a string constant (`"chunker-v1"`) to preserve forward-compat when chunking strategy changes.

### R2. Chunking ignores body normalization required by spec §10.3 third bullet

`index/chunking.rs:24` does `let body = memory.body.replace("\r\n", "\n");` — but elsewhere the canonical body bytes hashed into `body_hash` come from `markdown::hash_bytes(memory.body.as_bytes())` (query.rs:308), which does **not** normalize CRLF. So:

- Two clones receive the same memory; one writes through Windows tooling that introduces CRLF; the body bytes hash differently → `body_hash` differs even though canonical content is identical.
- Even when both sides normalize: `chunk.body_hash` (chunking.rs:42) hashes the LF-normalized chunk text, while `memories.body_hash` (query.rs:308) hashes the raw body bytes. These two `body_hash` columns now mean different things. That is exactly the kind of column-name-collision-with-different-semantics that bites you a year out.

Per spec §10.3: _"Chunks include byte offsets into normalized LF body."_ The spec is clear that LF normalization is a property of the body the substrate stores, not just an in-memory transform inside chunking. Either (a) normalize once at write time and treat the LF-form as canonical (which is what the merge driver already does), or (b) make `chunk.body_hash` explicitly named `chunk_text_hash` and document that it's over the LF-normalized chunk text.

**Recommendation:** rename `Chunk::body_hash` → `Chunk::content_hash` (matches spec §10.1 wording) so the type system stops conflating it with `memories.body_hash`. Also document that the LF-normalization at chunking.rs:24 is a defense-in-depth measure and the canonical bytes were already normalized upstream.

### R3. `query_chunks` does not filter encrypted-memory chunks at the SQL boundary

`index/query.rs:72-86`:

```rust
pub fn query_chunks(&self, text: &str) -> rusqlite::Result<Vec<ChunkResult>> {
    let mut stmt = self.connection.prepare(
        "SELECT memory_chunks.memory_id, memory_chunks.text, bm25(memory_chunks_fts) AS score
         FROM memory_chunks_fts JOIN memory_chunks ...
         WHERE memory_chunks_fts MATCH ?1 ORDER BY score LIMIT 20")?;
```

Today this is _probably_ safe because:

- Encrypted memories without a safe projection have `metadata_only = true`, body cleared, `index_body = false` ⇒ no chunks inserted (query.rs:315 gate).
- Encrypted memories with a safe projection have chunks of the **safe** body, which is what should be returned.

So the absence of an explicit encrypted-namespace filter in this query is OK for the _current_ code path. The risk is fragility: if any future code path inserts a chunk for an encrypted memory without the spec-mandated safe projection, this query will silently surface ciphertext-derived content. Defense-in-depth suggests adding an explicit `AND memories.metadata_only = 0` join filter and a runtime assertion that no chunk's parent memory has `path LIKE 'encrypted/%' AND no_safe_projection_recorded`. The spec's invariant ("the original body never appears in any FTS or vector store row") is too important to leave to an implicit convention.

Same observation for `query_vector_chunks` (line 89) — no filter.

**Recommendation:** add an `AND memories.metadata_only = 0` join clause to both query paths. It costs one index lookup; the safety gain is nontrivial.

### R4. `clear_plaintext_memory_index` does not preserve `pending_embedding_jobs` for encrypted-projection chunks

`index/query.rs:50-69`:

```rust
transaction.execute(
    "DELETE FROM pending_embedding_jobs WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)",
    [],
)?;
```

After deleting plaintext memories' chunks (lines 53-57), this `DELETE` removes pending jobs whose chunks are gone — correct. But the function is named `clear_plaintext_memory_index` and the comment says "preserving encrypted metadata rows." That preservation is partial: encrypted memories _with safe projections_ still have rows in `memory_chunks` (their projection chunks), so their pending jobs survive. Encrypted memories with `metadata_only = true` have no chunks, so their absence of jobs is fine. The naming is just confusing — the function actually clears all chunks not in the encrypted namespace, plus orphaned vectors/jobs.

**Recommendation:** rename to something like `clear_indexed_state_for_plaintext_reindex` and add a one-line comment explaining that encrypted-projection chunks survive because their parent `memories` row survives. Or, better, restructure so the "preserve encrypted, drop plaintext" partition is a single visible predicate, not three near-identical SQL statements.

### R5. `is_dropped_triple` and `embedding_triple_is_dropped` are duplicated, with different error types

`index/query.rs:265` and `index/query.rs:348` are the same query against `dropped_embedding_triples`, just one returns `Result<bool, VectorError>` and the other returns `rusqlite::Result<bool>`. The duplication is small but invites drift — if the `dropped_embedding_triples` schema gains a column or the query needs a fix, you have to remember to update both.

**Recommendation:** keep one helper. Have callers needing `VectorError` map at the call site, or have the helper return `rusqlite::Result<bool>` (it's a primitive query) and let the typed error live where the typed error needs to live.

### R6. `VectorError::Storage(String)` swallows the typed `rusqlite::Error` — debugging will be painful

Roughly twenty call-sites (query.rs:108, 119, 151, 155, 160, 167, 179, 184, 185, 197, 203, 209, 215, 219, 232, 261, 273, 282, plus the same pattern in vector.rs and reconcile_active_embedding_jobs) all do:

```rust
.map_err(|err| VectorError::Storage(err.to_string()))?;
```

This converts a structured `rusqlite::Error` (which carries error codes, constraint names, lock states) into a flat `String`. When something fails in production:

- You can't pattern-match on `BUSY` vs `LOCKED` vs `CONSTRAINT` to retry intelligently.
- Stream B's `update_embedding` worker can't distinguish "transient lock, retry" from "schema violation, never retry."
- The error message loses the SQLite extended code that tells you _which_ constraint fired.

Compounding the issue: a SQLite lock error becomes `VectorError::Storage("database is locked")` — which a test author will then probably match by string substring, and that match silently breaks the day rusqlite changes the wording.

**Recommendation:** add `#[error(transparent)] Sqlite(#[from] rusqlite::Error)` to `VectorError`. The 20 call-sites collapse to `?`. Stream B gains the ability to retry on `SqliteFailure(_, BUSY)`. The `Storage(String)` variant can stay for non-rusqlite storage failures (e.g., the eventual external vector adapter).

### R7. `unchecked_transaction` plus `&self` is a soundness foot-gun

Already mentioned in B6, but worth its own bullet. `index/query.rs:155` and `index/query.rs:361`:

```rust
let tx = self.connection.unchecked_transaction()...
```

`unchecked_transaction` is rusqlite's escape-hatch for taking a transaction without `&mut`. It _will_ succeed if another thread is mid-transaction on the same connection, and the second writer's writes either get rolled back together with the first (if both call `commit`) or the SQLite-level locks deadlock. Both `update_embedding` and `reconcile_active_embedding_jobs` take `&self`, so they can be called concurrently from the same `Arc<Mutex<Index>>` only if all callers cooperate via the `Mutex`. Today they do (api.rs:540-571 always `lock()` first), but the type system isn't enforcing it. Future you will write a non-locking call-site and discover this bug at 3am.

**Recommendation:** make these methods take `&mut self` and use `self.connection.transaction()` (which requires `&mut Connection`). The `Arc<Mutex<Index>>` wrapper at the API layer (api.rs:540) already provides exclusive access, so this is purely a type-system change.

### R8. `validate_dimension` lives in `sqlite_vec.rs` but isn't sqlite-vec-specific

`index/sqlite_vec.rs:43` validates `vector.len() == triple.dimension`. That's pure embedding-triple semantics, not adapter-specific. Putting it in `sqlite_vec.rs` couples the rule to the adapter. Stream A's spec (§10.1 `VectorStore` trait, §10.2.2 #4) makes dimension validation an _adapter-agnostic_ contract: the moment a second adapter exists (sidecar SQLite, external store), this helper has to either move or be re-implemented.

**Recommendation:** move `validate_dimension` to `vector.rs` (or a new `index/embedding.rs`). `sqlite_vec.rs` should hold only what is genuinely adapter-specific: the `register_extension` + `vector_table_name` + `serialize_f32` triplet.

### R9. Schema mismatches between code and spec are not detected by the build

This is meta-process, not code: every blocker in this review (B2, B3, B4, B5) is a divergence from a spec table that I had to manually diff. The merge driver got a constant (`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`) that is the single source of truth. The index has nothing comparable — the schema lives in `SCHEMA_SQL: &str` with no machine-readable contract. A future Codex pass that "fixes" a schema field will have no signal that it should also update spec §10.1 (or vice versa).

**Recommendation (out of scope for this task, worth raising):** consider a sqlite-side `pragma_user_version` or a generated-from-Rust DDL approach so the spec/code drift is detectable. At minimum, link `schema.rs` to spec §10.1 with a doc comment and an assertion in tests that materializes the schema and diffs columns.

## Nits

### N1. `mod.rs` re-export inconsistency

`index/mod.rs:10-13`:

```rust
pub use chunking::{chunk_memory, Chunk};
pub use migrations::open_index;
pub use query::{upsert_memory, Index};                                    // re-exports a free fn that should be deleted (B1)
pub use vector::{reconcile_missing, reconcile_orphans, reconcile_pending_jobs, VectorStore};
```

`pub mod sqlite_vec` is also exposed — the only `pub mod` while the others are `mod`. That asymmetry suggests it's a public-API surface, but `sqlite_vec.rs` exports zero types that callers outside `index/` should reach for. Either downgrade to `mod sqlite_vec` and re-export the genuine public surface, or document why the asymmetry exists.

### N2. `Chunk::body_hash` is `Sha256Text` (a typed string), `memory_chunks.body_hash` is stored as `TEXT`

That's fine, but the trip through `chunk.body_hash.as_str()` (query.rs:322) discards the type wrapper. If a caller ever passes a _different_ hash format, the `Sha256Text` constraint at the type level is the only thing catching it — and that's bypassed at the SQL boundary. Consider a single helper `bind_sha256(&mut Statement, &Sha256Text)` that ensures the format prefix `sha256:` is present. (Inspection of the code shows `Sha256Text::new` does not validate format, so the type isn't really a guard today.)

### N3. Comments-as-code-smell

A few examples worth deleting:

- `chunking.rs:1` `//! Body chunking.` — module name says it.
- `chunking.rs:7-19` doc comments on every field of `Chunk` are noise (`/// Chunk id.` on `pub chunk_id`).
- `vector.rs:1` `//! Vector store reconciliation helpers.` — module name says it.
- `migrations.rs:1` `//! Migration runner.` — same.
- `index/mod.rs:1` `//! Derived SQLite index, chunk, and vector helpers.` is OK because it actually says what's _in_ the module.

The pattern is: short Markdown-style `//!` taglines that repeat the file name. Delete or expand.

### N4. `query_chunks` SQL has a hard-coded `LIMIT 20`

`index/query.rs:74` ends `LIMIT 20`. `query_vector_chunks` (line 93) takes a `limit: usize` parameter. Inconsistent. Stream E callers will need configurable limits on both. Either both should accept a limit, or neither should.

### N5. Magic `4096` for chunk byte-length

`index/chunking.rs:31`: `let end = next_chunk_boundary(&body, start, 4096);`. Spec §10.3 says "Target ~400 tokens, 80-token overlap" — there is no overlap here at all (line 36 `start = end`), and 4096 _bytes_ is a different unit than the spec's _tokens_. For ASCII English ~4 chars/token gives ~1024 tokens — 2.5× the spec target. Multibyte content makes it worse.

This is more than a nit because it means today's chunks are too coarse for the embedding model's context window in practice. But it is also clearly a "stub chunker, replace later" — the function is 13 lines. Flagging so it doesn't get forgotten.

### N6. `format!("vec_{}", hex::encode(...))` uses 16 bytes (32 hex chars) of SHA-256

`index/sqlite_vec.rs:39`. Fine for collision resistance among triples (you'd need ~2^64 distinct triples to expect a collision), but worth documenting the rationale next to the truncation. Also: SQLite identifier max length is 64 chars on default builds — `vec_` + 32 hex = 36 chars, plenty of headroom, no need to mention.

### N7. `serialize_f32` could just be a one-liner with `bytemuck`

`index/sqlite_vec.rs:52-58` builds a `Vec<u8>` of LE f32 bytes by hand. `bytemuck::cast_slice(&vector)` does the same with zero copies and is widely used. Optional; current code is correct and clear.

### N8. `pending_embedding_jobs` insert in `vector::reconcile_missing` swallows the typed error

`vector.rs:48`: `.map_err(|_| VectorError::UnknownEmbeddingTriple(triple.clone()))?` — converts _any_ SQL failure (constraint, busy, IO) to `UnknownEmbeddingTriple`. Misclassifying a constraint violation as "unknown triple" will send a Stream B operator on a wild goose chase looking for a missing model. The message lies.

**Fix:** propagate as `VectorError::Storage(err.to_string())` (or better, `Sqlite(rusqlite::Error)` per R6).

### N9. Doc comment lies on `clear_memory_index`

`index/query.rs:39`: `/// Clear derived memory/chunk rows before a full reindex.` But the function also clears `chunk_vectors` and `chunk_embedding_meta` (lines 44-45). The comment lists "memory/chunk rows" only. Either the comment is incomplete or the function is doing more than its name says. The spec calls this "full reindex," which would also drop `pending_embedding_jobs` — which this function does _not_ clear, leaving stale jobs that point at chunk_ids that no longer exist. (Verified: line 41-46, no `pending_embedding_jobs` delete.) That's a minor bug too: a full reindex should drop pending jobs, then re-enqueue from the upsert path. Today it leaves orphan jobs behind.

## Strengths worth keeping

- **Triple-as-identity is enforced at the right granularity.** `vector_table_name` in `sqlite_vec.rs:34` deterministically derives the table name from the full triple, and `dropped_embedding_triples` (schema.rs:59) is checked on every `update_embedding` and `query_vector_chunks` call before the table is touched. The shape is correct, even where the surrounding plumbing has issues.
- **`update_embedding`'s stale-hash check is in the right place** (query.rs:138) and uses the typed `VectorError::StaleChunk` shape from spec §10.2.1 #3 rather than swallowing it as `Storage`.
- **`replay_pending_repairs` _does_ verify the ciphertext hash before re-indexing encrypted content** (`runtime/reconcile.rs:165-172`), satisfying the critical spec invariant. The index code participates correctly via `Index::upsert_memory(&op.indexed_memory, op.metadata_only)`.
- **`is_char_boundary`-aware chunk truncation** in `chunking.rs:57-66` correctly handles multi-byte UTF-8. This is the kind of detail that's easy to get wrong.
- **`AUTOINCREMENT` on `memory_chunks.chunk_rowid`** (schema.rs:15) honors the spec §10.1 rationale comment about VACUUM rowid permutation. Good.
- **FTS triggers handle update by delete-then-insert** (schema.rs:31-34), which is the correct pattern for FTS5 external-content tables. The scaffold here is short but correct.

## Open questions for Trey

1. **Is the six-column `memories` table (B4) intentional Task-N scope, or is it a missed contract?** The plan's task list would tell us; I didn't read the plan during this review. If it's scoped, please add a `// SCAFFOLD:` comment in `schema.rs` so future readers don't flag it again. If it's not scoped, this is a contract break that should block release-cert.
2. **Is `Index::new` (with the synthetic default) intended for tests only?** If so, `#[cfg(test)]` it. If production code is using it, B1 is a strict blocker.
3. **What's the planned timeline for the embedding-worker (Stream B) integration?** B2 (missing `content_hash` column) and B3 (missing `vector_table` / `embedded_at`) become hard blockers the moment Stream B's worker exists. If that integration is months out, the order can be: ship this Task-N as-is, schedule a v1.1 schema migration before B-worker lands. If that integration is imminent, fix now.
4. **Is the chunker (chunking.rs) considered "scaffold to be replaced"?** N5 (4096 bytes, no overlap, no markdown awareness) deviates substantially from spec §10.3. Worth knowing whether to flag it harder or let it ride.
5. **Does Codex's plan call out a `PRAGMA journal_mode = WAL` step, or did it land somewhere else?** I grep'd the index code and didn't find it. If it's set elsewhere (e.g., a connection pool init), B5 narrows to "missing schema-version gate" only.
