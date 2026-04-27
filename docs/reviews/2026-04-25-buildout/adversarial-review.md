# Stream A — Post-fix-run adversarial review

**Date:** 2026-04-25
**Reviewer:** Claude (architect, fresh context)
**Method:** Read the live spec (v1.1), the 12 binding decisions, and the actual code. Cross-checked claims against `crates/memory-substrate/src/`, `crates/memory-merge-driver/src/`, `crates/memory-test-support/src/`, and `fuzz/`. Did **not** read the per-phase final reports.

---

## 1. Verdict — **NO**

The implementation does not satisfy the spec contract Streams B–I will rely on. Two sets of issues land it short:

1. **Two decision-doc commitments are out and out unfixed.** Q2 (truly async via `spawn_blocking` + parking_lot::Mutex + flume actor) is wholly absent — `api.rs` still uses `std::sync::Mutex` inside `async fn`, no `spawn_blocking` anywhere in the workspace, no `parking_lot` or `flume` in `Cargo.toml`. Q9's `Sensitivity` enum is correctly free of a `Secret` variant and the textual prefilter exists, but `ClassificationOutcome::Secret` lives on (correct per decision) — this one's fine; it's Q2 that is the bald-faced violation.

2. **Critical invariant 3 is violated by the bootstrapping path.** Both `tree/layout.rs:56` (`bootstrap_repo_tree`) and `api.rs:1236` (`write_initial_config_if_absent`) write `provider: synthetic / model_ref: stream-a-test / dimension: 32` to disk on every `Substrate::init`. This is not a runtime fallback — it's an enshrined-on-disk default. Stream B/D/E will load this config and embed against a triple that exists nowhere in production, then have to migrate later. Worse than the silent fallback flagged in B-IX-1.

There are also several convergence-breaking bugs the swap-order fuzz target was supposed to surface (id-keyed BTreeMap last-insert-wins for entities/tombstone*events/evidence; quarantine `merge_id`/`created_at` regenerated per-clone; conflict-marker labels swap on `(ours, theirs)` swap). The fuzz target itself silently `* = (left, right)`skips quarantine outputs — so it never catches the convergence bug class it was designed to detect. Plus a watcher panic surface, ~25 production-code panic sites via`RepoPath::new`/`MemoryId::new`/`DeviceId::new` on dynamic input, and Phases 7+8 of the spec §13.5.1 startup reconciliation are missing from the orchestrator.

This is a "passes its own tests" implementation with structural divergences the tests aren't shaped to catch — same anti-pattern as the original review, recurring one level down.

---

## 2. Decision-doc commitments

| ID  | Commitment                                                         | Result                                    | Evidence (file:line)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --- | ------------------------------------------------------------------ | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | `memories` table to spec §10.1 (28 cols + 5 compound idx)          | **MET** (memories) / **PARTIAL** (chunks) | `index/schema.rs:14-45` (28 cols + extras `metadata_only`); `index/schema.rs:47-56` (5 compound indexes). BUT `memory_chunks` (line 58-67) is missing `ordinal`, `token_start`, `token_end`, `chunk_hash`, `summary`, `indexable`, `created_at`, `updated_at`, `UNIQUE(memory_id, ordinal)`. Spec §10.1 lines 908-924 mandate them. Also missing entirely: `memory_supersession`, `memory_related`, `memory_regressions`, `memory_regression_occurrences` (acknowledged TODO at `schema.rs:126-127`).                                                                                                                                                   |
| Q2  | Async via `spawn_blocking` + parking_lot::Mutex + flume actor      | **VIOLATED**                              | `api.rs:7` `use std::sync::Mutex`. `api.rs:33` `index: Arc<Mutex<Index>>`. `api.rs:46/63/72/84/99/123/219/239/396/517/589/745/758/763/776/787/799/821/829/834/840/845/850/860/865/869` all `async fn`. `grep -rn "spawn_blocking" crates/` returns zero. `Cargo.toml` has no `parking_lot`, no `flume`. Index handle is shared `Mutex`, not channel actor.                                                                                                                                                                                                                                                                                              |
| Q3  | Driver name `memory-merge-driver` consistently                     | **MET (code) / VIOLATED (spec)**          | Code consistently uses `memory-merge-driver` (`tree/layout.rs:9`, `git/init.rs:31-37`, `Cargo.toml`). Spec still uses `memory-frontmatter-merge` at lines 1317, 1326, 1327 of `docs/specs/stream-a-core-substrate-v1.1.md`. Decision doc said "spec amendment to §13.1 step 1 and step 2 to read `memory-merge-driver`" — the amendment never landed. Per CLAUDE.md "code is the law" but spec-code drift remains a bug surface.                                                                                                                                                                                                                        |
| Q4  | Device-id authority in `git::adopt_clone`; `Substrate::open` fails | **MET**                                   | `error.rs:80` `OpenError::DeviceIdentityMissing { repair }`. `api.rs:1091-1097` `load_device_id` returns this error when `local-device.yaml` is absent. `git/adopt.rs:102-108` `mint_device_identity` is the sole minter. Routes through `serde_yaml`/`yaml_serde::from_str::<LocalDeviceConfig>` (via `config::load_local_device_config` at `config/mod.rs:91`).                                                                                                                                                                                                                                                                                       |
| Q5  | Event-log CRC in-JSON, schema/device/seq fields                    | **MET (shape) / VIOLATED (seq)**          | `events/log.rs:24-48` `Event` struct has `schema`, `device`, `seq`, `crc32c`. `events/framing.rs:11-43` injects CRC inside the JSON object. 64-KiB bound enforced (`framing.rs:9, 39-41`). BUT `seq` is hardcoded `0` at every emission site: `api.rs:993`, `api.rs:681`, `runtime/reconcile.rs:334`. The decision-doc said "`seq` persists to `~/.memoryd/event-seq.json` under exclusive lock." That file does not exist; spec §12.4 multi-device union ordering by `(ts, device, seq, id)` is unfulfilled.                                                                                                                                           |
| Q6  | Plaintext-under-`encrypted/` rejection                             | **MET**                                   | `tree/validate.rs:67-72` calls `validate_encrypted_tier` for any `.md` under `encrypted/`. `tree/validate.rs:108-118` checks `frontmatter.extras.contains_key("encryption")` and returns `ValidationError::PlaintextUnderEncryptedTier`.                                                                                                                                                                                                                                                                                                                                                                                                                |
| Q7  | `roots_converged` per spec §13.6.1                                 | **PARTIAL**                               | `memory-test-support/src/convergence.rs:55-67` exists and walks deterministically, parses Markdown frontmatter, sorts JSONL by `(ts, device, seq, id)` (line 240-244), returns structured `ConvergenceReport`. BUT the canonical path uses `serde_yaml::to_string(&serde_yaml::Value)` (line 179-180), not the spec-mandated `frontmatter::canonical_serialize`. Round-trip via `serde_yaml::Value` does not preserve quoting decisions for YAML reserved literals (`null`, `yes`, `0x10`, etc.) the way `serialize::serialize_frontmatter` does. So two clones whose canonical bytes differ in YAML-literal-quoting can be falsely declared converged. |
| Q8  | Literal diff3 via imara-diff                                       | **MET (clean) / RISK (conflict)**         | `merge/body_diff3.rs:3` imports `imara_diff::{Algorithm, Diff, Hunk, InternedInput, Token}`; `compute_hunks` runs Histogram diff at line 62. Disjoint hunks merge cleanly (line 102-121). BUT conflict markers (`format_conflict_markers`, line 123-129) emit the FULL ours/theirs body, not the conflicting hunk window — and the markers do NOT swap label-order to canonical, so `(ours, theirs)` and `(theirs, ours)` produce different bytes. Convergence-breaking on conflict; survives because the fuzz target (see below) skips quarantines.                                                                                                    |
| Q9  | No `Sensitivity::Secret`; textual prefilter                        | **MET**                                   | `model.rs:84-95` `Sensitivity` enum has only `Public, Internal, Confidential, Personal`. `ClassificationOutcome::Secret` (line 80) lives correctly on the runtime classification type. `merge/three_way.rs:73-105` `refuse_secret_sensitivity` runs textual prefilter before YAML parse; raises `MergeError::SecretSensitivityRefused`. CLI test at `crates/memory-merge-driver/tests/merge_driver_cli.rs:57` confirms the `secret sensitivity refused` stderr message.                                                                                                                                                                                 |
| Q10 | Bench fixture corpus (TempDir, Stream-E namespacing)               | **MET**                                   | `memory-test-support/src/perf.rs:39` uses `tempfile::TempDir::new()` (not PID paths). Placement table at line 43-55 covers `me/notes/`, `me/preferences/`, `projects/<3 projects>/decisions/`, `projects/<3 projects>/conventions/`, `agent/patterns/`, `dreams/<3 dated>/` — proportional to spec usage. `corpus_sha256` (line 114-130) deterministic walk.                                                                                                                                                                                                                                                                                            |
| Q11 | `.compacted.jsonl` deletion after replay                           | **MET**                                   | `runtime/reconcile.rs:301-305` deletes the queue file (no rename to `.compacted.jsonl`). Comment confirms Q11 intent.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Q12 | `runtime::blocking::run_blocking` deleted                          | **MET**                                   | `ls crates/memory-substrate/src/runtime/` shows only `mod.rs`, `faults.rs`, `reconcile.rs`. `grep -rn "run_blocking" crates/` returns zero hits. `runtime/mod.rs:4` documents the deletion.                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

Summary: **10 met (some partial), 2 violated, 1 met-with-spec-drift.**

---

## 3. Critical invariants (CLAUDE.md)

| #   | Invariant                                                                               | Result                                      | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| --- | --------------------------------------------------------------------------------------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `secret` never persisted to disk; `WriteFailureKind::SecretRefused` before disk effects | **MET**                                     | `api.rs:410-411` (encrypted path), `api.rs:941` (plaintext path). `error.rs:130-132` defines the variant. Refusal emitted before any `atomic_write` call.                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 2   | Every write request carries a `ClassificationOutcome`                                   | **MET (write/encrypted) / GAP (tombstone)** | `model.rs:526` non-optional `classification: ClassificationOutcome` on `WriteRequest`. Same on `EncryptedWriteRequest` (line 545). BUT `TombstoneRequest` (line 549-555) lacks `classification`. Tombstone writes mutate frontmatter and call `atomic_write` at `api.rs:626` with no Stream-D classification gate. Spec §8.5 isn't explicit on this, but the invariant says "every write request" — not "every plaintext write request".                                                                                                                                                                                        |
| 3   | Embedding triple is identity, not flavor; no silent fallback                            | **VIOLATED**                                | `tree/layout.rs:55-56` `bootstrap_repo_tree` writes `synthetic / stream-a-test / 32` to disk in every fresh repo. `api.rs:1231-1238` `write_initial_config_if_absent` does the same redundantly during `Substrate::init`. `index/query.rs:46-55` exposes a non-`#[cfg(test)]` `Index::new` that constructs the synthetic triple. The triple is no longer a runtime fallback — it's an on-disk default, baked into the canonical `config.yaml` and synced to peers. Worse than the original B-IX-1 finding.                                                                                                                      |
| 4   | Device IDs only in local runtime state (`local-device.yaml`)                            | **MET**                                     | `config/mod.rs:16-25` `SyncedConfig` has no device-id field. `LocalDeviceConfig` (line 36-46) is the only carrier. `git/adopt.rs:102-122` is the sole minter, writes atomically via tempfile.                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| 5   | Single `MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`                                          | **MET**                                     | `lib.rs:27` `pub const SUBSTRATE_SCHEMA_VERSION: u32 = 1;`. `merge/mod.rs:18` `pub use crate::SUBSTRATE_SCHEMA_VERSION as MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION;`. `frontmatter/schema.rs:5` re-exports same. `events/log.rs:18` `EVENT_SCHEMA_VERSION = crate::SUBSTRATE_SCHEMA_VERSION`. `INDEX_SUPPORTED_SCHEMA_VERSION` is a separate (correct) constant for the SQLite schema.                                                                                                                                                                                                                                             |
| 6   | Two-clone convergence is canonical-content equality                                     | **PARTIAL**                                 | `convergence.rs:55-67` implements canonical comparison. BUT (a) uses `serde_yaml::Value` re-serialize, not canonical serializer (see Q7); (b) `merge/body_diff3.rs` conflict markers don't normalize ours/theirs labels (see Q8); (c) merge `_merge_diagnostics` includes `merge_id: ulid::Ulid::new()` and `created_at: Utc::now()` per merge (`merge/quarantine.rs:114-117`), so two clones merging the same conflict produce different bytes — and the fuzz target explicitly SKIPS quarantine outputs (`fuzz/fuzz_targets/merge_swap_convergence.rs:30-35`) so this convergence break cannot be caught by the fuzz harness. |
| 7   | `bench/baseline.<profile>.json` only via human commits                                  | **MET**                                     | `bin/stream_a_bench.rs:103-113` `guard_baseline_path` refuses any output path matching `baseline.*.json`. Defense-in-depth aligns with `bench-gate.sh` (not read but referenced).                                                                                                                                                                                                                                                                                                                                                                                                                                               |

Summary: **5 met, 1 partial, 1 violated.** Invariant 3 is the headline — the synthetic triple writing to disk is straight-up worse than the runtime fallback the original review flagged.

---

## 4. Blockers

### F-01. Synthetic embedding triple baked into `config.yaml` on every fresh repo

`tree/layout.rs:55-56`:

```rust
if !root.join("config.yaml").exists() {
    std::fs::write(root.join("config.yaml"), "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\n")?;
}
```

And duplicated at `api.rs:1231-1238` (`write_initial_config_if_absent`).

**Severity:** BLOCKER (invariant 3, spec §10.2.2 #5 "no silent fallback").

**Why this is worse than the runtime fallback the original review flagged:** the original B-IX-1 was a constructor default. This is a synced-to-disk canonical config, written before any operator can intervene. A daemon's first `init` produces a `config.yaml` that says "embed everything against the synthetic provider". Stream B will write embeddings against this triple, drift from any real model, and the operator has to manually replace the file before any production embedding worker can take over.

**Fix shape:** delete `write_initial_config_if_absent` and the `config.yaml` write in `bootstrap_repo_tree`. Make `Substrate::open` fail with a typed `OpenError::ActiveEmbeddingTripleRequired` when `active_embedding` is absent. Tests pass an explicit triple via `InitOptions { active_embedding: EmbeddingTriple { ... } }` (TODO already exists at `api.rs:56-57`). Index tests/fixtures use `Index::with_active_embedding(connection, test_triple)` (already preferred) — gate `Index::new` behind `#[cfg(test)]` (the comment claiming integration tests need it doesn't justify production exposure — integration tests can construct the triple explicitly).

---

### F-02. Q2 wholly unfixed — no `spawn_blocking`, no `parking_lot`, no flume actor

`api.rs:7`: `use std::sync::{Arc, Mutex}`.
`api.rs:33`: `index: Arc<Mutex<Index>>`.
`api.rs:46-869`: every public method is `pub async fn` over blocking I/O.
`grep -rn "spawn_blocking\|parking_lot::Mutex\|flume" crates/`: zero hits.
`Cargo.toml`: no `parking_lot`, no `flume`.

The decision-doc verdict: "Every public method in `api.rs` wraps its blocking body in `tokio::task::spawn_blocking` against a configured pool... Locks become `parking_lot::Mutex` (no poisoning to swallow). Index handle moves to a dedicated single thread (per spec §16.5) accessed via channel."

**Severity:** BLOCKER.

**Concrete symptom for Stream B:** an MCP transport adapter running on a tokio current-thread runtime calling `substrate.read_memory(id).await` will block the runtime executor for the duration of a filesystem walk + parse pass (every memory in the repo, since `read_memory` does an O(n) walk per the legacy path at `api.rs:99-112`). Stream B ships, MCP connections deadlock under modest load, the post-mortem points back here.

**Fix shape:** as the decision specified.

1. Add `parking_lot = "0.12"` and `flume = "0.11"` to workspace deps.
2. `Substrate::index` becomes `flume::Sender<IndexMsg>` to a dedicated thread that owns the `rusqlite::Connection`. Dropping the substrate sends a shutdown msg.
3. Every public `async fn` body wraps the blocking work in `tokio::task::spawn_blocking`. Cancellation safety per spec §16.7.
4. `runtime/mod.rs` already documents the intent (`spawn_blocking` directly per Q2); follow through.

---

### F-03. Production code panics via `RepoPath::new` / `MemoryId::new` / `DeviceId::new` on dynamic input

The newtypes' `new()` constructors are documented as test/fixture-only and `expect("…")` on validation failure (`model.rs:664, 711, 757`). But production code uses them with dynamic inputs at ~25 sites:

- `api.rs:102, 222, 294, 324, 599, 908, 1064, 1191`: `RepoPath::new(path.to_string_lossy().replace('\\', "/"))` and `RepoPath::new(format!("agent/patterns/{}.md", ...))`.
- `api.rs:331, 524, 675`: `DeviceId::new("dev_unknown")` as fallback when `try_new` fails.
- `index/query.rs:187, 225, 706`: `MemoryId::new(row.get::<_, String>(0)?)` on string from SQLite.
- `markdown/atomic.rs:142`: `RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str()))`.
- `runtime/reconcile.rs:333`: `DeviceId::new("dev_startupplaceholder")`.
- `runtime/reconcile.rs:474`: `RepoPath::new(relative_str)`.
- `watcher/subscription.rs:131`: `RepoPath::new(relative)` inside the notify callback.

**Severity:** BLOCKER (panic-on-bad-input in production paths, including a recursive watcher callback).

**Concrete symptom:** the watcher recursively monitors the repo. `should_watch` filters `.git/`, `.DS_Store`, atomic temps, editor backups. ANY other file (e.g., `README.md` at repo root, an `.md` under a directory not in `MEMORY_PREFIXES`) panics the watcher's notify callback at line 131. The watcher dies silently; subsequent file events never deliver. Stream A appears healthy from the daemon's perspective until the first stale read.

Also a process-wide panic on a SQLite row whose `memory_id` doesn't validate (corrupted index, future schema migration mid-flight) — no graceful degradation.

**Fix shape:** every dynamic-input call site routes through `try_new` and propagates the error. For the watcher callback, treat `RepoPath::try_new` failure as "skip this event with a `tracing::warn!`". For SQLite-row hydration, `MemoryId::try_new` failure means corrupted index — emit `OperatorRepairRequired` event and return the error. The "test/fixture only" comment on `new()` should be enforced by `#[cfg(test)]` or `#[doc(hidden)]` + a separate `MemoryId::trusted_unchecked` for the SQLite-already-validated path.

---

### F-04. Sequence number persistence missing — `seq: 0` everywhere

Every event emission site hardcodes `seq: 0`:

- `api.rs:993` (`record_event`)
- `api.rs:681` (tombstone fallback)
- `runtime/reconcile.rs:334` (Phase 9 completion)

The Q5 decision was: "`seq` persists to `~/.memoryd/event-seq.json` under exclusive lock." The file is referenced nowhere; the runtime never allocates a sequence number.

**Severity:** BLOCKER (spec §12.4 multi-device union ordering by `(ts, device, seq, id)` becomes ambiguous when timestamps collide, which is realistic at sub-millisecond write rates; spec §12.5 acceptance signal "strictly increasing seq per device" outright fails).

**Concrete symptom for Stream I (live event subscriptions):** subscribers ordering events by `seq` ASC see arbitrary order when `ts` collides. Test fixtures that produce two events with `Utc::now()` rounded to the same millisecond (common in tight loops) will have non-deterministic ordering.

**Fix shape:** add `runtime/seq.rs` with an `EventSeqAllocator` that opens `<runtime>/event-seq.json` under `fs2::FileExt::lock_exclusive()`, reads `{ device_id: u64 }` map, returns next value, fsyncs after bump. `Substrate` carries an `Arc<EventSeqAllocator>`; every event emission allocates before `append_event`.

---

### F-05. Phases 7 and 8 of spec §13.5.1 startup reconciliation are missing

`runtime/reconcile.rs:168-175`:

```rust
phase_1_crash_recovery_scan(repo, runtime, &mut report)?;
phase_2_event_log_recovery(event_log, &mut report)?;
phase_3_replay_pending_index(repo, runtime, index, &mut report)?;
phase_4_replay_pending_encrypted_index(repo, runtime, index, &mut report)?;
phase_5_replay_pending_events(runtime, event_log, &mut report)?;
phase_6_index_consistency(repo, index, &mut report)?;
phase_9_emit_completion(event_log, &mut report)?;
```

Spec §13.5.1 mandates 9 phases. The orchestrator skips phase 7 (working-tree audit, classify untracked/modified files, quarantine to `<runtime>/quarantine/<startup-ts>/`) and phase 8 (auto-commit any uncommitted post-merge reconciliation work).

**Severity:** BLOCKER (spec acceptance signal §13.7 unreachable; B-RT-1 was supposed to land all 9 phases).

**Fix shape:** implement `phase_7_working_tree_audit` (call `git status --porcelain=v1 -z`, classify entries, route quarantine candidates to the runtime quarantine dir, set `report.operator_action_required` on unexpected modifications) and `phase_8_auto_commit_reconciliation` (call into `git::auto_commit` with explicit namespace allow-list). Wire them into the orchestrator between phase 6 and phase 9.

---

### F-06. `Substrate::open` runs `full_reindex_from_repo` unconditionally

`api.rs:892`:

```rust
full_reindex_from_repo(&roots.repo, &mut index)
    .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
```

This runs on every open. Phase 6 (`reindex_stale_memories`) exists in `runtime/reconcile.rs:316-322` and does the right thing — only reindex memories whose `body_hash` or `mtime` has drifted. But `Substrate::open` ignores it and rebuilds the world.

**Severity:** BLOCKER for the cost model the spec promises; spec §13.5.1 phase 7 wording "Index/file consistency. Only reindex files whose hash has drifted" is functionally bypassed even though `phase_6_index_consistency` exists.

**Concrete symptom:** every daemon restart pays full O(n) reindex cost on a healthy repo. Stream H's eval reproducibility (which times the open path) sees noise because `bench/baseline.*.json` cold-reindex measurements are sometimes "open + reindex unchanged" and sometimes "open + reindex first time".

**Fix shape:** delete the `full_reindex_from_repo` call at `api.rs:892`. `reconcile_startup_pre_index` already runs phase 1-2 before the index opens; `reconcile_startup_full` runs phase 6 after. The "always full reindex" call is redundant cost and inconsistent with phase semantics.

---

### F-07. Convergence-breaking: id-keyed array unions use last-insert-wins

`merge/field_rules.rs:468-484`:

```rust
fn merge_entities_id_keyed(ours: &[Entity], theirs: &[Entity]) -> Vec<Entity> {
    let mut by_id: BTreeMap<String, Entity> = BTreeMap::new();
    for entity in ours.iter().chain(theirs.iter()) {
        by_id.insert(entity.id.clone(), entity.clone());  // theirs overwrites ours
    }
    by_id.into_values().collect()
}
```

Same pattern in `merge_tombstone_events_id_keyed` (line 478-484). For `merge_evidence_id_keyed` (line 488-519), the duplicate-id path emits a near-duplicate diagnostic but `by_id` still keeps the FIRST insert (ours), and on `(theirs, ours)` swap, `by_id` keeps theirs.

**Severity:** BLOCKER (spec §13.6.1 canonical-content equality unreachable when entities/tombstone_events/evidence collide on id with different content; B-MG-9's "id-keyed union" was supposed to fix exactly this).

**Concrete symptom:** Two clones merge the same conflict pair `(ours, theirs)` and `(theirs, ours)`. They produce different `entities[]`, `tombstone_events[]`, `evidence[]` payloads when any id appears on both sides with different label/content. Two-clone convergence test (when content actually differs) returns false.

**Fix shape:** for each id-keyed union:

1. Sort both inputs by id.
2. Walk merged: when both sides have the same id with different content, deterministic resolution by stable stringification (`serde_json::to_string` of the row, take min/max) or by emitting both as separate entries with disambiguating suffixes plus a diagnostic.
3. The `merge_evidence_id_keyed` near-duplicate path's "find existing by secondary key" iteration also depends on insertion order — convert to a sort-then-merge pass.

The fuzz target should catch this. It doesn't because (see F-08) it skips quarantines and there's no fuzz target for clean merges with conflicting id-keyed entities. Add a focused proptest.

---

### F-08. Swap-order convergence fuzz target silently skips quarantine outputs

`fuzz/fuzz_targets/merge_swap_convergence.rs:30-35`:

```rust
(Ok(MergeResult::Quarantine(left)), Ok(MergeResult::Quarantine(right))) => {
    // Quarantine outputs include `merge_id` (ULID) and `created_at`
    // (now()) which intentionally differ across runs. Skip strict
    // equality; assert both outputs at least parse and quarantine.
    let _ = (left, right);
}
```

The fuzz target was specifically commissioned to catch B-MG-6 swap-order divergence. Quarantine outputs are exactly where divergence is most likely (because they go through more diagnostic accumulation paths). Skipping them with `let _ = (left, right);` defeats the purpose.

**Severity:** BLOCKER (the safety net the fix run was supposed to install is itself broken).

**Root cause for the skip:** `merge/quarantine.rs:114-117` — `merge_id: format!("merge_{}", ulid::Ulid::new())` and `created_at: Utc::now()`. Two clones merging the same conflict pair produce different ULIDs and timestamps, so byte-equality fails even when the conflict resolution is identical.

**Severity-for-spec:** This is itself a CONVERGENCE bug — spec §13.6.1 cannot reach a fixed point when two clones quarantine the same conflict, because their canonical bytes differ. It's the same root cause as the fuzz skip.

**Fix shape:** make `merge_id` deterministic from inputs (e.g. `format!("merge_{}", sha256({base, ours, theirs, path}))`); make `created_at` either omitted from canonical output or derived from `max(ours.updated_at, theirs.updated_at)`. Then remove the fuzz skip — both arms must assert `assert_eq!(left, right)`.

---

### F-09. `chunk_id` UNIQUE constraint vs spec §10.3 derivation

`index/schema.rs:61`: `chunk_id TEXT NOT NULL UNIQUE` (also enforced by `CREATE INDEX IF NOT EXISTS idx_chunks_chunk_id`).
`index/chunking.rs:262`: `chunk_id = format!("chk_{digest}")` where `digest = sha256(chunk_text)`.
`index/chunking.rs:361-364`: explicit test "identical_text_produces_identical_chunk_id".

Two memories that share an identical chunk text — e.g. boilerplate "## Context" sections, common license headers, identical short replies, repeated quotes from a ground-truth doc — collide on insert. The second `INSERT INTO memory_chunks` fails with `UNIQUE constraint failed`.

**Severity:** BLOCKER (writes break for any non-trivial repo with structural overlap; surfaced explicitly in the prompt as Phase 5's load-bearing finding).

**Fix shape:** spec § 10.3 reads "chunk_id derives from chunk text content" but spec §10.1's `chunk_id TEXT NOT NULL UNIQUE` is then over-constrained. Resolution paths:

- (Spec amendment) make derivation include `memory_id`: `chunk_id = chk_<sha256(memory_id || ":" || chunk_text)>`. Spec § 10.3 currently doesn't pin this and §14.2/§14.4/§16.4 don't depend on cross-memory chunk identity.
- (Schema amendment) drop the UNIQUE constraint, replace with `UNIQUE(memory_id, ordinal)` (which is already in spec §10.1 line 923 but missing from `index/schema.rs`).

The latter aligns code with spec; the former changes the chunk-id derivation contract Stream E may already plan against.

---

### F-10. `memory_chunks` table is missing 7 spec columns and the spec's UNIQUE constraint

`index/schema.rs:58-67`:

```sql
CREATE TABLE IF NOT EXISTS memory_chunks(
  chunk_rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  memory_id   TEXT NOT NULL,
  chunk_id    TEXT NOT NULL UNIQUE,
  body_hash   TEXT NOT NULL,
  text        TEXT NOT NULL,
  start_byte  INTEGER NOT NULL,
  end_byte    INTEGER NOT NULL,
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
```

Spec §10.1 lines 908-924 mandates additionally:

- `ordinal INTEGER NOT NULL`
- `chunk_text` (the spec's name; code uses `text`)
- `token_start INTEGER NOT NULL`
- `token_end INTEGER NOT NULL`
- `chunk_hash TEXT NOT NULL` (spec name; code uses `body_hash`)
- `summary TEXT NOT NULL`
- `indexable INTEGER NOT NULL`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`
- `UNIQUE(memory_id, ordinal)`
- 3 indexes: `idx_chunks_memory`, `idx_chunks_indexable`, `idx_chunks_chunk_id`

Code has only 2 of 3 indexes; missing `idx_chunks_indexable`. The FTS triggers also reference `chunk_text` and `summary` columns the table lacks.

**Severity:** BLOCKER (Q1 says "fix to spec §10.1 now, not negotiable" — this part of Q1 was missed for `memory_chunks` even though the `memories` table got fixed).

**Fix shape:** align `memory_chunks` to the full spec schema. Update `chunking.rs::Chunk` to populate the new columns (ordinal, token_start/end, summary, indexable, created_at, updated_at). Triggers already reference `chunk_text` and `summary` (lines 70, 73, 77) — these will fail at runtime because the columns don't exist. (How are tests passing? Likely because the FTS5 trigger inserts `text` (the actual column name) but the trigger SQL says `chunk_text`. Sqlite would reject the trigger creation. Worth confirming the test suite actually exercises this path.)

Wait — re-reading line 68-70:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS memory_chunks_fts USING fts5(text, content='memory_chunks', content_rowid='chunk_rowid');
CREATE TRIGGER IF NOT EXISTS memory_chunks_ai AFTER INSERT ON memory_chunks BEGIN
  INSERT INTO memory_chunks_fts(rowid, text) VALUES (new.chunk_rowid, new.text);
END;
```

OK — code uses `text` (singular) consistently; FTS5 column is `text`. Spec uses `chunk_text` and `summary` (two FTS columns). So the FTS5 schema is also a divergence: spec `fts5(chunk_text, summary, ...)` vs code `fts5(text, ...)`. This affects search ranking and Stream E's recall block assembly.

**Combined severity:** BLOCKER for spec contract; current code compiles and "works" because it's internally consistent, but it doesn't match what Stream E will read.

---

### F-11. `memory_evidence` schema diverges from spec §10.1

`index/schema.rs:162-172`:

```sql
CREATE TABLE IF NOT EXISTS memory_evidence(
  memory_id       TEXT NOT NULL,
  evidence_id     TEXT NOT NULL,
  quote           TEXT NOT NULL,
  quote_norm_hash TEXT,                  -- spec: NOT NULL
  ref_text        TEXT NOT NULL,         -- spec: ref TEXT NOT NULL
  weight          REAL NOT NULL DEFAULT 1.0,
  observed_at     TEXT,
  -- missing: source TEXT
  PRIMARY KEY(memory_id, evidence_id),
  ...
);
```

Spec §10.1 lines 1007-1019 mandates `quote_norm_hash TEXT NOT NULL`, column name `ref` (not `ref_text`), and a `source TEXT` column. Also missing the spec's `idx_evidence_ref` and `idx_evidence_hash_ref` indexes.

**Severity:** BLOCKER (spec contract; Stream C's grounding/contradiction-detection logic will query these).

**Fix shape:** align column names and constraints. Note: `ref` is a SQL reserved word in some dialects but is fine in SQLite (it's a regular keyword) — the rename to `ref_text` was unnecessary defensive coding.

---

### F-12. Watcher panic surface in production path

`watcher/subscription.rs:131`:

```rust
let repo_path = RepoPath::new(relative);
```

Inside the notify callback. `should_watch` filters `.git/`, `.DS_Store`, atomic temps, editor backups — but does NOT filter paths outside the spec § 5.1 allow-list. A user creating `notes-2026.md` at the repo root, or running `cp something.md somewhere-not-in-allowlist/`, will trigger this code path with a path `RepoPath::try_new` rejects, panicking the watcher thread.

**Severity:** BLOCKER (silent watcher death; no graceful degradation).

**Fix shape:** route through `RepoPath::try_new` and skip the event with `tracing::debug!("watcher: skipping path outside Stream A allow-list: {}", relative)`. Already documented in F-03 as part of the broader newtype-panic pattern; this site is the highest-risk because it's in an async callback where panics don't propagate to the daemon's error handling.

---

## 5. Risks

### F-13. Tombstone path bypasses `ClassificationOutcome` gate

`api.rs:589-695`: `tombstone_memory(request: TombstoneRequest)`. `TombstoneRequest` (model.rs:549-555) carries `id: MemoryId, reason: String` — no `classification`. The body-mutation at `api.rs:600-621` rewrites frontmatter and writes via `atomic_write` without the classification refusal gates `write_memory`/`write_encrypted` apply.

A Stream D bug that classifies a tombstone payload as `Secret` would never refuse — `tombstone_memory` doesn't ask. Defense-in-depth gap.

**Severity:** RISK (spec §8.5 doesn't explicitly mandate classification on delete, but the invariant says "every write request").

**Fix shape:** add `classification: ClassificationOutcome` to `TombstoneRequest`; route through the same `enforce_plaintext_classification` gate.

---

### F-14. `roots_converged` uses `serde_yaml::to_string(&Value)` not `frontmatter::canonical_serialize`

`memory-test-support/src/convergence.rs:179-180`:

```rust
let value: serde_yaml::Value = serde_yaml::from_str(frontmatter_yaml).context("frontmatter parse")?;
let re_serialized = serde_yaml::to_string(&value).context("frontmatter serialize")?;
```

The Q7 decision named `frontmatter::canonical_serialize` specifically. `serde_yaml::Value` round-trip preserves no information about quoting decisions (whether `null` was written as `null` vs `"null"`, whether `0x10` was a string vs a hex int) — but the canonical serializer at `frontmatter/serialize.rs:104-119` quotes YAML reserved literals (R-FT-3 fix). Two clones can disagree on quoting and `roots_converged` declares them equal because `serde_yaml::Value` parses both to the same scalar.

**Severity:** RISK (false positive on convergence test; passes badge that shouldn't).

**Fix shape:** import `memory_substrate::frontmatter::canonical_serialize` from `memory-test-support` (cyclic dep concern is real — restructure or feature-gate). Or inline the canonical serializer's quoting rules.

---

### F-15. `_merge_diagnostics` regenerates `merge_id`/`created_at` per merge → forced quarantine divergence

`merge/quarantine.rs:113-117`:

```rust
pub(super) fn fresh_diagnostic(status: MergeStatus, human_reason: impl Into<String>) -> MergeDiagnostic {
    MergeDiagnostic {
        merge_id: format!("merge_{}", ulid::Ulid::new()),
        created_at: Utc::now(),
        ...
```

Two clones merging the same conflict pair produce different `merge_id` and `created_at` values — guaranteeing the canonical bytes differ. F-08 is the fuzz-skip that hides this; here's the upstream issue.

**Severity:** RISK (root cause of F-08; fix here lets F-08's fuzz target actually do its job).

**Fix shape:** derive `merge_id` from a deterministic hash of the conflict inputs (e.g. `merge_<sha256(base || "\0" || ours || "\0" || theirs || "\0" || path)>`). For `created_at`, either drop from the diagnostic (status + reason are enough for spec §6.10) or use `max(ours.updated_at, theirs.updated_at)` so two clones merging the same payload converge.

---

### F-16. `update_embedding` validates dimension via active triple, but `Index::with_active_embedding` is mutable per-instance

`index/query.rs:33-35`: `with_active_embedding` is a single-call constructor. The `active_embedding` field is read on every `update_embedding` for dimension validation. But Stream B's dimension migration (spec §10.2.2) requires changing the active triple at runtime — the current shape requires reconstructing the entire `Index` (and SQLite connection) to swap triples.

**Severity:** RISK (Stream B's `change_active_embedding` API mentioned in spec §16.4 is unimplementable without a setter).

**Fix shape:** add `Index::set_active_embedding(&mut self, triple: EmbeddingTriple) -> Result<(), VectorError>` that validates the triple is registered in `chunk_embedding_meta` (or matches a known dimension), updates the field, and emits an `EmbeddingModelChanged` event.

---

### F-17. Conflict markers in `merge_body_diff3` emit FULL bodies, not the conflicting hunks

`merge/body_diff3.rs:123-129`:

```rust
fn format_conflict_markers(ours: &str, theirs: &str) -> String {
    format!(
        "<<<<<<< ours\n{}=======\n{}>>>>>>> theirs\n",
        ensure_trailing_newline(ours),
        ensure_trailing_newline(theirs),
    )
}
```

A 1-line conflict in a 1000-line memory results in a quarantine output containing two full 1000-line bodies between `<<<<<<<` and `>>>>>>>`. The base context is also lost — diff3-style markers should preserve `|||||||\n<base>\n` for operator review.

**Severity:** RISK (operator UX; quarantine review impossible at scale; recall-assembly cost for the merged document is 2× input).

**Fix shape:** use `imara-diff`'s hunk-level conflict regions to emit markers around just the disjoint conflict spans, with shared context restored from `base`. Add a fixture test asserting the conflict marker only spans the actual divergence, not the whole document.

---

### F-18. Phase 5 deferrals confirmed (per the prompt's expectations)

- **B-API-2:** `query_memory`/`query_chunks` still return `Vec<QueryResult>` and `Vec<ChunkResult>` (`api.rs:758, 763`). `MemoryHit`/`ChunkHit` types exist in `model.rs:971-993` but are not wired into the public API. Confirmed deferred.
- **B-API-3 / Q2:** see F-02. Confirmed deferred (and decision-doc violation).
- **B-API-8:** `EventKind` (`events/log.rs:54-96`) has 9 variants. Spec §12.2 mandates 24. TODO comment at line 90-95 lists the missing 15. Confirmed deferred.
- **R-API-1..R-API-7:** various — no helper extraction (R-API-1), no streaming `events()` (R-API-7), `MemoryQuery` still thin (R-API-5), `ChunkQuery` still allows invalid combinations (R-API-6). Confirmed deferred.

These are documented as deferred. The risk is that the deferral list keeps growing and the public API ships under-spec.

---

### F-19. `memory-test-support` is a non-dev dependency of `memory-substrate`

`crates/memory-substrate/Cargo.toml:37-38`:

```toml
[dependencies.memory-test-support]
path = "../memory-test-support"
```

Should be `[dev-dependencies]`. `memory-test-support` is otherwise a fine perf/convergence helper crate, but linking it into the production `memory-substrate` binary pulls in `walkdir`, `tempfile`, `sha2` (already there), and exposes test fixtures to consumers. Review the call chain:

`bin/stream_a_bench.rs:16: use memory_test_support::perf::{corpus_sha256, synthetic_vector};`

The bench binary is the only caller. Move it to `[dev-dependencies]` and gate the bench binary behind `#[cfg(any(test, bench))]` or `[[bin]] required-features = [...]`.

**Severity:** RISK (production binary bloat; supply-chain surface).

---

### F-20. `Index::new` synthetic fallback constructor still public

`index/query.rs:46-55`: `pub fn new(connection: Connection) -> Self` constructs `Index` with the synthetic triple. Comment at line 43-44 explicitly says "Exposed without `#[cfg(test)]` so integration tests (which compile as separate crates) can construct an `Index` without a `config.yaml`."

The decision doc said: "Either gate `Index::new` `#[cfg(test)]` or require explicit triple."

The "integration tests need it" justification doesn't hold — integration tests can construct the triple explicitly via `EmbeddingTriple { ... }`. `with_active_embedding` is a one-line ctor; the fallback adds a runtime-fallback surface that other production callers can stumble into. The B-IX-1 fix is incomplete.

**Severity:** RISK (production-callable silent fallback to synthetic triple).

**Fix shape:** gate `Index::new` behind `#[cfg(any(test, feature = "test-fixtures"))]`. Update integration tests to call `Index::with_active_embedding(connection, EmbeddingTriple::synthetic_for_tests())`. Add a single `EmbeddingTriple::synthetic_for_tests()` constant under `#[cfg(test)]` to centralize the test triple.

---

### F-21. `tree::TreeValidationMode` only has two modes; spec §5.4 mandates `StartupPreflight`

`tree/validate.rs:25-32`: `enum TreeValidationMode { PartialSync, FullySynced }`. Spec §5.4 mentions a third mode that additionally checks local git merge-driver config presence (used by `Substrate::open` reconciliation). The original B-FT-3 finding flagged this.

**Severity:** RISK (spec acceptance signal §5.5 unfulfilled).

**Fix shape:** either add `StartupPreflight` mode that does the additional `git config --get merge.<driver>.driver` check, or — cleaner — keep this enum as-is and have `Substrate::open` orchestrate the merge-driver check separately (the latter is what `git/preflight.rs` already does via `git_preflight`). Pick one and remove the spec ambiguity.

---

### F-22. Watcher `should_suppress` reads file bytes from disk every event

`watcher/subscription.rs:132-135`:

```rust
let Ok(bytes) = std::fs::read(path) else {
    return false;
};
let hash = crate::markdown::hash_bytes(&bytes);
```

For every file event the watcher fires, the suppression check reads the entire file from disk to compute its hash. For a bulk `git pull` or large rsync, that's O(n) reads inside the watcher callback, blocking subsequent events.

**Severity:** RISK (performance under sync-heavy workloads; spec §11.1 doesn't specify, but realistic).

**Fix shape:** suppression keyed by `(repo_path, expected_hash)` pre-registered by atomic-write at write time, not recomputed on read in the watcher callback. The current shape is structurally backward.

---

### F-23. `serde_yaml::to_string` deprecated; will hit MSRV churn

`crates/memory-substrate/Cargo.toml:24` `serde_yaml = "0.9"` is the upstream-archived version (Apr 2024 archive notice). No drop-in replacement, but the spec's "canonical YAML" output relies on this crate; choosing now avoids a forced migration mid-stream.

**Severity:** RISK (supply-chain).

**Fix shape:** evaluate `serde_yaml_ng` (active fork) or `serde_yml` against the fixture round-trip tests. Pin in workspace deps.

---

## 6. Nits

### F-24. `phase_9_emit_completion` uses `DeviceId::new("dev_startupplaceholder")`

`runtime/reconcile.rs:333`: hardcoded fake device id. The completion event then carries this in the multi-device union, polluting any union view with a phantom device.

**Fix shape:** thread the real device id through `reconcile_startup_full(... device_id: &DeviceId, ...)` from `Substrate::open`.

### F-25. `bootstrap_repo_tree` writes `.gitignore` line `/.*.tmp` which doesn't match the atomic-write temp pattern

`tree/layout.rs:53`: `*.sqlite-wal\n*.sqlite-shm\n/.*.tmp\n`. The atomic-write temp pattern at `markdown/atomic.rs` is `.<basename>.<op_id>.tmp` — basename comes after the leading dot. The gitignore line `/.*.tmp` matches files whose basename starts with `.`, which is correct. NIT only because the `/` prefix means "root-level only" — atomic temps live alongside their target, anywhere in the tree. Should be `**/.*.tmp` or just `.*.tmp` to match anywhere. (NIT — atomic-write probably succeeds in moving before the watcher cycles, so the gitignore is belt-not-suspenders.)

### F-26. `EVENT_SCHEMA_VERSION` constant exists but `record_event` uses `crate::SUBSTRATE_SCHEMA_VERSION` directly

`api.rs:989, 677`. Inconsistent — pick one. (NIT)

### F-27. `corpus_sha256` walks twice (collect + sort + read)

`memory-test-support/src/perf.rs:114-130`. The `walkdir::WalkDir::sort_by_file_name()` is already deterministic; the explicit `paths.sort()` after collect is redundant. (NIT — perf irrelevant.)

### F-28. `merge_evidence_id_keyed` `secondary_key` returns `(quote_norm_hash, ref)` even when both are empty

`merge/field_rules.rs:521-524`. Workaround at line 507 checks `!(key.0.is_empty() && key.1.is_empty())` — but the workaround masks a deeper issue: for evidence with no quote_norm_hash and no ref, every distinct evidence on theirs side becomes its own near-duplicate of every distinct evidence on ours side. Fall-through is correct only because the body of the `if` insert path is gated by the empty check. Brittle. (NIT — works for now.)

### F-29. `body_diff3.rs` has no test for inline-conflict-region preservation

The current tests cover clean (3 cases) and overlapping (1 case). No fixture asserting that a single-line conflict in a multi-page body produces a quarantine output that's smaller than 2× the body. (NIT — test gap that masks F-17.)

---

## 7. Process notes

1. **The fuzz target was the safety net; it has a structural gap.** The `merge_swap_convergence` target was specifically commissioned to catch B-MG-6 swap-order divergence. It does the right thing for clean merges, then gives up at the quarantine arm because of the non-deterministic `merge_id`/`created_at` (F-15). The next fix run should fix F-15 first, then remove the fuzz skip — the fuzz target with the skip is worse than no target because it telegraphs "we have coverage" while the actual coverage is missing.

2. **"Test/fixture only" is not enforced by the type system.** ~25 production sites call `MemoryId::new`/`RepoPath::new`/`DeviceId::new` (F-03) on dynamic input. The newtype design says "production uses `try_new`" but Rust's visibility rules don't enforce it; doc comments don't compile. Either gate `new()` behind `#[cfg(any(test, feature = "test-fixtures"))]` and accept the fallout (every dynamic-input site becomes a `try_new` + propagation refactor), or rename `new()` → `from_unchecked_for_tests()` and let the name carry the warning. The current "polite suggestion" approach is the same anti-pattern as the original B-API-11 finding.

3. **Initial `config.yaml` writing on disk is a worse bug than the runtime fallback.** F-01 deserves first-priority on the next fix run. The path `bootstrap_repo_tree → write config.yaml with synthetic triple` was added during the buildout (Codex's call) but it inverts the spec's intent — the synthetic triple should never reach disk. The fix is "delete the lines"; the cost is that `Substrate::init` now requires an explicit `active_embedding` (which `InitOptions` should carry).

4. **Spec drift is real and tracked nowhere.** Q3 says "spec amendment to §13.1 to read `memory-merge-driver`" — never landed. Q9 spec language about `Sensitivity::Secret` was supposed to get explicit endorsement of "no Secret variant". F-09 chunk-id may need spec amendment (UNIQUE vs derivation). The spec is at v1.1 still; the buildout has accumulated 4-5 implicit deltas the next reviewer would have to dig out from decision docs and code archaeology. Bump the spec to v1.2 with explicit revision-goal entries before the next implementation phase.

5. **`async fn` over `std::sync::Mutex` is the same anti-pattern as the original review's critique** — the buildout did not address it. Phase 5 (per the prompt) explicitly deferred it. That's a defensible call IF Stream B's runtime constraints are later honoured. If they aren't, every Stream A consumer will need a wrapper layer that re-introduces `spawn_blocking` properly. Cheaper to fix it once at the source.

6. **`memory-test-support` as non-dev dep** (F-19) is the kind of gardening miss the next fix run should sweep up. Not load-bearing on its own; symptomatic of "ship now, prune later" pressure. A 5-line `Cargo.toml` change.

---

## Summary of severity counts

- **Blockers:** 12 (F-01 through F-12) — invariant violation, decision violation, panic surface in production, convergence break, schema gap, missing reconciliation phases.
- **Risks:** 11 (F-13 through F-23) — gaps that will bite under load or in Stream B integration.
- **Nits:** 6 (F-24 through F-29) — style/edge cases.

The decision-doc compliance table shows 10 of 12 commitments met (Q2 outright violated, Q5 partial via missing seq persistence). The CLAUDE.md invariant table shows 5 of 7 met, 1 partial (convergence), 1 violated (synthetic triple on disk).

Stream A is closer to spec than the original review found, but **not ready to support Stream B–I dependencies as-is.** Most blockers are local fixes (delete the synthetic-config-write, add seq persistence, route newtype callers through `try_new`, populate Phase 7+8); F-02 (async surface) and F-09/F-10 (schema gaps for chunks/evidence) are the heaviest. Recommend one more focused fix pass before the system-spec integration phase.
