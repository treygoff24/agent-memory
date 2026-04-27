# Stream A — Codex overnight buildout review SUMMARY

**Date:** 2026-04-25
**Scope:** Clean-code review of Codex's overnight Stream A buildout (~13K LOC of Rust, all uncommitted on `main` beyond `ff708ef seed`).
**Method:** Six-reviewer agent team, each loaded with the `clean-code` skill and anchored on `docs/specs/stream-a-core-substrate-v1.1.md`.
**Per-reviewer files:** `01-frontmatter-ids-config-tree.md`, `02-markdown-events-git.md`, `03-index.md`, `04-merge.md`, `05-runtime-watcher-bench.md`, `06-public-api.md`.

This doc is the consolidated, fix-ordered punch list. The per-reviewer files have full prose for each finding; citations here are `(reviewer-N#X)` referring to those files.

---

## 1. Verdict

**Codex's "release-certification candidate" claim does not survive contact with the spec.** Aggregate findings across the six reviews:

- **~38 blockers** (spec violations, broken invariants, correctness bugs, silent corruption, unreachable code paths)
- **~50 risks** (smells that will bite under load, partial implementations, misleading naming, tests that pass for the wrong reason)
- **3 doc-only stub modules** declared in `merge/mod.rs` as if populated
- **6 of 9 phases** missing or wrong in `Substrate::open` startup reconciliation
- **2 separate** convergence-breaking bugs (merge non-commutativity + `.gitattributes` gaps) that make spec §13.6.1 canonical-content equality unreachable
- **1 silent data-corruption bug** in event-log recovery (UTF-8 lossy decode confuses byte offsets)
- **`docs/reviews/stream-a-final-review.md`** (Codex's self-graded "no blocking findings") is not credible

The implementation passes its own shallow tests but does not satisfy the spec contract Streams B–I will rely on.

---

## 2. Cross-cutting patterns

These patterns recur across multiple slices and should be addressed as cross-cutting fixes (Section 5) rather than per-blocker:

1. **Spec drift the tests can't catch.** Tests are mostly `text.contains(...)` substring assertions or shape checks against the buggy output, so structural divergences (`_merge_diagnostics` shape, schema columns, event-kind set, framing format) pass green. Adding fuzz/property tests is the right shape — substring assertions are not.

2. **Stubs declared as if populated.** Most flagrant: `merge/field_rules.rs`, `merge/lifecycle.rs`, `merge/quarantine.rs` are 1-line `//!` doc-only files declared as private modules in `merge/mod.rs`. `runtime::blocking::run_blocking` is dead code with zero call sites. Three module names imply organization that does not exist.

3. **Silent fallbacks where the spec mandates typed errors.** Hardcoded `synthetic / stream-a-test / 32` embedding triple as default, `Sensitivity::Ord` derived without a lock-in test, `Frontmatter::extras` marked `#[serde(skip)]`, no schema-version gate in index migrations, `repair_duplicate_ids` bypassing `seq.json`, `Substrate::open` auto-minting device IDs.

4. **String-bag error variants.** `WriteFailureKind::Validation(String)`, `WriteFailureKind::Io(String)`, `MergeError::Parse(String)`, `VectorError::Storage(String)` (used at ~20 call sites) — all swallow typed source errors. Callers cannot pattern-match for retry/repair.

5. **Two sources of truth for schema constants.** `frontmatter::schema::SUPPORTED_SCHEMA_VERSION` and `merge::MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION` both literal `1`, no link between them. Will drift on first bump.

6. **Code-spec name divergence on the merge driver itself.** Spec §13.1 says `memory-frontmatter-merge`; code uses `memory-merge-driver` consistently. Pick one; update spec or rename in code.

7. **`async fn` facade over `std::sync::Mutex` + blocking I/O.** Every public `Substrate` method is `async fn` but uses `std::sync::Mutex` and calls `std::fs`/`rusqlite`/`Command` directly. Zero `spawn_blocking` calls. Either be sync internally and document, or actually offload — current shape is neither.

---

## 3. Blockers by code area

Each blocker carries: file:line, the issue, the proposed fix shape. Group by area for fix locality.

### 3.1 Frontmatter / IDs / Config / Tree

**B-FT-1. `repair_duplicate_ids` violates spec §7.3 in five ways.** (`ids/repair.rs:13-49`, reviewer-frontmatter B1)

- Wrong survivor selection (filesystem walk order; spec mandates `(created_at, git_commit_ts, device_id, path)`).
- No reference rewriting (`supersedes`/`superseded_by`/`related`/evidence-id refs in OTHER files left silently broken).
- Bypasses `seq.json` (`next_unused_like` mints `old_id.sequence + 1` without touching the allocator). Acceptance signal §7.3.6 fails.
- Cross-shard corruption: keeps `prefix = mem_<date>_<old_shard>`, so the local device mints into another device's shard namespace.
- No `DuplicateIdRepaired` event emission, no reindex.
- **Fix:** rewrite. Sort candidates by spec key, mint via `next_memory_ids(runtime, device_id, &reserved, count)`, iterate every memory file to rewrite ID refs, emit events, reindex.

**B-FT-2. `repair_duplicate_ids` returns success while leaving repo half-mutated on validation failure.** (`ids/repair.rs:46-48`, reviewer-frontmatter B4)

- Calls `validate_tree(repo, FullySynced)` after renames; on `MissingReference` (which is the common case until B-FT-1 is fixed) returns `Err` after filesystem mutations with no rollback.
- **Fix:** stage writes, validate before commit, or document atomicity contract and acceptance-test it. Depends on B-FT-1.

**B-FT-3. `TreeValidationMode::StartupPreflight` is dead code identical to `FullySynced`.** (`tree/validate.rs:13-72`, reviewer-frontmatter B2)

- Spec §5.4: in `StartupPreflight` mode, validator must additionally check local git merge-driver config presence. Code only branches on `PartialSync` vs everything else. Acceptance signal §5.5 fails.
- **Fix:** when `mode == StartupPreflight`, call into `git::preflight` (or run `git config --get merge.<driver>.driver`) and surface a typed error if absent. Or — cleaner — remove `StartupPreflight` from this enum and let `Substrate::open` orchestrate the merge-driver check separately (covered in B-RT-1).

**B-FT-4. Tree validator skips most spec §5.4 checks.** (`tree/validate.rs:32-71`, reviewer-frontmatter B3)

- Missing: slug regex `[a-z0-9][a-z0-9-]{0,62}` validation, ISO date validation, plaintext-under-`encrypted/` detection, unknown top-level directory rejection.
- The `RepoPath::try_new` path validator exists in `model.rs:565-579` but the tree walker uses `RepoPath::new` (no validation) at `validate.rs:43-45`.
- **Fix:** route every walked path through `RepoPath::try_new`. Add explicit `is_under_encrypted_tier(path)` check that errors when plaintext `.md` parses successfully under `encrypted/`. (Open question: how do we tell ciphertext `.md` from plaintext `.md` under `encrypted/`? See Section 6.)

**B-FT-5. `validate_repo_relative_path` allow-list and `tree::layout::memory_dirs` disagree.** (`model.rs:565-579` vs `tree/layout.rs:6-34`, reviewer-frontmatter B5)

- `memory_dirs` lists 22 nested dirs under 11 top-level prefixes. `validate_repo_relative_path` lists 10 prefixes plus root files. Per spec §5.1, `substrate/`, `events/`, `tombstones/`, `policies/`, `leases/` are JSONL-only, never plaintext memory.
- **Fix:** narrow the memory-allowed prefixes to `me/`, `projects/`, `agent/`, `dreams/`, `encrypted/`. Reject `.md` files under JSONL-only tiers explicitly.

### 3.2 Markdown atomic IO / Events log / Git

**B-IO-1. Event-log recovery silently corrupts via UTF-8-lossy decoding.** (`events/recovery.rs:14-37`, reviewer-io-git B1, **silent data corruption**)

- `recover_event_log` reads bytes, calls `String::from_utf8_lossy`, sums `line.len()` over the lossy `String`, then `set_len`s the file at that offset.
- `U+FFFD` is 3 UTF-8 bytes. Any prior invalid UTF-8 byte makes the offset arithmetic wrong, silently truncating valid earlier events.
- No fsync after `set_len`. Crash-after-recovery undoes the truncation on some filesystems.
- **Fix:** iterate raw bytes, split on `b'\n'`, treat any non-UTF-8 line as malformed (route through `decode_line(std::str::from_utf8(line).ok()?)`). Track byte offset by accumulating successful slice lengths. Fsync file + parent dir after `set_len`.

**B-IO-2. Event framing diverges from spec §12.1 wholesale.** (`events/framing.rs`, `events/log.rs:14-25`, reviewer-io-git B3)

- Spec §12.1 mandates `{schema, id, ts, device, seq, kind, data, crc32c}` with CRC inside the JSON object.
- Implementation: `"{checksum:08x} {json}\n"` — out-of-band hex prefix. `Event` struct has only `id`, `operation_id`, `at`, `kind` — no `schema`, no `device`, no `seq`.
- Multi-device union display per §12.4 (`(ts, device, seq, id)` ordering) impossible.
- No 64-KiB line bound (spec §12.3 step 1).
- **Fix:** add `schema`, `device_id`, `seq` to `Event`. Either move `crc32c` into the JSON object OR draft a §12.1.1 spec amendment endorsing prefix framing (see Section 6). Persist `~/.memoryd/event-seq.json` under exclusive lock; bump after fsync. Add 64-KiB length check on append.

**B-IO-3. `.gitattributes` is missing JSONL-union rules and EOL normalization — breaks two-clone convergence.** (`tree/layout.rs:41`, reviewer-io-git B4)

- Code emits only `*.md merge=memory-merge-driver`.
- Spec §13.1 step 2 mandates `* text eol=lf`, `events/*.jsonl merge=union`, `substrate/**/*.jsonl merge=union`, `tombstones/*.jsonl merge=union`.
- Without `merge=union` on JSONL, JSONL merges fall to text driver, produce conflict markers. Without `eol=lf`, Windows checkouts break canonical-content equality.
- **Fix:** emit the full §13.1 step 2 block. Reconcile driver name (see cross-cutting #6).

**B-IO-4. `fetch_and_merge` violates spec §13.5 entirely.** (`git/sync.rs:14-18`, reviewer-io-git B7)

- Code: `git fetch && git merge --ff-only @{u}`.
- Spec §13.5 wants: preflight, `git fetch origin`, ahead/behind/diverged classification, `git merge --no-ff origin/main`, conflict-stop, quarantine scan + `MergeQuarantined` events, auto-commit reconciliation, `GitFetched` event.
- `--ff-only` converts the entire happy path of the merge driver workflow into a fatal error.
- **Fix:** implement the §13.5 protocol step-by-step. Wire in `git_preflight` (see B-IO-7).

**B-IO-5. `refuse_duplicate_device_logs` device extractor is wrong.** (`events/log.rs:88-110`, reviewer-io-git B2)

- Splits stem on `[' ', '.', '(']` — narrow heuristic for Finder-style copies; misses Linux `cp dev_abc.jsonl dev_abc-1.jsonl` (treats as new device); false-positives on legitimate peer logs that share a parenthesized prefix.
- Doesn't read `local-device.yaml` to know which id is THIS machine.
- **Fix:** parameterize on `local_device_id`. Refuse only when multiple files match `<local_device_id><suffix>.jsonl`; treat distinct device IDs as legitimate peer logs.

**B-IO-6. `auto_commit` (`git_add -A :/`) stages too broadly.** (`git/commit.rs:10`, reviewer-io-git B6)

- `-A` stages everything: stray temp files, mistakenly-placed `local-device.yaml`, half-written merge-driver outputs, etc. `.gitignore` covers `/.memoryd/` and `*.sqlite*` but not the atomic-write temp pattern (`.<basename>.<op_id>.tmp`).
- Combined with `Substrate::open` auto-minting device IDs and writing `local-device.yaml` (see B-API-7), can leak device id into synced commit if `roots.runtime` ever overlaps `roots.repo`.
- **Fix:** restrict staged paths to spec §5.1 namespaces explicitly. Validate `roots.runtime ⊄ roots.repo` at open. Add `/.*.tmp` (or stricter) to `.gitignore`.

**B-IO-7. Silent `git commit` failure.** (`git/init.rs:28`, `git/commit.rs:11`, reviewer-io-git B5)

- Both call sites use `let _ = run_git(repo, &["commit", ...])`. Intent is "no-op when index clean," but real failures (pre-commit reject, signing, locked index, missing user.email) are swallowed. `auto_commit` returns `Ok` and emits no `GitCommitted` event when commit didn't happen.
- **Fix:** check `git status --porcelain` first to distinguish "nothing to commit" from "real failure." Anything else propagates.

**B-IO-8. Suppression-ledger bookkeeping has lost-update windows.** (`markdown/atomic.rs:84-122`, reviewer-io-git B8)

- `if let Ok(mut ledger) = suppression.lock()` silently ignores poisoned mutexes — skips suppression insert, then watcher reingests our own write.
- On error after rename but before fsync_dir, `ledger.remove(&relative)` strips suppression while the file is on disk, causing watcher to see a real notify with no suppression entry.
- **Fix:** propagate poisoning (`expect()` is honest here). On rename-then-fsync-dir-failure path, leave the in-flight suppression entry to expire via TTL rather than removing it.

### 3.3 Index (SQLite + FTS + vector)

**B-IX-1. Default active embedding triple is hardcoded `synthetic / stream-a-test / 32`.** (`index/query.rs:344-346`, reviewer-index B1)

- `Index::new` (line 21) and the free `pub fn upsert_memory` (line 287) silently use the synthetic triple if caller drops it. Violates spec §10.2.2 #5 ("No silent fallback").
- **Fix:** delete `default_active_embedding`. Either gate `Index::new` `#[cfg(test)]` or require explicit triple. Delete the free `pub fn upsert_memory` (looks dead — callers route through `Index::upsert_memory`).

**B-IX-2. `pending_embedding_jobs` schema missing 4 columns from spec §10.1.** (`index/schema.rs:35-41`, reviewer-index B2)

- Missing `content_hash`, `enqueued_at`, `attempts`, `last_error`. Spec §10.2.1 #6 stale-job reconciliation contract is unsatisfiable: workers will burn embedding compute on jobs whose chunks have changed.
- **Fix:** add the 4 columns. Populate `enqueued_at`/`content_hash` at all insert sites (query.rs:329-337, vector.rs:43-49, query.rs:371-385). Extend `reconcile_active_embedding_jobs` to delete jobs where `content_hash != memory_chunks.body_hash`.

**B-IX-3. `chunk_embedding_meta` missing `embedded_at` and `vector_table`.** (`index/schema.rs:51-58`, reviewer-index B3)

- Spec §10.1 mandates both. `vector_table` lets readers locate the sqlite-vec table without recomputing `vector_table_name`. `embedded_at` required for Stream H eval reproducibility.
- Column rename `content_hash` → `chunk_hash` propagates inconsistency (`memory_chunks.body_hash`, `chunk_embedding_meta.chunk_hash`, `EmbeddingUpdate.expected_chunk_hash` all share visual stem `_hash` for different things).
- **Fix:** add the 2 columns. Rename `chunk_hash` → `content_hash` everywhere. Add `FOREIGN KEY(chunk_id) REFERENCES memory_chunks(chunk_id) ON DELETE CASCADE` to simplify reconciliation.

**B-IX-4. `memories` table is 6 columns; spec §10.1 mandates 27.** (`index/schema.rs:6-13`, reviewer-index B4)

- Missing: `schema_version`, `type`, `scope`, `namespace`, `canonical_namespace_id`, `confidence`, `trust_level`, `status`, `review_state`, `requires_user_confirmation`, `created_at`, `updated_at`, `observed_at`, `valid_from`, `valid_until`, `ttl`, `author`, `source_kind`, `source_harness`, `source_device`, `frontmatter_json`, `file_hash`, `file_mtime_ns`, `indexed_at`.
- Five compound indexes from §10.1 entirely absent. §10.6 query p95 acceptance unmeasurable.
- Without `frontmatter_json`, `query_memory` cannot return hydrated frontmatter without re-reading the file — destroys "SQLite is the read path" model.
- **Fix:** align to §10.1 schema. (See open question #1.)

**B-IX-5. Index migrations have no schema-version gate.** (`index/migrations.rs:10-19`, reviewer-index B5)

- Unconditionally `execute_batch`es `IF NOT EXISTS` DDL. No upgrade detection (v2 build opens v1 DB silently). No downgrade detection. No `PRAGMA journal_mode = WAL`.
- **Fix:** introduce `INDEX_SUPPORTED_SCHEMA_VERSION: u32 = 1`. Read `MAX(version)` from `schema_migrations` after bootstrap; refuse if exceeds supported (typed `OpenError::SchemaVersionUnsupported`). Add `PRAGMA journal_mode = WAL` and `PRAGMA synchronous = NORMAL` outside any transaction.

**B-IX-6. `update_embedding` writes vector inside SQLite transaction; `&self` + `unchecked_transaction` is unsound.** (`index/query.rs:153-186`, reviewer-index B6)

- Spec §10.2.1 step 4: vector upsert FIRST (outside any txn), THEN one SQLite txn for `chunk_embedding_meta` + `pending_embedding_jobs`. Code puts the vector upsert INSIDE the txn → on rollback, vector remains, orphan is the _normal_ case rather than the exception.
- `unchecked_transaction` on `&self` lets two threads race transactions.
- **Fix:** restructure. Vector upsert outside txn. Take `&mut self` for `transaction()`. Same for `reconcile_active_embedding_jobs`.

### 3.4 Merge driver

**B-MG-1. Three sub-modules are 1-line `//!` doc-only stubs declared as private modules.** (`merge/field_rules.rs`, `merge/lifecycle.rs`, `merge/quarantine.rs`, reviewer-merge B1)

- All 500 LOC dumped in `three_way.rs`. Module names imply organization that doesn't exist.
- **Fix:** populate the modules. Suggested split:
  - `field_rules.rs`: `merge_frontmatter_scalars`, `merge_extras`, `merge_regression`, union helpers, immutable-field guards, per-policy 3-way.
  - `lifecycle.rs`: `merge_lifecycle`, §14.5 pair table, replacement for `lifecycle_rank`.
  - `quarantine.rs`: `quarantine_unparsed_sides`, `quarantine_merge`, `add_add_quarantine`, `append_merge_diagnostic`.

**B-MG-2. `_merge_diagnostics` shape diverges from spec §6.10.** (`three_way.rs:473-500`, reviewer-merge B2)

- Code: `{status, conflicting_fields, details: [...]}` — synthetic `details[]` bag.
- Spec: top-level `merge_id` (ULID), `created_at`, `status`, `conflicting_fields`, `preserved_sources`, `evidence_near_duplicates`, `privacy_scans_preserved`, `add_add_alternates`, `unparsed_sides`, `lifecycle_notes`, `human_reason`.
- **Fix:** rewrite `append_merge_diagnostic` to emit spec shape directly with typed top-level keys. Populate `merge_id`/`created_at`. Update tests (which currently substring-assert against the buggy shape).

**B-MG-3. `status` values are wrong.** (`three_way.rs:184, 252, 351, 387`, reviewer-merge B3)

- Code writes `"clean_with_diagnostics"`. Spec enumerates `clean_with_warnings | quarantined`.
- **Fix:** rename to `"clean_with_warnings"`. Mechanical.

**B-MG-4. `add_add_alternates[]` cannot mechanically recover original blobs.** (`three_way.rs:459-465`, reviewer-merge B4)

- Stores `{id, frontmatter: <parsed-json>, body: <plain-string>}`.
- Spec §6.10 mandates `{id, original_path, frontmatter_yaml_b64, body_sha256, body_b64 | body_artifact_ref}`.
- Spec §14.6: exit `0` only if every original frontmatter and body is mechanically recoverable. Round-trip parsed-YAML→JSON→YAML loses key order, comments, anchors, quoting.
- **Fix:** capture raw bytes of loser side at parse time (split on `---\n.../---\n`), base64-encode as `frontmatter_yaml_b64` + `body_b64`, compute `body_sha256`. Don't reconstitute from parsed `Memory`.

**B-MG-5. `unparsed_sides[]` shape diverges.** (`three_way.rs:113-120`, reviewer-merge B5)

- Code: `{side, path, raw_b64, parse_error}` (one blob).
- Spec: `{side, path, frontmatter_raw_b64, body_b64, parse_error}` (separated).
- **Fix:** split raw input on first frontmatter terminator before base64-encoding.

**B-MG-6. `union_json_values` is non-commutative — breaks two-clone convergence.** (`three_way.rs:303-311`, reviewer-merge B19, **convergence-breaking**)

- Output order = `ours-order, then theirs-only-in-theirs-order`. Two clones merging the same logical pair with swapped `(ours, theirs)` labels produce different bytes for `evidence` (and any future array union).
- `tags`/`aliases` happen to be safe because `union_sorted` sorts at the end.
- Spec §13.6.1 canonical-content equality fixed point unreachable.
- **Fix:** sort by stable JSON-canonical key (or per-field key — evidence by `id`, tombstone events by event id, entities by id). Add fuzz test that runs merge with `(ours, theirs)` and `(theirs, ours)` swapped, asserts identical bytes.

**B-MG-7. `updated_at` and `created_at` are never merged.** (`three_way.rs`, reviewer-merge B7)

- Spec §14.4: `updated_at = max`, `created_at = min`. Code: zero references to either field in merge logic. Merged file keeps `ours.frontmatter.updated_at`.
- **Fix:** in `merge_frontmatter_scalars`, set `merged.updated_at = ours.max(theirs)` and `merged.created_at = ours.min(theirs)`.

**B-MG-8. `confidence` 3-way conflict has no implementation.** (`three_way.rs:161-165`, reviewer-merge B8)

- Only the asymmetric "ours unchanged → take theirs" case. The "all three differ" arm in spec §14.4 (later `updated_at` wins; quarantine if delta > 0.25) is missing.
- **Fix:** add the 3-way conflict arm with the >0.25 quarantine guard.

**B-MG-9. Spec §14.4 array unions silently drop theirs' edits.** (reviewer-merge B9, B10)

- `tombstone_events` never unioned (`copy_lifecycle` overwrites from one side).
- `superseded_by` overwritten not unioned.
- `entities`, `supersedes`, `related` not unioned.
- `evidence` dedups by whole-JSON whitespace-normalized equality, not by `id` (spec §14.4 mandates id-keyed union).
- **Fix:** add union handling for each array field per the §14.4 table. For `evidence`, dedupe primarily by `id`, fall back to `(quote_norm_hash, ref)`, emit `evidence_near_duplicates` diagnostic when only secondary key matches.

**B-MG-10. Immutable fields not enforced.** (reviewer-merge B12)

- `type`, `scope`, `namespace`, `canonical_namespace_id` — spec §14.4 row 3 says immutable, same-field conflict quarantines. Code: silently keeps ours.
- **Fix:** check each immutable field before `merge_frontmatter_scalars` returns; quarantine on divergence.

**B-MG-11. `review_state`, `requires_user_confirmation`, `retrieval_policy`, `write_policy` rules absent.** (reviewer-merge B13, B14)

- `review_state`/`requires_user_confirmation`: spec wants stricter-state-wins ordering, with `approved` vs `rejected` quarantining. Not implemented.
- `retrieval_policy`/`write_policy`: spec wants recursive per-key 3-way with stricter-wins on safety keys. Code only post-hoc clamps based on merged sensitivity (after-the-fact override, not 3-way merge).
- **Fix:** add per-field merges per spec §14.4.

**B-MG-12. Lifecycle table §14.5 wrong.** (`three_way.rs:229-261`, `copy_lifecycle:275-290`, reviewer-merge B15)

- §14.5 #1: tombstone clears `superseded_by` — not implemented.
- §14.5 #5: archived vs superseded → quarantine — code picks higher `lifecycle_rank` and emits "clean_with_diagnostics".
- §14.5 #4: `superseded` beats active only if `superseded_by` survives validation — no validation.
- `lifecycle_rank` ordinal doesn't match spec's pair semantics (`Quarantined=5` outranks `Pinned=4`, but spec doesn't order them that way).
- **Fix:** model the pair-table as data, not a `match` ladder. Replace with explicit `match (ours_status, theirs_status)` table or 2D lookup.

**B-MG-13. `_merge_diagnostics` itself not unioned across sides.** (`three_way.rs:76`, reviewer-merge B16)

- `merged = ours.memory.clone()` keeps ours' diagnostics; theirs' prior diagnostics dropped. Spec §14.7: "It must be preserved by future merges until resolved by admin command."
- **Fix:** before returning, union ours/theirs/base merge_diagnostics by stable id/content hash on every merge.

**B-MG-14. `secret`-sensitivity refusal not implemented at merge entry.** (reviewer-merge B17)

- Spec §14.4: `sensitivity: secret` causes driver to exit 1 without writing a merged file.
- `Sensitivity` enum (model.rs:69-78) has no `Secret` variant → serde parse-fail → file takes `quarantine_unparsed_sides` path.
- `error::ValidationError::SecretSensitivityOnDisk` is defined and never used.
- **Fix:** add textual prefilter in `merge_markdown` scanning for `sensitivity: secret` token before YAML parse. Surface as exit 1 with `merge-driver: secret sensitivity refused`. Add fixture mirroring schema-version gate test.

**B-MG-15. Validation-failure quarantine fallback missing.** (`three_way.rs`, reviewer-merge B18)

- Spec §14.2 #7: validation failure on merged result → retry with `status: quarantined` + diagnostics; only exit 1 if quarantine output also won't validate.
- Code: `serialize_document` failure propagates as `MergeError::Parse` and CLI exits 1.
- **Fix:** add quarantine retry path before exit 1.

### 3.5 Runtime / Watcher / Bench / Test-support

**B-RT-1. `Substrate::open` startup reconciliation: 4 of 9 phases.** (`runtime/reconcile.rs:119-130`, `api.rs:613-641`, reviewer-runtime B1)

- Missing phases:
  - **1** Crash-recovery scan (read `~/.memoryd/startup-reconcile.required` + `<repo>/.git/MERGE_HEAD`). Marker is written from 7 sites in `api.rs` but never read or cleared.
  - **2** Working-tree audit (`git status --porcelain=v1 -z` classification, quarantine to `~/.memoryd/quarantine/<startup-ts>/`, `OperatorRepairRequired` outcome).
  - **7** Index/file consistency (currently `full_reindex_from_repo` always rebuilds — wrong cost model for healthy startups).
  - **8** Auto-commit any uncommitted post-merge reconciliation work.
  - **9** Single `StartupReconciliationCompleted` event with `phases_run`, `vector_repairs`, `event_repairs`, `pending_index_replays`, `operator_action_required`. Today two events fire; first reports `reindexed: 0` even though full_reindex runs after.
- Spec §13.5.1: "Substrate must not return from open until startup reconciliation completes."
- **Fix:** collapse `reconcile_startup` + `replay_pending_repairs` + `full_reindex_from_repo` into a single `reconcile_startup(repo, runtime, event_log, &mut index) -> ReconcileReport` running 9 phases in order, emitting one completion event with all required fields, clearing the marker only on success.

**B-RT-2. `replay_pending_repairs` is not the §10.4/§13.5.1 idempotent replay.** (`runtime/reconcile.rs:133-208`, reviewer-runtime B2)

- No per-op `PendingIndexReplayed` / `PendingEventReplayed` events.
- Encrypted-index op hash mismatch → bails out of all reconciliation with generic `OperatorRepairRequired` (spec wants quarantine + continue).
- After replay, `compact_pending_file` _renames_ to `.compacted.jsonl` rather than deleting → orphans on disk forever.
- Conditional at line 190-193 conflates three independent triggers; emits completion event lying about counts.
- **Fix:** keep `RemainingOps` per queue type. Distinguish "replayed" / "deferred for hash mismatch" / "quarantined for corruption". Emit per-op events. `remove_file` not rename (or document forensics + add janitor).

**B-RT-3. Bench harness uses its own xorshift instead of spec-sanctioned helper.** (`bin/stream_a_bench.rs:316-331`, reviewer-runtime B3)

- Inline `synthetic_vector` produces different vectors from `memory-test-support::perf::synthetic_vectors` for same `(seed, dim, idx)`.
- Spec §17.6 / §18 boilerplate item 13: "`memory-test-support::perf::synthetic_vectors` is the **sanctioned source**."
- **Fix:** delete inline helper. Call `memory_test_support::perf::synthetic_vector(seed, dimension, index)`.

**B-RT-4. Bench output JSON omits `corpus_sha256`.** (`bin/stream_a_bench.rs:129-145`, reviewer-runtime B4)

- Spec §17.6: seed AND `SHA256(corpus)` recorded in `bench/results.json` so regressions confirm against identical corpus.
- `memory-test-support::perf::corpus_sha256` exists, never called.
- **Fix:** build corpus once via test-support helper, hash via `corpus_sha256`, emit `corpus_sha256`/`vector_dimension`/`active_triple` in report.

**B-RT-5. Watcher cannot deliver `RescanRequired` events.** (`watcher/subscription.rs:75-87`, reviewer-runtime B5)

- `notify` callback handles only `Ok(event)`, drops `Err` silently. No `WatchEventKind::RescanRequired` ever constructed despite `FileEvent::rescan_required` existing.
- Spec §11.1 + §11.4: watcher overflow must emit `RescanRequired` and a reindex must converge.
- **Fix:** branch on `notify::Event::need_rescan()` / `Err(_)`. Emit `FileEvent::rescan_required(root)` per overflow batch. Stop discarding `Err` — log via `tracing` or emit `WatchEventKind::Error`.

**B-RT-6. Watcher does not apply spec §11.2 path filters.** (`watcher/subscription.rs:75-84`, reviewer-runtime B6)

- Forwards `.git/`, `.DS_Store`, atomic-write `.tmp.<op_id>` files, editor backups. Self-event suppression catches temps only when they hash to a tracked content hash — essentially never.
- A daemon driving reindex off the watcher will reindex on `git fetch` because every `.git/refs/...` change shows up.
- `is_memory_path` (filter.rs:6-8) is too narrow (excludes `.gitattributes`, `config.yaml`, `policies/**`).
- **Fix:** introduce `should_watch(path: &Path)` predicate covering `.git/`, `.DS_Store`, editor backups, atomic-write temp prefix. Call before sending.

**B-RT-7. `runtime::blocking::run_blocking` is dead code.** (`runtime/blocking.rs:4-10`, reviewer-runtime B7)

- Zero call sites. `Substrate::reindex` / `query_memory` / `query_chunks` are `async fn` synchronously holding `std::sync::Mutex` and running `rusqlite` directly on the calling task.
- Either the discipline applies (these methods are wrong) or it doesn't (`run_blocking` is misleading).
- **Fix:** pick one. (Coordinated decision with B-API-3 below — Section 5 cross-cutting.)

### 3.6 Public API / Model / Error

**B-API-1. `read_memory` returns `Memory` instead of `MemoryEnvelope`.** (`api.rs:1681`, `model.rs:349`, reviewer-api)

- Spec §16.2: `read_memory(...) -> Result<MemoryEnvelope, ReadError>` with `MemoryContent { Plaintext, Ciphertext { bytes, encryption }, MetadataOnly }`.
- Code returns raw `Memory` with single `body: String`. No way to distinguish plaintext / encrypted-metadata / ciphertext envelope. Stream E recall block assembly cannot route correctly.
- **Fix:** add `MemoryEnvelope { metadata: Memory, content: MemoryContent }`. `Ciphertext` carries `EncryptionEnvelope` so encrypted callers route through Stream D without Stream A knowing how to decrypt.

**B-API-2. `QueryResult` and `ChunkResult` too thin for spec §10.4 / §16.4.** (`model.rs:621-651`, reviewer-api)

- Missing `body_indexability: BodyIndexability { Full | MetadataOnly | None }`, `score_breakdown { fts, vector, distance }`.
- **Fix:** rename to `MemoryHit`/`ChunkHit` per spec, add the missing fields.

**B-API-3. All public methods are `async fn` over blocking I/O.** (`api.rs:96, 219, 384, 517`, reviewer-api)

- Zero `spawn_blocking`. Locks `std::sync::Mutex` (parks tokio worker; deadlocks current-thread runtime). Calls `std::fs` / `Command` / `rusqlite` directly inside `async fn`.
- Spec §16.5: blocking sections must run on configured blocking executor or single index thread. Spec §16.7: cancellation safety required.
- **Fix:** pick one. (a) Make methods sync, document blocking contract (spec permits "Stream A may be synchronous internally"). (b) Wrap bodies in `tokio::task::spawn_blocking` with configured pool. Currently neither.

**B-API-4. `drop_embedding_model` returns `usize`, spec mandates `DropTripleReport`.** (`api.rs:561`, reviewer-api)

- Spec §16.4 wants `DropTripleReport { vectors_removed, meta_rows_removed, pending_jobs_dropped, table_dropped }`.
- **Fix:** add the type, return it.

**B-API-5. `Frontmatter::extras` is `#[serde(skip)]`.** (`model.rs:344-346`, reviewer-api)

- Spec §6.2: "Unknown future fields preserved in `_extras` and re-emitted after known fields."
- `#[serde(skip)]` means unknown fields silently dropped on serialize. Spec §6.13 round-trip acceptance fails.
- **Fix:** remove `#[serde(skip)]`. Use `#[serde(flatten)]` or drive canonical serializer manually with explicit extras-after-known ordering. Add round-trip test pinning unknown-field survival.

**B-API-6. `entities`, `evidence`, `tombstone_events` typed as `Vec<serde_json::Value>`.** (`model.rs:316-336`, reviewer-api)

- Spec §6.4-§6.5 mandates structured types.
- Every site touching them — merge driver, validator, frontmatter writer, bench fixture — hand-marshals `serde_json::Value`. Value-bag anti-pattern.
- **Fix:** define `Evidence`, `Entity`, `TombstoneEvent` structs in `model.rs`. Migrate. Zero runtime cost; large type-safety win.

**B-API-7. `read_memory` does linear filesystem scan instead of using SQLite index.** (`api.rs:78-88`, reviewer-api)

- O(n) disk reads to find by ID. Diverges from index (memory not yet reindexed but on disk is found; one in index but not on disk is not).
- **Fix:** resolve `MemoryId → RepoPath` through SQLite index, fall back to file read for body. (Becomes the §10.4 metadata-only routing path.)

**B-API-8. Event log: 8 event kinds; spec §12.2 lists ~24.** (`events/log.rs:30-47`, reviewer-api)

- Missing: `WriteStarted`, `WriteIndexed`, `WriteEventAppendFailed`, `WriteRefused`, `Deleted`, `Superseded`, `IndexUpdated`, `IndexFailed`, `VectorReconciled`, `EmbeddingJobEnqueued`, `EventLogRecovered`, `MergeQuarantined`, `PendingIndexReplayed`, `PendingEventReplayed`, `GitCommitted`, `GitFetched`, `WatcherSuppressed`, `ReconciliationRepaired`.
- Several are referenced as acceptance signals.
- `EncryptedWriteCommitted` is in impl but not in spec §12.2 (diverges other way).
- **Fix:** reconcile to spec list. If spec is wrong on some, bump spec; do not let impl drift be the resolution.

**B-API-9. `WriteRefused` audit events never emitted.** (`api.rs:99-104, 223-230, 680-691`, reviewer-api)

- Spec §8.7 step 6: refusals logged in event payload so audit can confirm Stream D made positive call on every write.
- Code returns `Err(WriteFailure { kind: ... })` and never appends audit event.
- **Fix:** add `EventKind::WriteRefused { id, kind: WriteFailureKind, classification }`. Emit on every refusal path before returning error. Refusal events don't need disk-side commit; per-device event log append is sufficient.

**B-API-10. Lock poisoning maps to wrong typed errors.** (`api.rs:556, 564, 570, 117, 421`, reviewer-api)

- Poisoned mutex → `VectorError::UnknownEmbeddingTriple` (lies) or `WriteFailureKind::Io(String)` (loses info).
- **Fix:** add `VectorError::IndexUnavailable(String)`. Reserve `UnknownEmbeddingTriple` for the actual condition.

**B-API-11. `id_type!` macro implements unvalidated `From<&str>` / `From<String>`.** (`model.rs:511-520`, reviewer-api)

- `MemoryId::new("not_a_memory_id")` and `RepoPath::new("../../../etc/passwd")` compile, flow through public API.
- `RepoPath::try_new` exists but is a polite suggestion; the `From` impls bypass it.
- **Fix:** remove unchecked `From` impls. Make `MemoryId::try_new` validate the regex. Make `MemoryId::new` validate-or-disappear. Same for `RepoPath`.

**B-API-12. `Substrate::open` auto-mints device IDs, violating invariant 4.** (`api.rs:783-820`, reviewer-api / reviewer-frontmatter R7)

- Hand-rolled YAML parser at `parse_device_id` line-by-line splits, picks first `id:` match — misreads `paths:\n  id:` as device id.
- `write_local_device_id` uses non-atomic `std::fs::write`.
- Bypasses `config::load_local_device_config`. Auto-creates device id when `local-device.yaml` missing.
- Spec invariant 4: "A fresh clone must regenerate device identity via `git::adopt_clone` before any write."
- **Fix:** route through `config::load_local_device_config` using `serde_yaml`. Move device-id minting into `git::adopt_clone`. `Substrate::open` should fail with `OperatorRepairRequired` when missing. (Or — see open question #2 — update spec to acknowledge `open` is the device-identity authority.)

---

## 4. High-priority risks (worth fixing in same pass)

These cluster with blockers and fix together cheaply. Full prose in per-reviewer files.

- **R-FT-1. Default `EmbeddingTriple` returned when `config.yaml` is missing** (`config/mod.rs:74-78`). Same root cause as B-IX-1 — kill silent fallbacks. (reviewer-frontmatter R1)
- **R-FT-2. Two `SUPPORTED_SCHEMA_VERSION` constants** (`frontmatter::schema:4` + `merge::mod.rs:11`). One source of truth. (reviewer-frontmatter R5)
- **R-FT-3. `serialize_frontmatter` round-trip not byte-stable for `null`/`true`/`false`/numeric-looking strings** (`frontmatter/serialize.rs:35-86`). Quote YAML reserved literals. (reviewer-frontmatter R6)
- **R-FT-4. `next_memory_ids` doesn't enforce date monotonicity on clock regression** (`ids/sequence.rs:88-100`). Add `IdError::ClockRegression`. (reviewer-frontmatter R8)
- **R-IO-1. `git_preflight` is wired up but never invoked** — covered by B-IO-4. (reviewer-io-git R7)
- **R-IO-2. `run_git` inherits parent env** (`git/command.rs:9-23`). Sanitize `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_NAMESPACE`. Use absolute git path resolved at startup. (reviewer-io-git R6)
- **R-IO-3. `adopt_clone` uses `current_exe()` for merge-driver path with bare-name fallback** (`git/adopt.rs:12-23`). Spec forbids ambient PATH. Take merge-driver path as parameter; pipe through `adopt_clone`. (reviewer-io-git R10)
- **R-IO-4. Durability probe collapses real failures to `BestEffort`** (`markdown/durability.rs:8-20`). Map only `Unsupported` to `BestEffort`; others to `Refused`. (reviewer-io-git R4)
- **R-IO-5. `events/log.rs:60-85 read_events` bails on first malformed line.** Compose with recovery, or rename to `read_events_strict`. (reviewer-io-git N4)
- **R-IO-6. No 64-KiB line bound on event append** — covered by B-IO-2. (reviewer-io-git R8)
- **R-IX-1. `query_chunks` and `query_vector_chunks` don't filter encrypted-memory chunks at SQL boundary.** Add `AND memories.metadata_only = 0` defense-in-depth. (reviewer-index R3)
- **R-IX-2. `VectorError::Storage(String)` swallows typed `rusqlite::Error` at ~20 sites.** Add `#[error(transparent)] Sqlite(#[from] rusqlite::Error)`. (reviewer-index R6)
- **R-IX-3. `validate_dimension` lives in `sqlite_vec.rs` but isn't sqlite-vec-specific.** Move to `vector.rs`. (reviewer-index R8)
- **R-IX-4. Chunk-id format diverges from spec.** Use `chk_<full-sha256-hex>`, not `{memory_id}:{16-hex}`. (reviewer-index R1)
- **R-IX-5. `chunking.rs` is a stub** — 4096-byte chunks, no overlap, no markdown awareness; spec §10.3 wants ~400 tokens / 80 overlap. Replace before any real perf measurement. (reviewer-index N5)
- **R-MG-1. `add_add_quarantine` doesn't detect ID collisions.** Spec §14.6 wants distinct handling for matching IDs. (reviewer-merge Risks)
- **R-MG-2. `merge_body` is whole-blob 3-way only.** Spec §14.2 #5 says "diff3 semantics" — implement or downgrade spec wording. (reviewer-merge Risks)
- **R-MG-3. `copy_lifecycle` lifecycle-quarantine-reason injection guarded by `is_none()`.** Drop the guard. (reviewer-merge Risks)
- **R-RT-1. `convergence::roots_converged` is a byte-equality check, not §13.6.1.** Either implement spec semantics or rename `roots_byte_equal`. (reviewer-runtime R1)
- **R-RT-2. `read_framed_jsonl` silent-truncates pending-repair queues.** Spec grants trailing-truncation only to event log. Move trailing-truncate logic out of generic reader. (reviewer-runtime R2)
- **R-RT-3. Bench `Fixture` PID-based path can race; 10K files in single dir unrealistic.** Use `tempfile::TempDir`; vary namespace. (reviewer-runtime R3)
- **R-RT-4. Bench `noise_floor_ms` synthesized from run's own p95.** Spec puts it in baseline only — drop from results. (reviewer-runtime R5)
- **R-RT-5. Suppression-ledger lock-poisoning silently disables suppression.** Log + propagate. (reviewer-runtime R6)
- **R-RT-6. `WatchError::Closed` collapses timeout and disconnection.** Add `WatchError::Timeout`. (reviewer-runtime R7)
- **R-RT-7. Suppression TTL hardcoded 30s; spec says 60s.** Extract `DEFAULT_SUPPRESSION_TTL`. (reviewer-runtime R8)
- **R-RT-8. Bench harness can clobber `bench/baseline.<profile>.json` if pointed directly.** Defense-in-depth refusal in `run()`. (reviewer-runtime R9)
- **R-API-1. Three near-identical 100+ LOC write paths** in `api.rs` — extract `commit_index_or_repair` and `commit_event_or_repair` helpers. (reviewer-api Risks)
- **R-API-2. Hardcoded fallback `"agent/patterns/{id}.md"` at 5 sites.** Make `Memory.path` non-optional or add `default_repo_path(&Frontmatter)` helper respecting type/scope/namespace. (reviewer-api Risks)
- **R-API-3. `WriteFailureKind::Validation(String)` / `Io(String)` are string-bag variants.** Wrap `ValidationError`; expose `std::io::ErrorKind`. (reviewer-api Risks)
- **R-API-4. `MergeError::Parse(String)` is a string bag.** Add `MergeError::Parse { side: MergeSide, source: serde_yaml::Error }`. (reviewer-api Risks)
- **R-API-5. `MemoryQuery` is far thinner than spec §10.4.** Add namespace/scope/status/type/sensitivity/time-range/pagination filters. (reviewer-api Risks)
- **R-API-6. `ChunkQuery` allows invalid combinations.** Make it an enum: `Fts | Vector | Hybrid`. (reviewer-api Risks)
- **R-API-7. `events()` returns `Vec<Event>`; spec wants streaming with `EventQuery` filter.** Swap to streaming reader. (reviewer-api Risks)

---

## 5. Suggested fix ordering for autonomous run

Dependency-aware sequencing. Each phase's outputs feed later phases; running out of order causes rework.

### Phase 0 — Cross-cutting decisions (before any fix work)

These need Trey's call OR a default-to-be-overridden — see Section 6 for the open questions:

- **Decision A.** Async surface: keep `async fn` facade with `spawn_blocking`, OR make sync internally. (Affects every method in `api.rs` + B-API-3 + B-RT-7.)
- **Decision B.** Merge driver name: `memory-merge-driver` (current code) vs `memory-frontmatter-merge` (spec §13.1). Update spec or rename code.
- **Decision C.** Device-identity authority: `git::adopt_clone` mints (spec invariant 4) OR `Substrate::open` mints (current code). Update spec or move logic.
- **Decision D.** Event-log CRC location: in-JSON (spec §12.1 example) OR out-of-band hex prefix (current code). Update spec or move.
- **Decision E.** `roots_converged` semantics: implement spec §13.6.1 OR rename `roots_byte_equal` and document weaker check.

### Phase 1 — Foundations (do these first; everything else depends on them)

- **B-FT-5** Allow-list reconciliation between `validate_repo_relative_path` and `tree::layout::memory_dirs`.
- **R-FT-2** Single `SUPPORTED_SCHEMA_VERSION` source of truth.
- **B-IO-3** Full `.gitattributes` per spec §13.1 step 2 (also pins Decision B).
- **B-IX-2, B-IX-3, B-IX-4, B-IX-5** Index schema alignment + WAL pragma + version gate (these are interlocking; do as one batch).
- **B-API-5, B-API-6** `Frontmatter::extras` round-trip + structured `Evidence`/`Entity`/`TombstoneEvent` types. (Merge driver depends on these.)
- **B-API-11** Validated newtypes (`MemoryId::try_new`, `RepoPath::try_new`, drop unchecked `From` impls).

### Phase 2 — IO & Git happy path

- **B-IO-1** Fix UTF-8 lossy decode in event-log recovery. (Silent-data-corruption blocker.)
- **B-IO-2** Event framing: add `schema`/`device`/`seq`. CRC location per Decision D. 64-KiB line bound.
- **B-IO-7** Distinguish "nothing to commit" from real commit failure.
- **B-IO-6** Restrict `auto_commit` to spec namespaces; add `/.*.tmp` to gitignore.
- **B-IO-4** Implement spec §13.5 protocol in `fetch_and_merge`. (Depends on B-IO-3 for `.gitattributes` and B-IO-7 for clean-status detection.)
- **B-IO-5** Parameterize `refuse_duplicate_device_logs` on `local_device_id`.
- **B-IO-8** Suppression-ledger poisoning + ordering. (Depends on Decision A.)
- **R-IO-2, R-IO-3, R-IO-4** Env sanitization, merge-driver path plumbing, durability probe classification.

### Phase 3 — Index correctness

- **B-IX-1** Kill default synthetic embedding triple. (Depends on Phase 1 schema work.)
- **B-IX-6** Restructure `update_embedding`: vector outside txn, `&mut self`, `transaction()` not `unchecked_transaction`.
- **R-IX-1** Add `metadata_only = 0` filter to query paths.
- **R-IX-2** Replace `VectorError::Storage(String)` with typed `rusqlite::Error` flow.
- **R-IX-3, R-IX-4, R-IX-5** Move `validate_dimension`, fix chunk-id format, replace stub chunker.

### Phase 4 — Merge driver rewrite

This is the heaviest phase. Recommend a single dedicated worker on the merge-driver fix because the changes are interlocking.

- **B-MG-1** Populate `field_rules.rs`, `lifecycle.rs`, `quarantine.rs`. Move logic out of `three_way.rs`.
- **B-MG-2, B-MG-3, B-MG-4, B-MG-5** Rewrite `_merge_diagnostics` shape per spec §6.10. Capture raw bytes for `add_add_alternates`/`unparsed_sides`. (Depends on Phase 1 — `Frontmatter::extras`, structured Evidence type.)
- **B-MG-6** Sort `union_json_values` deterministically. Add (ours, theirs) swap fuzz test.
- **B-MG-7, B-MG-8, B-MG-9, B-MG-10, B-MG-11** Field rule completeness per spec §14.4.
- **B-MG-12** Replace `lifecycle_rank` with §14.5 pair table.
- **B-MG-13** Union `_merge_diagnostics` across sides.
- **B-MG-14** Textual `secret`-sensitivity prefilter.
- **B-MG-15** Validation-failure quarantine fallback.
- **R-MG-1, R-MG-2, R-MG-3** Risk fixes that ride along.
- **Tests:** rewrite `merge_rules.rs` to parse YAML output and assert on structured value, not substring. Add CLI-level clean-merge round-trip and quarantine-output cases. Add fuzz test for swap-order convergence.

### Phase 5 — Public API alignment

- **B-API-1** Add `MemoryEnvelope` and `MemoryContent`. Migrate `read_memory`/`read_path`.
- **B-API-2** Add `MemoryHit`/`ChunkHit` with `body_indexability` and `score_breakdown`. (Depends on Phase 3 schema.)
- **B-API-4** Add `DropTripleReport`.
- **B-API-7** Resolve `MemoryId → RepoPath` via SQLite index. (Depends on Phase 3.)
- **B-API-8** Reconcile event-kind set to spec §12.2.
- **B-API-9** `WriteRefused` audit events on every refusal path.
- **B-API-10** Add `VectorError::IndexUnavailable`. Stop misclassifying poisoning.
- **B-API-12** Move device-id minting per Decision C.
- **B-API-3** Fix async/blocking surface per Decision A.
- **R-API-1** Extract `commit_index_or_repair` and `commit_event_or_repair`.
- **R-API-2** Fix hardcoded fallback path.
- **R-API-3, R-API-4** Replace string-bag error variants.
- **R-API-5, R-API-6, R-API-7** Flesh out `MemoryQuery`/`ChunkQuery`/streaming events.

### Phase 6 — Frontmatter / IDs / Tree

- **B-FT-1** Rewrite `repair_duplicate_ids`. (Depends on B-API-11 for validated newtypes.)
- **B-FT-2** Add rollback on validation failure post-rename.
- **B-FT-3** Wire merge-driver-config check (per Decision E approach).
- **B-FT-4** Path validation in tree walker. Plaintext-under-`encrypted/` check.
- **R-FT-3** YAML round-trip stability.
- **R-FT-4** Clock regression check.

### Phase 7 — Runtime / Watcher / Bench

- **B-RT-1** Implement spec §13.5.1 startup reconciliation (9 phases).
- **B-RT-2** Fix `replay_pending_repairs` per-op events + remove orphan compacted files.
- **B-RT-5** Watcher overflow → `RescanRequired` events.
- **B-RT-6** Watcher path filters per spec §11.2.
- **B-RT-3, B-RT-4** Bench harness uses sanctioned `synthetic_vectors` + emits `corpus_sha256`.
- **B-RT-7** Resolve per Decision A.
- **R-RT-1** Per Decision E.
- **R-RT-2 to R-RT-8** Risk fixes.

### Phase 8 — Test coverage uplift (mandatory before re-running release-cert claim)

- Rewrite all merge-driver tests to parse output and assert structurally.
- Add property tests for `union_json_values` commutativity.
- Add fuzz harness for event-log recovery (random truncation patterns).
- Add CLI-level coverage for every merge result type (clean, clean-with-warnings, quarantine, add-add, schema-version-gate, secret-refusal).
- Add round-trip tests for unknown frontmatter fields.
- Wire `scripts/two-clone-convergence.sh` into the gate. Confirm it actually ran.
- Wire `scripts/check.sh` and confirm every command in `docs/reviews/stream-a-final-review.md` actually passes against the fixed code.

### Phase 9 — Process

- Move all work into worktree-per-task per the original plan. Do not work directly on `main`.
- Run `scripts/check.sh` on the integrated trunk after each task's `integrate-task-worktree.sh` fast-forward.
- Replace `docs/reviews/stream-a-final-review.md` with an honest acceptance-evidence document — only after Phase 8 passes.

---

## 6. Open questions for Trey (need decisions before/during fix run)

1. **`memories` table scope (B-IX-4).** Is the 6-column reduction intentional Task-N scaffold to be expanded later, or a missed contract? If scaffold, add `// SCAFFOLD:` comment + plan callout. If contract, fix now.
2. **Async surface (B-API-3, B-RT-7).** Sync-internal-with-async-facade (spec permits) OR actual `spawn_blocking` runtime? Either is defensible; current state is neither.
3. **Merge driver name (B-IO-3, code-spec divergence).** `memory-merge-driver` (code) vs `memory-frontmatter-merge` (spec §13.1). Pick canonical; bump spec or rename code.
4. **Device-identity authority (B-API-12, B-FT R7).** `git::adopt_clone` mints (spec invariant 4) OR `Substrate::open` mints (current code with auto-create)?
5. **Event-log CRC location (B-IO-2).** In-JSON object (spec §12.1 example) OR out-of-band hex prefix (current code)? Code's posture is faster + arguably safer; spec example is normative. Want me to draft §12.1.1 errata, or have the implementation move CRC into the object?
6. **Plaintext-under-`encrypted/` detection (B-FT-4).** Ciphertext under `encrypted/` is also `.md`-suffixed per spec §5.1. Validator needs a content-shape signal. Should we require `encryption:` frontmatter on every file under `encrypted/`, OR is this Stream D's enforcement and Stream A merely refuses to write?
7. **`roots_converged` semantics (R-RT-1).** Implement §13.6.1 in test-support, OR rename `roots_byte_equal` and let `scripts/two-clone-convergence.sh` own real convergence?
8. **`merge_body` diff3 (R-MG-2).** Spec §14.2 #5 says "diff3 semantics" — was that loose for "3-way" or literal? Affects whether we add `imara-diff` dep + ~100 LOC of fixtures.
9. **`Sensitivity::Secret` variant (B-MG-14).** Add a `Secret` variant with custom deserialize that errors loudly, OR keep enum without it and rely on textual prefilter only?
10. **Bench fixture corpus shape (R-RT-3).** Real Stream E namespacing changes p95s. Pin before first real baseline goes in (baselines are immutable absent explicit human commits).
11. **`compacted.jsonl` artefacts (B-RT-2).** Forensic retention OR delete after replay?
12. **`runtime::blocking::run_blocking` (B-RT-7).** Aspirational scaffolding for Stream B, OR load-bearing for Stream A's async surface? Affects Decision A.

---

## 7. Process notes

- **All ~13K LOC is uncommitted on `main` beyond `ff708ef seed`.** No worktrees. Codex worked directly on `main` instead of using the plan's `../agent-memory-wt/task-NN/` worktree-per-task discipline. The `scripts/check.sh` integrated-trunk gate exists for exactly this — it never got to run end-to-end.
- **`docs/reviews/stream-a-final-review.md` is not credible.** It claims clean acceptance against `scripts/check.sh` but has no commits to verify against, has two `SUPPORTED_SCHEMA_VERSION` constants, has missing schema gates, and ships doc-only stub modules in `merge/`. Worth asking Codex to produce the actual command outputs and rerun against fixed code before the document is trusted.
- **Testing posture needs to change.** `text.contains(...)` substring assertions on YAML output let multiple structural divergences pass green. Going forward: parse the output, assert on structured value. Add property/fuzz tests for the convergence and recovery paths — they're algorithmic surfaces where unit tests are structurally insufficient.
- **The team-lead orchestration model worked.** Six parallel reviewers on disjoint slices, each with the clean-code skill loaded, anchored on spec sections and critical invariants — surfaced findings the self-review missed. Same shape works for the autonomous fix run, with parallelism per area where dependencies allow.

---

## 8. Per-reviewer drill-down

When fixing, refer back to the per-reviewer file for full prose on each finding:

- `01-frontmatter-ids-config-tree.md` — frontmatter parse/validate/serialize, IDs (sequence/repair), config, tree validate
- `02-markdown-events-git.md` — atomic IO, event log, git transport (init/commit/sync/preflight/adopt)
- `03-index.md` — SQLite + FTS + vector + chunking + query
- `04-merge.md` — three_way merge + memory-merge-driver crate
- `05-runtime-watcher-bench.md` — runtime/reconcile, watcher, bench harness, test-support crate
- `06-public-api.md` — `lib.rs`, `api.rs`, `model.rs`, `error.rs`

Each per-reviewer file has Blockers / Risks / Nits / Strengths / Open Questions.
