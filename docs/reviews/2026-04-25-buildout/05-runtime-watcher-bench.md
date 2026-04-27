# Clean-code review — runtime / watcher / bench / test-support

Reviewer: reviewer-runtime
Files reviewed:

- `crates/memory-substrate/src/runtime/mod.rs`
- `crates/memory-substrate/src/runtime/blocking.rs`
- `crates/memory-substrate/src/runtime/faults.rs`
- `crates/memory-substrate/src/runtime/reconcile.rs`
- `crates/memory-substrate/src/watcher/mod.rs`
- `crates/memory-substrate/src/watcher/filter.rs`
- `crates/memory-substrate/src/watcher/suppression.rs`
- `crates/memory-substrate/src/watcher/subscription.rs`
- `crates/memory-substrate/src/bin/stream_a_bench.rs`
- `crates/memory-test-support/src/lib.rs`
- `crates/memory-test-support/src/convergence.rs`
- `crates/memory-test-support/src/perf.rs`
- `crates/memory-test-support/src/bin/rust_boundary_check.rs`

Spec consulted: `docs/specs/stream-a-core-substrate-v1.1.md` (§§7, 10.4, 11, 13.5.1, 13.6.1, 17.6, 18.9). Also cross-checked the `Substrate::open` call site in `api.rs:613–641` and the bench shell wrapper `scripts/bench-gate.sh`.

## Blockers

### B1 — `Substrate::open` does not implement spec §13.5.1 startup reconciliation

- `runtime/reconcile.rs:119–130` (`reconcile_startup`) and `api.rs:613–641` (`open_with_options`) collectively cover only phases 4, 5, 6 of the spec contract. Missing or wrong:
  - **Phase 1 — crash-recovery scan.** No code reads `~/.memoryd/startup-reconcile.required` or `<repo>/.git/MERGE_HEAD`. The marker is written by `write_startup_marker` from seven sites in `api.rs` but is never read or cleared. Stale markers accumulate forever; new startups don't gate on them.
  - **Phase 2 — working-tree audit.** No `git status --porcelain=v1 -z` classification, no quarantine of invalid/conflicted/unknown paths to `~/.memoryd/quarantine/<startup-ts>/`, no `OperatorRepairRequired` outcome surfaced from the audit. Unknown `.gitattributes` / dirty policies / files with conflict markers are not detected at startup.
  - **Phase 7 — index/file consistency check.** `full_reindex_from_repo` (api.rs:827) clears and rebuilds the entire plaintext index unconditionally. The spec's hash-comparison-then-enqueue path is never exercised; this is also the wrong cost model for healthy startups (reindexing 10K memories every open).
  - **Phase 8 — auto-commit any post-merge reconciliation work that was never committed.** Not implemented.
  - **Phase 9 — emit `StartupReconciliationCompleted`.** Two completion events get emitted (once at `reconcile.rs:127` and conditionally again at `reconcile.rs:196`) and neither carries the spec fields `phases_run`, `vector_repairs`, `event_repairs`, `pending_index_replays`, `operator_action_required`. The first event also reports `reindexed: 0` even though `full_reindex_from_repo` later reindexes the entire repo — the emitted count is structurally wrong.
- Spec §13.5.1 final paragraph: _"Substrate must not return from open until startup reconciliation completes or returns an explicit operator-required error. There is no path where Stream B begins serving writes against an unreconciled substrate."_ This contract is not met.
- Fix shape: collapse `reconcile_startup` + `replay_pending_repairs` + `full_reindex_from_repo` into a single `reconcile_startup(repo, runtime, event_log, &mut index) -> ReconcileReport` that runs the nine phases in order, emits one `StartupReconciliationCompleted` at the end carrying every required field, and clears `startup-reconcile.required` only on success. Add the missing phase implementations or, if some are deferred, write explicit `unimplemented!`/typed-error markers and surface them to `OpenError::OperatorRepairRequired` so reviewers can see what's missing.

### B2 — `replay_pending_repairs` is not the §10.4/§13.5.1 idempotent replay the spec describes

- `runtime/reconcile.rs:133–208`. Specific issues:
  - Spec §13.5.1 step 5 mandates per-op `PendingIndexReplayed` events. Spec step 6 mandates per-op `PendingEventReplayed`. Neither is emitted. The single combined `StartupReconciliationCompleted` at line 196 conflates `replayed_pending_events` with `repaired_events` and loses the per-op audit trail.
  - Encrypted-index ops (line 156): on hash mismatch the function returns `std::io::Error::other` (line 166), which `api.rs:630` maps to `OpenError::OperatorRepairRequired`. A single corrupt encrypted op kills startup with a generic error before any other pending repair runs. Spec §13.5.1 phase 2 sends suspect content to `~/.memoryd/quarantine/`; it does not bail out of reconciliation.
  - Encrypted-index ops never appear in any `remaining` list. After a successful replay, line 194 calls `compact_pending_file` which **renames** the file to `.compacted.jsonl` rather than deleting it. The compacted file is now orphaned on disk forever; nothing ever cleans it up.
  - Conditional at line 190–193 conflates three independent triggers (`replayed_pending_events > 0`, events file exists, encrypted-ops file exists). If only the encrypted-ops file existed at start (replayed cleanly), the code still emits a completion event whose `repaired_events: counts.replayed_pending_events` is `0`, claiming nothing happened. The event is a lie.
- Fix shape: keep a `RemainingOps` collection per queue type, distinguish "replayed" from "deferred for hash mismatch" from "quarantined for corruption", and emit per-op events as the spec mandates. Compaction should `remove_file` not rename; if rename is intentional for forensics, document why and add a janitor that deletes `.compacted.jsonl` after N startups.

### B3 — Bench harness uses its own xorshift instead of the spec-sanctioned `memory-test-support::perf::synthetic_vectors`

- `bin/stream_a_bench.rs:316–331` defines `synthetic_vector` with xorshift state mixing.
- `memory-test-support/src/perf.rs:30–38` defines `synthetic_vector` with `StdRng::seed_from_u64(seed ^ index)` and `gen_range(-1.0..1.0)`.
- These produce **different** vectors for the same `(seed, dimension, index)` tuple.
- Spec §17.6 corpus-and-vector-provenance: _"deterministic synthetic vectors generated by `memory-test-support::perf::synthetic_vectors(seed, dimension, n)`"_. Spec §18 boilerplate item 13: _"`memory-test-support::perf::synthetic_vectors` is the **sanctioned source**."_
- This is also why the bench's claim of "reproducible across machines" via its own seed is wrong — anything else in the codebase that uses the test-support helper to assert on the corpus will see a different corpus than the bench harness produced.
- Fix: delete the inline `synthetic_vector` in `stream_a_bench.rs` and call `memory_test_support::perf::synthetic_vector(seed, dimension, index)` (or expose a `synthetic_vector` helper alongside the existing `synthetic_vectors`).

### B4 — Bench output JSON omits the spec-required corpus identity hash

- `bin/stream_a_bench.rs:129–145` writes a JSON report with `seed`, `tier`, `profile`, runs, and metrics, but no `corpus_sha256`.
- Spec §17.6: _"The seed and the SHA256 of the materialized corpus are recorded in `bench/results.json` so a perf regression can be confirmed against an identical corpus."_
- `memory-test-support::perf::corpus_sha256` exists for exactly this purpose and is not called.
- Fix: build the corpus once via the test-support helper, hash it via `corpus_sha256`, and emit `"corpus_sha256": ..., "vector_dimension": ..., "active_triple": ...` in the report. Without this, no downstream consumer can verify regressions ran against the canonical corpus.

### B5 — `WatchSubscription` cannot deliver every event class the spec mandates

- `watcher/subscription.rs:75–87`: the `notify` callback handles only `Ok(event)` and silently drops `Err`. Notify v8.0 emits errors when the OS event queue overflows (FSEvents `kFSEventStreamEventFlagMustScanSubDirs`, inotify `IN_Q_OVERFLOW`); these arrive as either `Err(notify::Error)` or as `Event` with `EventKind::Other` flags. No code path constructs a `WatchEventKind::RescanRequired` event.
- `WatchEventKind::RescanRequired` and `FileEvent::rescan_required` (lines 26–37) are dead code; only `tests/watcher_lifecycle.rs:39–41` references them, and that test only exercises the constructor — it does not assert that the watcher emits one on overflow.
- Spec §11.1 lists `RescanRequired { reason: WatcherOverflowReason }` as a required event variant. Spec §11.4: _"Watcher overflow emits RescanRequired and a reindex converges."_ This signal is not delivered.
- Fix: branch on `notify::Event::need_rescan()` (notify 6+) / `Err(_)` and emit `FileEvent::rescan_required(root)` once per overflow batch. Also stop discarding `Err`; either log via `tracing` or surface as a `WatchEventKind::Error(String)` so subscribers learn the watcher itself is degraded.

### B6 — Watcher does not apply the §11.2 path filters (`is_memory_path` is exported but never used in the subscription)

- `watcher/subscription.rs:75–84` calls neither `is_memory_path` nor any other filter. Every path under the repo root is forwarded.
- `watcher/filter.rs:6–8` (`is_memory_path`) is `pub use`-d from `mod.rs:7`, but the only call site in the codebase is `api.rs:776`, which uses it for an unrelated rename validation.
- Spec §11.2: _"Watch the repo root recursively, **excluding `.git/`, editor backups, `.DS_Store`, and temp files** matching Stream A's same-directory temp pattern. Do not watch `~/.memoryd/`."_ (`~/.memoryd/` is outside the repo so probably fine; the in-repo exclusions are not.)
- Effect: subscribers receive `.git/index`, `.git/HEAD`, `.DS_Store`, atomic-write `.tmp.<op_id>` files, editor `~`/`.swp` files. Self-event suppression catches the temp files only when they hash to a tracked content hash, which is essentially never. A daemon that drives reindex off the watcher will reindex on `git fetch` because every change to `.git/refs/...` shows up.
- Fix: introduce a `should_watch(path: &Path)` predicate (covers `.git/`, `.DS_Store`, editor backups, the atomic-write temp prefix) and call it before sending. The current `is_memory_path` is too narrow (excludes `.gitattributes`, `config.yaml`, `policies/**`); a separate "is repo content I care about" predicate is the right shape.

### B7 — `runtime::blocking::run_blocking` is dead code; substrate methods do blocking I/O on async tasks

- `runtime/blocking.rs:4–10` exposes `run_blocking` wrapping `tokio::task::spawn_blocking`.
- Grep finds zero call sites in the substrate crate.
- `Substrate::reindex` (api.rs:522), `query_memory` (535), `query_chunks` (540) are `async fn` that synchronously hold a `std::sync::Mutex` and run `rusqlite` I/O directly on the calling task.
- Either the discipline applies (and these methods are wrong) or it doesn't (and `run_blocking` is misleading scaffolding). Brief invariant 4 calls this out explicitly.
- Fix shape: pick one. If async-friendly is the goal, every public async method that touches SQLite or files needs to wrap its body in `run_blocking`, and the `Mutex` should be `tokio::sync::Mutex` (or a sharded blocking-pool task that owns the connection). If the discipline is _not_ the goal, delete `runtime::blocking` entirely and stop pretending. As written, this is a placeholder masquerading as a guarantee.

## Risks

### R1 — `convergence::roots_converged` does not implement the §13.6.1 spec definition

- `memory-test-support/src/convergence.rs:33–35`: walks both roots, compares raw bytes, ignoring `.git`, `.memoryd`, `target`. That's it.
- Spec §13.6.1 requires: byte equality only for tracked Markdown, `.gitattributes`, `.gitignore`, `config.yaml`, `policies/**`; **set equality by event id** for `events/<device-id>.jsonl` (line order is not canonical); set-by-id normalization for `tombstones/**` and `substrate/**/*.jsonl`; structural equality for `_merge_diagnostics.add_add_alternates[]`.
- The current helper would declare two semantically-convergent clones non-convergent if their event JSONL line order differs. It would also compare `events/<other-device>.jsonl` files raw-byte rather than by event-id set.
- No call sites yet — `roots_converged` is unused — so this is a Risk, not a Blocker. But the helper's name is a contract: a future test that trusts it will get false negatives or false positives depending on which way the canonical-vs-byte mismatch cuts.
- Fix: either implement the §13.6.1 normalization (parse JSONL, set-equality by id) or rename to `roots_byte_equal` and document that it is a strictly weaker check than spec convergence.

### R2 — `read_framed_jsonl` silently truncates pending-repair queue files

- `runtime/reconcile.rs:225–261`. If a pending repair queue ends with a single malformed unterminated line, the function `set_len`s the file to drop it and proceeds.
- Spec §12.3 step 5 grants this trailing-line-truncation behaviour to the **event log**, not to pending repair queues. Pending queues are durable repair markers; truncating them silently masks crash-after-partial-write of a repair entry — exactly the case where reviewer attention is required.
- Recommendation: move the trailing-truncation behaviour out of the generic `read_framed_jsonl` and into the event-log code that owns it. Pending queues with malformed frames should surface `OpenError::OperatorRepairRequired` with the queue name and line offset.

### R3 — Bench `Fixture` build can race with itself across runs and creates unrealistic file layout

- `bin/stream_a_bench.rs:160`: `temp_dir().join(format!("stream-a-bench-{}-{seed:x}", std::process::id()))`. PID is reused on Linux/macOS over time; two parallel CI shards on the same runner could collide. The fixture then `remove_dir_all` the dir before re-creating, which is destructive against any concurrent run that landed on the same PID+seed.
- Lines 211 and 271: every memory in the corpus lives at `agent/patterns/{id}.md`. 10K files in one directory is not the realistic Stream B layout (the spec assumes namespaced subdirs) and may inflate or deflate cold-reindex p95 vs a more realistic distribution.
- Recommendation: use `tempfile::TempDir` (auto-cleanup, per-process unique). Vary the namespace per memory (e.g. round-robin across `agent/patterns/`, `user/preferences/`, `system/...`) so the cold-reindex corpus matches the layout Stream E will see.

### R4 — Bench `run()` is a 130-line god function mixing arg parsing, fixture build, six measurement loops, and report write

- `bin/stream_a_bench.rs:22–150`. Clean-code: this should be three or four functions.
  - `parse_args(env::args()) -> BenchArgs` (arg surface).
  - `run_iterations(&fixture, runs, seed) -> Metrics` (the six loops, currently inline).
  - `write_report(&Metrics, &args, &output)` (JSON assembly + write).
- The main loop body at lines 64–124 is also six near-identical shapes (`vec.push(measure_async(|| async { ... }).await?)`). A small helper `bench_step!` macro or a `for (label, builder) in &steps` loop would dedupe. Right now adding a seventh metric means cloning a 10-line block.
- Recommendation: extract a `BenchArgs` struct with `BenchArgs::parse() -> Result<Self, String>`, give `Fixture` a `measure_all(&self, runs, seed) -> Metrics` method, and let `main` orchestrate.

### R5 — Bench emits `noise_floor_ms` synthesized from the run's own p95

- `bin/stream_a_bench.rs:304`: `"noise_floor_ms": 2.0_f64.max(p95 * 0.50)`.
- Spec §17.6: _"`noise_floor_ms` (also stored in baseline.json, default 2 ms)"_ — i.e. `noise_floor_ms` is a property of the **baseline**, not the current run. `bench-regression-check.sh:31` confirms this: it reads `noise_floor_ms` from the baseline, never from results.
- The field in results is dead but misleading: a future maintainer could plausibly believe it's load-bearing.
- Recommendation: drop `noise_floor_ms` from the results report. If diagnostic, name it explicitly (e.g. `derived_noise_estimate_ms`) so it can't be mistaken for the spec field.

### R6 — Suppression-ledger lock-poisoning silently disables suppression

- `watcher/subscription.rs:103`: `suppression.lock().map(...).unwrap_or(false)`. If the mutex is poisoned, `should_suppress` returns `false` and every self-event is forwarded as a real event indefinitely.
- Spec doesn't mandate behaviour on poisoning, but the realistic alternative — abort the watcher subscription — is safer than silently producing duplicate-work events forever.
- Recommendation: log poisoning via `tracing::error!` and propagate via a `WatchEventKind::Error` (paired with the §B5 fix) or terminate the subscription. The current "fail open" silently degrades correctness.

### R7 — `recv_timeout` collapses timeout and disconnection to the same `WatchError::Closed`

- `watcher/subscription.rs:52–54`: `recv_timeout(timeout).map_err(|_| WatchError::Closed)`.
- A daemon polling loop cannot distinguish "no events in this window, keep going" from "watcher gone, abort." It will treat every poll-with-no-events as a fatal subscription closure.
- Recommendation: introduce `WatchError::Timeout` and map `RecvTimeoutError::Timeout -> Timeout`, `RecvTimeoutError::Disconnected -> Closed`. One-line fix that prevents an entire class of caller bug.

### R8 — Suppression entries' 30s TTL is hardcoded; spec says 60s and notes correctness should not depend on it

- `watcher/suppression.rs:33`: `expires_at: Instant::now() + Duration::from_secs(30)`.
- Spec §11.3: _"Default expiry is 60 seconds, but correctness does not depend on expiry; hash mismatch wins."_
- 30s vs 60s isn't a correctness bug because hash equality is the deciding factor. But the magic number disagrees with the spec default and isn't named.
- Recommendation: extract `const DEFAULT_SUPPRESSION_TTL: Duration = Duration::from_secs(60);`. Document that hash-equality-not-TTL is the correctness contract. Easier to audit later.

### R9 — Bench harness binary has no path-safety guard against being pointed at `bench/baseline.<profile>.json`

- `bin/stream_a_bench.rs:146`: unconditionally `fs::write(&output, ...)`.
- `scripts/bench-gate.sh:14` guards via `case "$(basename "$output")" in baseline.*.json) ...`. This catches the canonical invocation, but anyone running `cargo run -p memory-substrate --bin stream_a_bench -- ... --output bench/baseline.linux-x86_64.json` directly will silently clobber the baseline.
- Spec §17.6 / §18.9 mandate baselines change only via human-authored commits. Defense-in-depth at the harness layer is cheap.
- Recommendation: in `run()`, after parsing `output`, refuse if `output.file_name().map(|name| name.to_string_lossy().starts_with("baseline.")) == Some(true)`. One conditional, prevents an entire class of accident.

### R10 — Watcher subscription channel is unbounded

- `watcher/subscription.rs:72`: `channel()` (`std::sync::mpsc`, unbounded).
- A slow consumer plus an event storm (e.g. mass `git checkout`, large rebase) can balloon memory without bound. No backpressure signal back to the watcher.
- Spec §11 doesn't mandate bounds; this is operational hardening. Mention it in case the daemon ever runs without a tight consumer.

## Nits

### N1 — `runtime/reconcile.rs:179` rebuilds `existing_event_ids` on every replay call

- `read_events(event_log)?.into_iter().map(|event| event.id).collect()` walks the entire event log into memory. For a long-running daemon with millions of events that's nontrivial cost. Acceptable for a one-shot at startup, but the spec doesn't actually need the full set — only the IDs of events queued for replay matter. A streaming `read_events_ids` would be cheaper.

### N2 — `runtime::FaultSet` only stores names, not parameters

- `runtime/faults.rs:7–20`. Tests will eventually want `fault.set("write.fsync", FailEvery(3))`-style parameterized injection. The current shape is fine for "is this point active" booleans only.

### N3 — `Fixture::build`'s `next_memory_id` collisions

- `bin/stream_a_bench.rs:210`: every memory uses `mem_20260424_a1b2c3d4e5f60718_{index:06}`. The shard `a1b2c3d4e5f60718` is hardcoded and unrelated to a real device id. This works for a synthetic fixture but means the bench corpus's IDs would collide with any real device whose shard happens to land on those bytes. Cosmetic in a bench, surprising if it ever leaks into a real test.

### N4 — `convergence::ignored` uses `.git` / `.memoryd` / `target` substring on a single component

- `memory-test-support/src/convergence.rs:38–42`. Fine in practice, but `target` is broader than spec — a memory file whose path component is literally `target` (e.g. `agent/target/...`) would be excluded. The spec's exclusion list does not include `target`; that's a Cargo build artefact that shouldn't even live under the repo root in normal usage. Probably defensible but worth a comment.

### N5 — `rust_boundary_check` is misnamed

- `memory-test-support/src/bin/rust_boundary_check.rs`. The binary does two textual lints: no absolute path literals in tests, no raw `.unwrap()`/`.expect(` in src. There is no boundary verification in the architectural sense (e.g. "no module under `index::` references `git::`", "public API is the documented surface"). Either rename to `rust_lint_check` / `forbidden_patterns` or expand the contents to actually check boundaries.

### N6 — `unwrap-justified:` escape hatch requires the marker on the same line as the `.unwrap()`

- `memory-test-support/src/bin/rust_boundary_check.rs:46`. Authors who put the rationale on the line above (the natural English position) will trip the lint. Recommend: walk a small lookbehind window (e.g. previous comment-only lines) for the marker, or change the convention to require it directly above and document loudly.

### N7 — `metric()` mutates its `&mut [f64]` slice

- `bin/stream_a_bench.rs:295`: `fn metric(values: &mut [f64])` sorts in place. Fine, but the signature is misleading because the surrounding caller doesn't reuse the values after. A `fn metric(mut values: Vec<f64>)` taking ownership would be clearer.

### N8 — `append_framed_jsonl` round-trips through `serde_json::Value` for no reason

- `runtime/reconcile.rs:214`. `serde_json::to_value(value)` then `encode_event_line(&value)`. `encode_event_line` only needs a `&Value`, but if the goal is "framed JSONL" with the framing handled centrally, there's no reason `encode_event_line` couldn't accept anything `Serialize`. Minor, but the double-conversion is visible enough to comment on.

### N9 — `reconcile.rs:158–161` constructs a path-error with an `?` that bypasses the existing `std::io::Error::other` pattern

- `.ok_or_else(|| std::io::Error::other("pending encrypted index op missing path"))`. Three lines later (165) the same shape uses `std::io::Error::other(format!("..."))`. Consistency: use either `Error::other` everywhere or `Error::new(InvalidData, ...)` to match the truncation branch at 257.

## Strengths worth keeping

- **`SuppressionLedger` semantics are correct.** Hash-keyed in-flight + committed states with TTL + hash-mismatch-wins is the spec's invariant 3, and it's faithfully implemented in `watcher/suppression.rs:43–58`. Brief invariant 3 holds.
- **`SuppressionState` enum is small and clear.** No bitfields or boolean tuples; the variants name themselves.
- **Reconcile data types are well-modelled.** `PendingIndexOp`, `PendingEncryptedIndexOp`, `PendingEventOp` all carry `attempts` + `last_error` for observable replay state. Even though the replay loop doesn't use those fields yet, the schema is right.
- **Atomic compaction in `compact_or_rewrite_pending_file`** uses the rename-tmp-then-rename pattern with directory fsync — correct, follows the §8.3 protocol style.
- **`memory-test-support::perf::synthetic_vectors` is structurally correct** (deterministic StdRng seed, L2 normalization, sha256 corpus identity). It's just unwired from the bench harness.
- **`WatchSubscription`'s drop semantics are right:** the subscription holds the `RecommendedWatcher`, dropping releases OS resources, and `unsubscribe` is the explicit form. Spec §11.4 last-bullet acceptance is structurally satisfied; the test in `tests/watcher_lifecycle.rs` is the right shape.

## Open questions for Trey

1. **Is §13.5.1 working-tree audit + quarantine in scope for Stream A v0.3 plan, or deferred?** B1 calls out that phases 1, 2, 7, 8 are all missing. If Codex deferred them intentionally, the plan should say so and `Substrate::open` should at least surface a typed `OpenError::ReconciliationDeferred` instead of silently doing 4-of-9 phases and returning `Ok`.
2. **Is `runtime::blocking` aspirational scaffolding for Stream B, or load-bearing for Stream A's own async surface?** B7's fix path depends on intent. If Stream B will run its own runtime and call substrate from a `spawn_blocking`-aware shim, Stream A can be sync-honest (drop `async` from the offending methods). If substrate's async API is the contract, every `pub async fn` that touches SQLite needs to be rewritten.
3. **Should the bench fixture corpus mirror real Stream E namespacing (R3)?** It changes p95s. Worth pinning before the first real baseline goes in, since baselines are immutable absent explicit human commits.
4. **Are encrypted-index-ops `compacted.jsonl` artefacts intended to persist forever?** B2 calls out the orphan-file path. If forensics are the reason, name and document; if it's a bug, switch to `remove_file`.
5. **Is `roots_converged` planned to be the real §13.6.1 helper or strictly a byte-level smoke check?** R1's concern is that the name implies the spec semantics. If the test-support helper will only ever be a byte check and the real convergence comparison lives in `scripts/two-clone-convergence.sh`, rename and document.
