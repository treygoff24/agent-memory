# 2026-06-18 — Fix `memory_chunks.chunk_id` shared-membership crash

**Status:** Revised post-review — SUPERSEDED APPROACH below; implementing the spec-conformant fix in §0.
**Author:** Claude (Stream B owner; Stream A fix per CLAUDE.md "Stream A modules are fair game for fixes").
**Executor:** This Claude session, direct edits to `crates/memory-substrate`. Full `scripts/check.sh` + two-clone convergence on trunk after.

---

## 0. REVISION (post plan-reviewer) — spec-conformant fix supersedes §2/§4

Plan-reviewer found the original §2 premise false. The spec (`stream-a-core-substrate-v1.1.md:1148`) defines `chunk_id = chk_<sha256(memory_id || chunker_version || ordinal || chunk_hash)>`, with `chunk_id TEXT NOT NULL UNIQUE` **and** `UNIQUE(memory_id, ordinal)`. So `chunk_id` is globally unique *by construction* and cross-memory dedup is explicitly precluded by the spec. The crash is a **shipped deviation**: `chunk_id_from_text()` (chunking.rs:240) hashes the chunk text alone, dropping `memory_id`/`chunker_version`/`ordinal` (its docstring even miscites "spec §10.3" and falsely claims merge-relevance).

**Fix = conform to the spec** (no schema redesign, no FK change, no vec0 fan-out, no FTS rebuild — three of the original blockers evaporate):

1. **Derivation (`index/chunking.rs`).** Add `CHUNKER_VERSION` constant. Replace `chunk_id_from_text(text)` with `chunk_id_from_parts(memory_id, chunker_version, ordinal, chunk_hash)`. Assign `chunk_id` in a final ordered pass inside `chunk_memory` (where `memory.frontmatter.id` and the ordinal are in scope); the per-strategy producers (`sections_to_chunks`, `byte_split_chunk`) stop setting `chunk_id`. `chunk_hash` = the existing per-chunk `body_hash`. Fix the docstrings.
2. **Migration (`index/migrations.rs`).** Bump `INDEX_SUPPORTED_SCHEMA_VERSION` 4→5. `migrate_v5` forces a full body reindex so every chunk_id recomputes — the reviewer confirmed reconciliation short-circuits on `memories.file_hash` (reconcile.rs:496-501 via query.rs:1779), so `migrate_v5` must invalidate it: `UPDATE memories SET file_hash = '<sentinel>'` (all rows — plaintext AND encrypted, since the encrypted sweep at api.rs:2562 also hash-compares). Per-memory reconciliation then clears+rechunks each memory; the `chunk_embedding_meta` FK cascade + existing orphan sweeps clean stale embedding rows; stranded old `vec0` rows are hidden by INNER joins (pre-existing leak, out of scope, noted). No table rebuild needed — the table structure is unchanged.
3. **No schema constraint/FK change.** `chunk_id` stays `UNIQUE` (correct under spec derivation); `chunk_embedding_meta`'s FK stays valid. The original §4.1/§4.2/§4.3/§4.4 (drop UNIQUE, fan-out, drain dedup, sweep additions) are NOT done.

Spec conformance ⇒ **no spec version bump** (this returns code to the contract). Trey's `~/memorum` install takes the fresh-rebuild path (its index file was deleted), so it never runs migrate_v5; the migration is for any install still on a populated v4 index.

**Tests:** two distinct memories containing identical chunk text both index (distinct chunk_ids) and are both FTS+vector findable; one memory repeating identical text across two ordinals indexes (distinct chunk_ids); update the existing `chunking.rs` unit tests that asserted text-only content-addressing (`identical_text_produces_identical_chunk_id`, the `chunk_id_from_text` callers); a migrate_v5 upgrade test asserting non-zero chunk/FTS counts and new-form chunk_ids after open.

The sections below (§1–§8) are the ORIGINAL pre-review plan, kept for history. §1 (the bug) and §3 (the two-id structural fact) remain accurate; §2 and §4 are superseded by §0.

---

## 1. The bug

`memory_chunks.chunk_id` is declared `TEXT NOT NULL UNIQUE` (globally unique), but `chunk_id` is **content-addressed** — `chunk_id_from_text()` (chunking.rs:240) hashes the chunk text alone. Each `memory_chunks` row binds one chunk to one `memory_id`, and per-memory reindex does a plain `INSERT` (query.rs:1651). So when **two different memories contain an identical text chunk**, or **one memory repeats a chunk**, the second insert hits `UNIQUE constraint failed: memory_chunks.chunk_id`, startup reconciliation aborts with `OperatorRepairRequired`, and the daemon refuses to open.

Reproduced on the live `~/memorum` substrate (207 imported memories): a fresh rebuild from canonical files crashes identically, so the duplicate is in the source corpus, not stale index state. Canonical data (Markdown + events JSONL) is intact; only the derived SQLite index is affected, and it is rebuildable.

## 2. Decision (made — encode, do not reopen)

Content-addressing is deliberate: the embedding-dedup tables (`chunk_vectors`, `chunk_embedding_meta`, `pending_embedding_jobs`) all key on `chunk_id` so an identical chunk is **embedded once**. We preserve that. The fix makes `memory_chunks` a per-`(memory, chunk)` membership table where the same content-addressed `chunk_id` may appear in multiple rows. Embedding **compute** stays deduped by `chunk_id`; embedding **storage** (the sqlite-vec `vec0` table, keyed by `chunk_rowid`) fans out to every rowid that shares the chunk so all memories are KNN-findable.

## 3. Key structural fact

`memory_chunks` carries two ids: `chunk_rowid INTEGER PRIMARY KEY AUTOINCREMENT` (one per row → one per `(memory, chunk)`) and `chunk_id TEXT` (content hash, now many-per-table). **FTS5 and the `vec0` KNN table are addressed by `chunk_rowid`; the dedup tables are addressed by `chunk_id`.** Every bug site is exactly where code crosses `chunk_id → a single chunk_rowid / single row`, because that mapping stops being 1:1.

## 4. Changes

### 4.1 Schema (`index/schema.rs`)
- `memory_chunks`: change `chunk_id TEXT NOT NULL UNIQUE` → `chunk_id TEXT NOT NULL`, and add a table-level `UNIQUE(memory_id, chunk_id)`. Keep `chunk_rowid` PK, the `memory_id` FK, and the three FTS triggers (they fire on `chunk_rowid`, unaffected).
- `chunk_embedding_meta`: **drop** `FOREIGN KEY(chunk_id) REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE`. SQLite requires an FK parent column to be UNIQUE/PK; dropping the `memory_chunks.chunk_id` UNIQUE makes this FK an error. Its lifecycle becomes orphan-sweep-driven (4.3), matching `chunk_vectors`/`pending_embedding_jobs` — which is also the *correct* shared-chunk semantics (the embedding survives while any memory references the chunk_id; per-row cascade would wrongly delete it when one of several sharers is removed).
- Bump the `INSERT OR IGNORE INTO schema_migrations` floor as needed; see 4.5.

### 4.2 Vector write fan-out (`index/query.rs`)
- Replace `read_chunk_rowid(conn, chunk_id) -> i64` (query.rs:1424, `query_row`, picks an arbitrary rowid) with `read_chunk_rowids(conn, chunk_id) -> Vec<i64>` (all rows sharing the chunk_id).
- `upsert_vector_payload` (1435): write the `vec0` `INSERT OR REPLACE INTO {table}(rowid, embedding)` for **every** returned rowid; write the `chunk_vectors` shadow row once (chunk_id-keyed, unchanged). Call sites: `update_embedding` (228/232) and `update_embeddings_batch` (276/278).
- Result: a chunk shared by N memories gets its single computed embedding written to all N rowids → KNN finds every memory. Without this, only one memory is vector-searchable (silent recall miss — the highest-risk site in the audit).

### 4.3 Orphan sweeps (`index/query.rs`)
- Add `chunk_embedding_meta` to the `chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)` sweep wherever `chunk_vectors`/`pending_embedding_jobs` are swept (query.rs:210-214 in `remove_*`, and the reconcile sweep ~1825-1836). Set-membership subqueries are duplicate-safe. This replaces the dropped FK cascade.

### 4.4 Drain query fan-out (`index/query.rs` ~441 + `crates/memoryd` worker)
- The pending-jobs drain `JOIN memory_chunks mc ON mc.chunk_id = pj.chunk_id` now fans out to N rows per chunk_id → the same job is yielded N times → the worker embeds the same text N times. Add `GROUP BY pj.chunk_id` (or `SELECT DISTINCT` on the projected columns) so each chunk_id drains **once** per batch, preserving the `LIMIT` budget. The single computed result then fans out to all rowids via 4.2. Confirm `crates/memoryd/src/embedding/worker.rs` needs no change beyond receiving deduped jobs.

### 4.5 Migration (`index/migrations.rs`)
- Bump `INDEX_SUPPORTED_SCHEMA_VERSION` 4 → 5. Add `migrate_v5` that rebuilds `memory_chunks` to the new constraint and rebuilds `chunk_embedding_meta` without the FK. SQLite cannot ALTER a UNIQUE/constraint, so use the table-rebuild idiom (CREATE `*_new` with new schema → `INSERT … SELECT` copy → drop old → rename) inside the migration transaction. The old data cannot contain duplicate chunk_ids (the old UNIQUE enforced it), so the copy is safe; FTS shadow stays consistent because triggers fire on the copied rows' rowids — **verify** whether the rebuild must also rebuild `memory_chunks_fts` (external-content FTS tied to `chunk_rowid`); if rowids change on copy, `INSERT INTO memory_chunks_fts(memory_chunks_fts) VALUES('rebuild')` after.
- **Trey's install is the trivial case**: its index file is already deleted, so the fresh `open_index` builds `memory_chunks` directly from the corrected `SCHEMA_SQL` (no migration path taken) and the substrate's reconciliation repopulates from the 207 canonical files. The migration covers other installs sitting on a v4 index.

## 5. Open questions for plan-reviewer

1. **Reconciliation repopulation.** After a fresh/empty `memory_chunks`, does `Substrate::open` reconciliation actually rechunk every canonical memory, or does it short-circuit on `memories.file_hash` matching disk and leave chunks empty? If the latter, `migrate_v5` (or the open path) must also invalidate `memories.indexed_at`/force a body reindex. **This is the load-bearing question for whether the rebuild actually works.**
2. **`vec0` rebuild on migrate_v5.** If `chunk_rowid`s are preserved by the copy (explicit `chunk_rowid` column in the `INSERT … SELECT`), existing `vec0`/FTS stay valid. Confirm the copy preserves `chunk_rowid` so we don't strand embeddings.
3. **`validate_update_preconditions` / `vector.rs` scalar reads** (query.rs:1407, vector.rs:75, query.rs:1831 correlated subquery): `SELECT body_hash … WHERE chunk_id=?1` now matches multiple rows. `body_hash` is invariant across rows of a chunk_id (both derived from the same text), so the value is benign, but add `LIMIT 1` + an invariant comment so it is explicit and a future body_hash divergence can't silently pick a row.
4. Is there any path that treats `(SELECT COUNT(*) FROM memory_chunks)` or `vector_count` as "unique chunks"? Confirm counts that should be per-chunk_id use `COUNT(DISTINCT chunk_id)`.

## 6. Tests (add to `crates/memory-substrate/tests`)
- Two distinct memories containing an identical text chunk: both index without error; both are FTS-findable; both are vector/KNN-findable (the fan-out); exactly one embedding computed for the shared chunk (`chunk_vectors`/`chunk_embedding_meta` has one row for that chunk_id).
- One memory repeating an identical chunk twice indexes without error.
- Deleting one of two memories sharing a chunk leaves the other's chunk row + the shared embedding intact (orphan sweep only removes when the last referer is gone).
- `migrate_v5` upgrade test: build a v4 index with data, open, assert the new constraint holds and no embeddings are stranded.

## 7. Gate
- `cargo test -p memory-substrate` and `-p memoryd` during the loop.
- `bash scripts/check.sh` on trunk (fmt, oxfmt, oxlint, clippy -D warnings, full nextest, specgate, two-clone convergence, bench regression) before commit. Two-clone convergence is canonical-content equality (invariant 6); the index change is derived-only, so convergence should be unaffected — but it must be run to confirm.

## 8. Out of scope
- Re-keying `vec0` by `chunk_id` (a deeper architectural change). The fan-out-to-rowids approach preserves the current `chunk_rowid`-addressed `vec0` design with minimal blast radius.
- Any change to the canonical on-disk format, events, or merge driver.
