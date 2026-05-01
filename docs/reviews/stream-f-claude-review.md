# Stream F — Claude adversarial review

**Reviewer:** Claude (Anthropic) — fresh-context adversarial pass after Codex's gate-pass.
**Co-reviewer:** Substrate-surface teammate spawned by Claude (general-purpose subagent, same repo, same skills).
**Skills used as rubrics:** `clean-code` (Robert C. Martin) + `rust-engineer` (idiomatic Rust 2021).
**Date:** 2026-05-01.
**Live spec:** `docs/specs/stream-f-dreaming-v0.2.md`.
**Live plan:** `docs/plans/2026-04-30-stream-f-dreaming.md`.
**Reviewed commit:** `4f96dfb` ("Ship Stream F dreaming"). 138 files, ~22k LoC.
**Codex's gate:** PASS (`docs/reviews/stream-f-final-gate-report.md`).
**This review's verdict:** **NOT shipping-ready as-is.** Eight blockers below need to land before Stream F delivers the operational behavior the spec promises.

---

## 0. Reviewer's framing

Codex's self-review rounds caught and patched a lot. The architecture is sound:

- masked synthesis, three-pass pipeline, harness-CLI delegation, governance-bound candidate writes, Stream A surface additions all land cleanly;
- deep-cuts spec compliance (path encoding without colons, 9-tool MCP set, invariant-driven routing in the substrate) is correct;
- Stream A's `is_noncanonical_stream_f_repo_path` precedes `MEMORY_PREFIXES` in `validate_repo_relative_path` so dream files can't accidentally enter canonical parsing.

What slipped through is mostly **wiring gaps, not architecture flaws**. The biggest single issue is that the _daily/scheduled_ half of Stream F — the part that makes the system "memoryful over time" — isn't actually plumbed into a daemon or CLI surface. Both `run_scheduled_lease` and `run_cleanup` are reachable only from tests and the bench binary. A deployed memoryd cannot run them. Combined with a substrate-API bypass in cleanup and content-blind encrypted descriptors, the dreaming pipeline as shipped is structurally complete but operationally non-functional.

The blockers should be closeable in days, not weeks. This review is structured so Codex's planner can shape it into per-task worktree work without further triage.

---

## 1. Blockers

These must land before Stream F can be relied on for its primary purpose. Each blocker carries a `Why it matters`, a `What's wrong (file:line)` pointer, and a `Fix shape` so the planner can scope it.

### B1. `run_scheduled_lease` is unwired from production

**Why it matters.** Spec §6.1.2 promises that "a transient `git fetch` blip at 03:00 does not silently erase the day's dream" via bounded retry. As shipped, no production code path invokes `run_scheduled_lease`. The spec's "scheduled-vs-manual lease semantics split" (one of the 12 declared v0.1→v0.2 fixes) is structurally complete in the code but operationally absent.

**What's wrong.**

- `crates/memoryd/src/dream/lease.rs:198` defines `run_scheduled_lease`.
- Only callers, per `grep -rn run_scheduled_lease`: `crates/memoryd/tests/dream_lease_scheduled_retry.rs` (six call sites).
- `crates/memoryd/src/cli.rs:222-234` defines `DreamCommand` with subcommands `Status / Now / Review / Enable / Disable`. No `Scheduled`. No `Run`.
- `crates/memoryd/src/main.rs` and `crates/memoryd/src/server.rs` contain no scheduler thread, no cron-driven worker, no daemon-internal trigger.
- The harness installer's launchd/systemd/cron template has no documented entry point that fires this path.

**Fix shape.**

1. Add `memoryd dream scheduled` (or equivalent) as an admin-only CLI subcommand that wraps `run_scheduled_lease` per spec §6.1.2. Args: `--scope`, `--repo`, `--runtime`, `--json`. Same exit-code conventions as `dream now`.
2. Document in `docs/api/stream-f-dreaming-api.md` that scheduled runs are invoked via this command and that the harness installer registers an OS-level scheduler (launchd/systemd/cron) to call it daily at `dreams.cleanup_run_hour_utc`.
3. Add an integration test in `crates/memoryd/tests/dream_cli.rs` exercising `memoryd dream scheduled --scope me` end-to-end against a `ScriptedLeaseGit`-equivalent or a temp-repo fixture.
4. The OS-scheduler templates can come in a follow-up; the CLI surface and contract test are the v0.2 ship-blocker.

**Open question for the planner.** Spec §6.1.2 step 1 says "At `dreams.cleanup_run_hour_utc` ... attempt the manual-run sequence above." The spec implies a daemon-internal scheduler. If we'd rather do that than rely on the OS scheduler, a worker thread in `crates/memoryd/src/workers.rs` is the right home. Either is spec-compliant; the OS-scheduler path is simpler to ship.

### B2. `run_cleanup` is unwired from production

**Why it matters.** Spec §7 mandates a daily idempotent cleanup pass: substrate-fragment archival (§7.1), stale-candidate archival, entity-index rebuild, memory lint, tombstone integrity, supersession-chain orphan check, `observed_at` refresh, event-log compaction. Without cleanup, the substrate accumulates indefinitely, fragments never archive, candidates never time out, and event logs never compact. The "memoryful over time" promise of Stream F depends on this running.

**What's wrong.**

- `crates/memoryd/src/dream/cleanup.rs:43` defines `pub async fn run_cleanup`.
- Only callers, per `grep -rn run_cleanup`: `crates/memoryd/tests/dream_cleanup.rs` (12 call sites) and `crates/memoryd/src/bin/stream_f_dream_bench.rs:305`.
- No CLI subcommand, no daemon worker.

**Fix shape.**

1. Add `memoryd dream cleanup` as an admin CLI subcommand. Args: `--repo`, `--runtime`, `--device-id` (default from `local-device.yaml`), `--now` (default `Utc::now()`, accepts ISO-8601 for testing), `--json`.
2. Either (a) install an OS scheduler to run `memoryd dream cleanup` daily at `dreams.cleanup_run_hour_utc + dreams.dream_retry_window_minutes`, or (b) add a daemon-internal scheduler thread that fires it. Pick the same approach as B1 for consistency.
3. Add a contract test in `dream_cli.rs` covering at least the success path and the dirty-tree-defers-commit path.

### B3. Scheduled lease retry doesn't actually wait between attempts

**Why it matters.** Even after B1 wires up the scheduled path, the retry-window contract is silently violated: spec §6.1.2 step 2 calls for exponential backoff ("1min, 2min, 4min, 8min, 16min, 32min, capped at 32min between attempts"). The code computes the offset list and then discards each offset.

**What's wrong.**

- `crates/memoryd/src/dream/lease.rs:215` — `for _offset in retry_offsets`. The `_offset` binding is unused. The function loops attempt-after-attempt with no sleep.
- `crates/memoryd/tests/dream_lease_scheduled_retry.rs:85-107` — `scheduled_persistent_failure_records_missed_run_summary` scripts 3 fetch failures with `retry_window_minutes: 3` and asserts `report.attempts == 3`. This validates the offset count, not any wall-clock behavior. Equivalent: scripting 10 failures with `retry_window_minutes: 180` would also pass instantly.

**Fix shape.**

1. Introduce a clock/sleeper trait that the function injects (so tests stay fast):
   ```rust
   pub trait LeaseSleeper {
       fn sleep_minutes(&self, minutes: u16);
   }
   pub struct RealLeaseSleeper;
   impl LeaseSleeper for RealLeaseSleeper { fn sleep_minutes(&self, m: u16) { std::thread::sleep(Duration::from_secs(u64::from(m) * 60)); } }
   pub struct ImmediateLeaseSleeper; // for tests
   ```
2. Compute the _delta_ between consecutive offsets and sleep for that delta before each attempt after the first.
3. Update the existing tests to use `ImmediateLeaseSleeper` (no behavioral change) and add a new test that asserts the sleeper was called with the expected delta sequence `[1, 2, 4, 8, 16, 32, 32, 32, ...]` capped at 32.
4. Bonus: an integration test with a fake clock that asserts the _cumulative_ sleep time approaches `retry_window_minutes` for the persistent-failure case.

**Open question.** If B1 lands as a daemon-internal scheduler, the sleeper can be `tokio::time::sleep` and the trait collapses to a `Box<dyn Fn(Duration) -> impl Future>`. If B1 is an OS-scheduler-driven CLI, the sleeper is sync `thread::sleep` and CLI-blocking. Pick one.

### B4. Cleanup mutates canonical memories with raw `fs::write`, bypassing `Substrate::write_memory`

**Why it matters.** Stream A's contract (CLAUDE.md "Critical invariants" #2): "Every write request carries a `ClassificationOutcome`. No defaults." Plus event log emission, atomic rename, hash check, and merge-driver awareness on contested writes are all functions of the `Substrate::write_memory` API. Cleanup currently bypasses all of that for two operations.

**What's wrong.**

- `crates/memoryd/src/dream/cleanup.rs:128-152` — `archive_stale_candidates` flips `MemoryStatus::Archived` and writes the memory back via `fs::write(&absolute, serialize_document(&memory)...)`. No `WriteRequest`, no classification, no event emission.
- `crates/memoryd/src/dream/cleanup.rs:231-267` — `refresh_observed_at` does the same for the `extras["observed_at"]` update.
- This means: (a) no `EventKind::*` emission for cleanup-driven mutations, (b) no atomic rename — a memoryd crash mid-`fs::write` corrupts the canonical file, (c) no expected-base-hash check — concurrent edits on a sibling device can be silently overwritten, (d) no `Substrate` index refresh for the mutated row.

**Fix shape.**

1. Replace `fs::write(serialize_document(&memory))` with `substrate.write_memory(WriteRequest { ... write_mode: WriteMode::UpdateExisting, classification: ClassificationOutcome::Trusted, event_context: EventContext { actor: Some("memoryd-cleanup-bot".to_string()), reason: ... }, ... }).await`.
2. The base-hash for `UpdateExisting`: re-read the memory immediately before mutation and use its computed hash so concurrent edits on another device are detected.
3. The classification stays `Trusted` for a status flip (status is frontmatter, not body content) — but if `refresh_observed_at` is folded into the same request shape, the body is unchanged so this is consistent.
4. Sequence cleanup so all canonical-memory mutations go through this path; `fs::write` is reserved for cleanup's own report file (`dreams/cleanup/<device>/<date>.json`) which is non-canonical.
5. Add a regression test: cleanup runs, then a substrate query returns the new `Archived` status AND the event log contains a corresponding `EventKind::MemoryWritten` (or whichever variant matches `UpdateExisting`).

**Note for Codex.** The `Substrate` API may not currently expose an "update without classifier rerun" path. If `write_memory` insists on running Stream D classification on every body, that's a Stream D/A coordination point — but cleanup status flips don't change the body, so the existing classifier run is a no-op there. Verify before refactoring.

### B5. Encrypted substrate fragment descriptors are content-blind

**Why it matters.** Spec §5.1.1: "`descriptor` carries Stream D's safe-descriptor projection (the same projection produced for encrypted canonical memories), used as Pass 1 input in lieu of the ciphertext." The shipped implementation collapses every encrypted observation's `summary_safe` to one of three constants. Pass 1 reading encrypted descriptors gets approximately zero per-fragment signal, which means encrypted observations contribute nothing meaningful to dreaming — defeating the spec's "encrypted fragments still inform Pass 1 via their safe descriptor" model.

**What's wrong.**

- `crates/memoryd/src/handlers.rs:626-632` — `encrypted_observe_descriptor`:
  ```rust
  fn encrypted_observe_descriptor(kind: ObserveKind) -> EncryptedSubstrateDescriptor {
      let tag = observe_kind_tag(kind);
      EncryptedSubstrateDescriptor {
          summary_safe: format!("encrypted {tag} substrate fragment"),
          tag_safe: vec![tag.to_string()],
      }
  }
  ```
- Spec §5.1.1 example: `summary_safe = "User asked about auth flow integration"`, `tag_safe = ["auth"]` — content-derived, not type-derived.
- `orchestration.rs:464` reads `descriptor.get("summary_safe")` for Pass 1 input. Every encrypted fragment now contributes the same generic phrase.

**Fix shape.**
This is a Stream D/F coordination point. Three honest paths:

1. **Build the projection the spec describes.** Stream D ships a `safe_plaintext_fragment` classifier that gates whether a string is safe to index. We need a projection function that takes the plaintext and returns a _truncated, classifier-trusted summary_ — something like the first N tokens that pass `safe_plaintext_fragment`, plus a classifier-derived tag set. This is a small new function in `memory-privacy`. Stream F passes the original plaintext through this projection _before_ encrypting and uses the result as `descriptor.summary_safe`.
2. **Reuse Stream D's encrypted-canonical-memory projection.** The spec says "the same projection produced for encrypted canonical memories." If Stream D already produces such a projection for canonical writes, factor it out and call it from `encrypted_observe_payload`. If Stream D currently _also_ uses content-blind descriptors for encrypted canonical memories, raise that as a Stream D bug — both sides may need the same fix.
3. **Spec amendment.** If a content-aware safe projection turns out to be more risk than the per-fragment Pass 1 signal is worth, amend the spec to v0.3 to make content-blind descriptors normative. Trey owns that call.

**Recommended.** Path 1 is the lowest-cost spec-faithful fix and unblocks Pass 1 for encrypted-substrate users. Codex should investigate Stream D's actual encrypted-canonical-memory descriptor today before committing to a path; if Stream D ships content-blind there too, Path 2 with a paired Stream D fix is correct.

### B6. `RepoPath::new` (panic-on-invalid) used in production archive path construction

**Why it matters.** `RepoPath::new` is documented as a test/fixture constructor: panics on invalid input. Production code paths construct paths with `try_new` and propagate errors. A panic in `archive_expired_substrate_fragments` aborts the daemon worker rather than returning a typed `WriteFailure`.

**What's wrong.**

- `crates/memory-substrate/src/api.rs:837` (per substrate teammate's pointer; verify exact line).
- Today the panic is unreachable because the device-id regex (`^dev_[a-z0-9]+$`) constrains the device segment to safe bytes. But any future change to either the device-id format or `is_noncanonical_stream_f_repo_path`'s `.jsonl` branch turns a contract violation into a panic.

**Fix shape.**
Replace `RepoPath::new(format!(...))` with `RepoPath::try_new(format!(...)).map_err(|err| WriteFailure { kind: WriteFailureKind::InvalidPath, ... })?`. Mirror the error-shape used elsewhere in `api.rs`.

### B7. Missing substrate-level test for `WriteFailureKind::DreamProseAsSource`

**Why it matters.** Spec §2 invariant 1 ("Dream prose is never a grounding source") is enforced in `crates/memory-substrate/src/api.rs` (`enforce_no_dream_prose_sources`), but the only test that exercises the guard is `crates/memoryd/tests/dream_grounding_rehydration.rs:244` — an integration test in a different crate. Stream A's own test suite has zero substrate-level coverage of this write-time guard. A future refactor of `api.rs` that breaks the guard would not be caught by Stream A's gate.

**Fix shape.**
Add to `crates/memory-substrate/tests/dream_canonical_isolation.rs`:

1. A test that calls `Substrate::write_memory` with a `WriteRequest` whose `memory.frontmatter.source.reference = Some("dreams/journal/me/2026-04-30.md")` and asserts `WriteFailureKind::DreamProseAsSource`.
2. Same with `evidence[].reference` pointing to `dreams/journal/...` and `dreams/questions/...`.
3. Same via `supersede_memory` to confirm both write paths enforce the guard.

### B8. Missing substrate-level test for `Secret`-classification refusal in `append_substrate_fragment`

**Why it matters.** CLAUDE.md "Critical invariants" #1: "`secret` is never persisted to disk. ... Stream A returns `WriteFailureKind::SecretRefused` before any disk effect (spec §8.7)." The new fragment-append surface (`Substrate::append_substrate_fragment`) is the only new write path in Stream F that needs this guard, and it's untested at the substrate boundary. The canonical-write path is covered by `api_write_read::classification_secret_refuses_before_any_disk_effect`; the fragment-append path has no equivalent.

**Fix shape.**
Add to `crates/memory-substrate/tests/dream_substrate_primitives.rs`:

1. A test that calls `Substrate::append_substrate_fragment(SubstrateFragmentAppendRequest { classification: ClassificationOutcome::Secret, ... })` and asserts `WriteFailureKind::SecretRefused` with no on-disk effect.
2. Companion test that verifies the disk root is byte-identical before and after the refused call (no partial JSONL append).

---

## 2. Risks (worth tracking, not strict ship-blockers)

### R1. `ClassificationOutcome::Trusted` hardcoded for restored Pass 2 candidates

**Where.** `crates/memoryd/src/dream/orchestration.rs:178`.

**Concern.** Pass 2 restores masked content via `MaskingSession::restore`, then writes the candidate with `classification: ClassificationOutcome::Trusted`. The restored text is gated by `candidate_plaintext_is_safe` (line 153) which calls `safe_plaintext_fragment` — anything classified as PII is rejected with `unsafe_candidate`. So unsafe content never reaches `write_memory`.

But Stream D's full classifier produces `Trusted | RequiresEncryption | Refuse`. Storage actions other than `Refuse` are conflated here: phone/email/address content that Stream D would route to `EncryptAtRest` (per the Stream D Claude-review fix in `5f7d926`) is routed to `unsafe_candidate` refusal instead. Result: legitimate dream output that should encrypt-at-rest is silently dropped.

**Fix shape (low cost).**
Replace `ClassificationOutcome::Trusted` with the result of running `classify_privacy(&restored_text, namespace, None)` and mapping its `storage_action` to either `Trusted`, `RequiresEncryption`, or refused. This mirrors what `memory_observe`'s handler already does at `handlers.rs:440-458`.

**Open question.** Whether Stream F's `dreaming-strict` policy should permit encrypt-at-rest candidates at all is a governance call. If "dreams should never produce encrypted candidates," document that as a deliberate policy choice and keep the current behavior. If "dreams produce candidates at whatever sensitivity tier the content demands," fix per above.

### R2. `EchoCli` is reachable as a production CLI option

**Where.** `crates/memoryd/src/handlers.rs:226` (`if name == "echo" { return Ok(()); }`), `crates/memoryd/src/dream/orchestration.rs:86`, `crates/memoryd/src/dream/run.rs:45`.

**Concern.** Spec §4.2 explicitly tags `EchoCli` as "test-only, replays canned outputs from a `HashMap<PromptHash, String>` fixture, never spawns a subprocess." Allowing `memoryd dream now --cli echo` in production isn't a security issue (echo doesn't leak), but it ships test infrastructure as a documented CLI surface. Contract drift.

**Fix shape.** Either (a) gate the `--cli echo` path behind a `#[cfg(any(test, feature = "echo-harness"))]` so it disappears from release builds, or (b) document EchoCli as a supported v0.2 admin-only option (and update spec §4.2 accordingly). (a) is cleaner.

### R3. `find_executable` doesn't filter empty PATH components

**Where.** `crates/memoryd/src/dream/harness.rs:618-623`.

**Concern.** A daemon environment with `PATH=":/usr/bin"` (legacy Unix shorthand for "search cwd") would find `claude`/`codex` in `cwd` ahead of `/usr/bin`. Defense-in-depth fail. The risk is exploitable only if memoryd runs from a directory writable by another user or if a malicious binary is dropped into the install dir, but the cost of the fix is one filter call.

**Fix shape.**

```rust
std::env::split_paths(&path_env)
    .filter(|p| !p.as_os_str().is_empty())
    .map(|directory| directory.join(program))
    .find(|candidate| is_executable_file(candidate))
```

### R4. stdin-write EPIPE promoted to subprocess error

**Where.** `crates/memoryd/src/dream/harness.rs:460` (`stdin_write_result?;` after the success-path branch).

**Concern.** A harness CLI that reads its prompt to do its work and closes stdin early (perfectly normal Unix behavior) would surface a `BrokenPipe` from the stdin writer thread. The `?` then converts a successful run into an error after stdout was already captured cleanly.

**Fix shape.** When the child exited with status 0 AND we have non-empty stdout, downgrade `stdin_write_result` from `?`-error to a debug log entry. The harness CLI's success is the authoritative signal; the writer thread's EPIPE is informational.

### R5. Cleanup propagates per-file errors via `?`, breaking idempotency

**Where.** `crates/memoryd/src/dream/cleanup.rs:147` (`archive_stale_candidates`), `crates/memoryd/src/dream/cleanup.rs:252-262` (`refresh_observed_at`).

**Concern.** Spec §2 invariant 6: "Cleanup is commutative and idempotent." One corrupt frontmatter or one un-stat-able source file currently aborts the whole pass and leaves the cleanup report unwritten — opposite of idempotent.

**Fix shape.** Wrap each per-memory mutation in a `match` that, on error, pushes a `CleanupFinding { kind: "memory_lint" | "observed_at_refresh", ... }` and continues to the next file. The pass completes; the report tells the operator which files need attention.

### R6. `contains_phone_like_digit_sequence` is overzealous on metadata fields

**Where.** `crates/memoryd/src/handlers.rs:580` and its caller `validate_observe_metadata_is_safe` at line 544.

**Concern.** The function rejects any string with 10+ contiguous digits (separated only by `-._<space>`). Applied to `session_id`, `harness`, `harness_version`, and entity ids. A `session_id: "sess_01234567890ABCD"` would be rejected — harmless for typical ULID-style ids (which interleave letters and digits) but unprincipled. Plus: the entity id format is already constrained to `^ent_[\w.:-]+$` at line 510-518, so the canary's contribution to entity validation is double-defense at best.

**Fix shape.** Drop `contains_phone_like_digit_sequence` from `validate_observe_metadata_is_safe`. Keep `contains_aws_access_key`, the GitHub PAT prefix, and the Stripe live-key prefix — those are unambiguous secret signals. Phone-number-shaped digit runs are a content classifier's job (Stream D), not metadata's.

### R7. No test exercises MaskingSession Drop on panic

**Where.** `crates/memoryd/tests/dream_pass_pipeline.rs` covers success-path drop (line 685) and empty-pass-1-path drop (line 636) — both clean returns, not unwinding. Spec §6.5 explicitly: "Acceptance tests cover the failure-path drop with a `MaskingSession` instrumented to assert its `Drop` ran."

**Concern.** Rust's language guarantee covers panic-drop-during-unwind for stack values, so this isn't a runtime correctness issue. It is a test coverage gap against an explicit spec acceptance signal.

**Fix shape.** Add a `#[tokio::test]` that builds a `DreamRunner` with a custom `CandidateWriter` that panics on `write_candidate`. Wrap `runner.run()` in `tokio::task::spawn(...)` to convert the panic into a `JoinError`, then assert `observer.drops() == 1`.

### R8. `run_scheduled_lease_with_runner` swallows the original dream error if release fails

**Where.** `crates/memoryd/src/dream/lease.rs:220` (`release_manual_lease_with_git(git, request.acquire.clone())?;`).

**Concern.** The `?` propagates a release-side error and discards the original `run_dream` error. Lossy diagnostics: the operator sees "release failed" when the actual cause was "Pass 2 timed out."

**Fix shape.** Capture both errors:

```rust
if let Err(dream_err) = run_dream(&lease) {
    if let Err(release_err) = release_manual_lease_with_git(git, request.acquire.clone()) {
        // log release_err to the cleanup summary; return the original dream_err
    }
    return Err(dream_err);
}
```

### R9. `cleanup-bot` shells out to git directly while `lease-bot` goes through a trait

**Where.** `crates/memoryd/src/dream/cleanup.rs:419-431` (`commit_cleanup`) shells out via `Command::new("git")`. `crates/memoryd/src/dream/lease.rs` uses the `LeaseGit` trait (`git.rs:12-18`).

**Concern.** Two unrelated paths to git for two daemon-authored writers. Cleanup can't be tested with a `ScriptedLeaseGit`-style fake. Spec §7.1 and §6.1.3 share commit conventions ("memoryd lease-bot", "memoryd cleanup-bot", same author email pattern); they should share a commit harness.

**Fix shape.** Generalize `LeaseGit` to a `DreamGit` trait that covers both lease and cleanup commits, or introduce a sibling `CleanupGit` trait. Either lets `dream_cleanup.rs` tests script git outcomes without a real repo.

### R10. `is_dream_prose_ref` doesn't cover `dreams/cleanup/` or `leases/`

**Where.** `crates/memory-substrate/src/api.rs:1476` (per substrate teammate).

**Concern.** Spec §2 invariant 1 names "Pass 1 narrative and Pass 3 questions" specifically, so the current scope is technically compliant. But cleanup JSON and lease JSONL are also dream artifacts that shouldn't be cite-able as canonical-memory grounding sources. If Stream G later exposes cleanup reports as reviewable, a citation could slip past `enforce_no_dream_prose_sources`.

**Fix shape.** Extend the prefix check to `journal | questions | cleanup`. Don't add `leases` (that's daemon-authored telemetry, never user-facing). Update the spec-compliance test alongside.

### R11. `validate_noncanonical_stream_f_file` silent-passes unrecognized Stream F paths

**Where.** `crates/memory-substrate/src/tree/validate.rs:140-168` (per substrate teammate).

**Concern.** A fallthrough `Ok(())` at the end of the match. Today no path reaches it because `is_noncanonical_stream_f_repo_path` and the validator are kept in sync. A future addition to one without the other turns the validator into a no-op for that family.

**Fix shape.** Replace the fallthrough with `panic!("unreachable: noncanonical Stream F path family must have an explicit validation branch")` or refactor to a path-family enum so the compiler enforces exhaustiveness.

### R12. `DreamsConfig` drops `Eq` from `SyncedConfig` (silent breaking change for downstream)

**Where.** `crates/memory-substrate/src/config/mod.rs` (per substrate teammate).

**Concern.** `DreamsConfig` contains `f64` (e.g., `pass_2_drift_threshold`), so `SyncedConfig` can no longer derive `Eq`. The workspace gate passes because no in-tree code does `assert_eq!` on a `SyncedConfig`. But `SyncedConfig` is a public exported type; any out-of-tree code that derived `Eq` transitively now fails to compile.

**Fix shape.** Either (a) use a fixed-point representation (e.g., parts-per-thousand `u16`) for the threshold so `Eq` survives, or (b) document the breaking change in `docs/api/stream-a-public-api.md` and bump the API doc's version note. (a) is cleaner; the threshold is config-validated to one decimal place anyway.

### R13. `best_effort_event_seq_start` reads the entire event log on open (substrate teammate)

**Where.** `crates/memory-substrate/src/api.rs:1318-1329`.

**Concern.** O(n) blocking read of the full event log to compute the max sequence number for the device. Used only for `DurabilityTier::BestEffort`. On a repo with a 90-day-bounded event log on an active agent, this could be 100k+ events. Today the bench fixture and `memory_observe` test paths probably hit this. Production BestEffort opens will too.

**Fix shape.** Two options. (a) Cache the max-seq in `local-device.yaml` and update on each best-effort write. (b) Read the event log tail-first and stop at the first record matching the device. (b) is simpler and idempotent against truncation; (a) is faster but adds local state.

---

## 3. Nits

These are cosmetic or low-priority. Codex can fold them into adjacent task work.

- **N1.** Magic numbers in `crates/memoryd/src/dream/orchestration.rs:35-37`: `MAX_DREAM_SUBSTRATE_FRAGMENTS = 1_000`, `MAX_DREAM_ACTIVE_MEMORIES = 256`, `MAX_PREVIOUS_QUESTIONS = 64`. Spec doesn't define these caps at this layer. Either promote to `DreamsConfig` knobs (with validation ranges) or add a doc comment explaining the rationale and trace to the spec section that motivates each cap.
- **N2.** `crates/memoryd/src/dream/harness.rs:580` — `validate_json_if_expected(_prompt: &str)` argument is unused. Drop it.
- **N3.** `crates/memoryd/src/dream/lease.rs:296` — `format!("run_{}", ...timestamp_nanos_opt().unwrap_or_default()...)` collapses to `"run_0"` on overflow. Improbable in practice but unprincipled. Use a ULID or device-id-prefixed counter.
- **N4.** `crates/memoryd/src/dream/run.rs:248` — `failed_pass(code, error.to_string())` stringifies the structured `DreamError` instead of mapping to a stable error code per spec §3.2's `error_code` contract. The current shape `pass_2_failed:<freeform>` is parseable but defeats the spec's intent.
- **N5.** `crates/memoryd/src/dream/harness.rs:414-464` — `run_hardened_command_blocking` is a 50-line function doing scratch-dir setup, spawn, two reader threads, stdin writer thread, wait-with-timeout, error mapping, AND JSON validation. Split into `prepare_command`, `spawn_and_capture`, and `finalize_output`. Per clean-code ch. 3, "functions should do one thing."
- **N6.** `crates/memoryd/src/dream/pass2.rs:50-53` — When `proposals.len() > candidate_cap`, the entire batch is refused with `too_many_candidates`. Truncate-to-N is also defensible and saves the dream's useful output. Spec §6.3 step 4 says "candidate count must not exceed `dreams.pass_2_max_candidates`" without specifying behavior on overflow. Pick truncate-with-warning for less-lossy operation.
- **N7.** `crates/memoryd/src/dream/cleanup.rs:147` — `archive_stale_candidates` writes via non-atomic `fs::write`. Even after B4 routes through `Substrate::write_memory`, document the durability tier expected.
- **N8.** `crates/memory-substrate/src/api.rs` — `append_jsonl_record` (per substrate teammate, ~line 1415) has two separate `if matches!(target.durability, DurabilityTier::Full)` blocks that could be one.
- **N9.** `crates/memory-substrate/src/tree/validate.rs:140-168` — repeats `rel.starts_with(...)` string checks that `is_noncanonical_stream_f_path` already performed. Extract a path-family enum and centralize.
- **N10.** `crates/memory-substrate/src/git/commit.rs` — `run_lease_commit` has a hardcoded 6-element arg vec for the error message that duplicates the actual command args. Add `// keep in sync with command.args(...)` or compute one from the other.
- **N11.** `crates/memory-substrate/src/config/mod.rs:277` — `is_known_harness_name` validates `gemini` even though the spec defers Gemini support. Config files referencing `gemini` pass validation but produce `dream_disabled` at runtime on most devices. Either drop `gemini` from the validator or accept that the runtime-vs-config mismatch is a deliberate "ready when adapter ships" choice. If the latter, document at the `DreamsConfig::default_cli_priority` comment.

---

## 4. What looked good (worth recording)

These are the deliberate design choices Codex got right and should not regress while fixing the above.

- **9-tool MCP registry.** `crates/memoryd/src/mcp.rs:220-265` correctly exposes only agent-facing tools (`Search/Get/Note/Write/Supersede/Forget/Reveal/Startup/Observe`); `dream_now`/`dream_status` and other admin commands are CLI-only as spec §3.1 requires.
- **`memory_note` is genuinely unchanged.** `crates/memoryd/src/handlers.rs:380-411` still writes a canonical memory and only that. The v0.1→v0.2 fix to keep this behavior was honored.
- **`memory_observe` runs Stream D classification before any disk effect.** `crates/memoryd/src/handlers.rs:440` — refuses Secret early, routes PII to encrypted, plaintext to plaintext. Matches spec §2 invariant 8.
- **Path encoding without colons on disk, with colons on the wire.** `crates/memoryd/src/dream/scope.rs:42-48` produces `dreams/journal/project/<id>/<date>.md`; `scope.as_str()` returns `project:<id>` for the synthetic namespace_prefix. Spec §1.1 fix #11 honored.
- **`MaskingSession` is genuinely deterministic.** `crates/memory-privacy/src/masking.rs:75-84` — counter-keyed token assignment, BTreeMap iteration order is stable. The `EchoCli` lookup-by-prompt-hash flow works precisely because of this. Fragile coupling but it does hold.
- **Hallucination detection is strict and deterministic.** Pass 2 evidence catalog lookup (`crates/memoryd/src/dream/pass2.rs:191-195`) does the `(kind, ref)` tuple match. Pass 3 entity allowlist (`crates/memoryd/src/dream/pass3.rs:166-175`) does the same for entity ids. Both reject hallucinations as deterministic refusals, not LLM judgment.
- **`safe_plaintext_fragment` defense-in-depth.** Applied to Pass 1 output (`pass1.rs:40`), Pass 3 questions (`pass3.rs:98`), and restored Pass 2 candidates (`orchestration.rs:153`). Three independent safety nets.
- **Substrate-surface routing.** `is_noncanonical_stream_f_repo_path` is correctly checked **before** `MEMORY_PREFIXES` in `validate_repo_relative_path` (per substrate teammate, model.rs:1099 before 1102). Dream files cannot accidentally enter canonical parsing.
- **`enforce_no_dream_prose_sources` covers both write paths.** Per substrate teammate: applied to both `write_memory` and `supersede_memory`. No supersede-route bypass.
- **Drop observer instrumentation.** `crates/memoryd/src/dream/masking.rs:11-19` lets tests assert `MaskingSession::Drop` ran without changing production code. Clean shape.
- **Subprocess hardening.** `crates/memoryd/src/dream/harness.rs:69-122` (`MinimalEnvironment`) correctly clears env, applies a documented allowlist, and forces `TERM=dumb`. Per-adapter narrowing via `for_adapter(path_env, allowlist)` provides defense-in-depth so a misconfigured `DOCUMENTED_ENV_ALLOWLIST` can't leak `OPENAI_API_KEY` into a Claude subprocess.
- **SIGTERM-then-SIGKILL with a 2s grace.** `crates/memoryd/src/dream/harness.rs:471-498`. The `unsafe extern "C"` block is documented with a SAFETY note (line 515-517).
- **Argv-fallback is declared per-adapter.** Spec §2 invariant 10 honored. The current built-in adapters (`ClaudeCodeCli`, `CodexCli`) declare `PromptTransport::Stdin`; tests assert argv never contains the prompt (`dream_harness_cli.rs:36`).
- **Stream A surface additions are spec-scoped.** Per substrate teammate: every new merge-driver branch is dispatched cleanly per path family; range-checks cover every numeric `DreamsConfig` field; `validate_substrate_fragment_append` enforces classification/payload alignment.

---

## 5. Recommended task ordering for the planner

Suggested sequencing (Codex's planner can re-order based on dependency graph):

1. **B7, B8** — substrate-level tests (no production code changes; can land first as regression coverage).
2. **B6** — `RepoPath::try_new` swap (small, isolated; one file).
3. **B4** — cleanup → `Substrate::write_memory`. Architecturally important; depends on confirming `Substrate::write_memory(UpdateExisting)` semantics.
4. **B5** — encrypted-substrate descriptor projection. Coordinate with Stream D; may need a small `memory-privacy` API addition.
5. **B3** — scheduled-lease retry sleeper trait + actual waiting. Depends on B1's wiring choice (daemon worker vs CLI) so do B1 first.
6. **B1, B2** — wire `run_scheduled_lease` and `run_cleanup` into CLI subcommands (or daemon-internal worker — pick one). Add contract tests to `dream_cli.rs`.
7. **R1, R7, R9, R10, R11, R12, R13** in any order. Independent.
8. **R2, R3, R4, R5, R6, R8** in any order. Independent.
9. **N\*** swept up alongside whichever blocker/risk touches the same file.

Each task should land in its own per-task worktree per the Stream A workflow (`../agent-memory-wt/task-NN/` on `stream-f-fix/task-NN-<slug>` branches). Per CLAUDE.md, full `scripts/check.sh` runs only on the integrated trunk after `integrate-task-worktree.sh` fast-forwards `main` — workers run only their narrow gate.

---

## 6. Reviewer's caveats and uncertainty

Where I might be wrong, called out so Codex can verify before acting:

- **B5 (encrypted descriptors).** I'm assuming Stream D ships a content-aware safe projection for encrypted canonical memories that Stream F should reuse. If Stream D currently uses content-blind descriptors there too, the fix is bigger than I scoped — both sides need the projection function, and a Stream D spec amendment may be appropriate. Codex should grep `crates/memory-privacy/` for the existing projection before committing to a path.
- **R1 (`Trusted` hardcoded).** The `dreaming-strict` policy may be intentionally conservative — refusing anything that would need encryption, period. If that's the design choice, document it in spec §6.3 step 6 and keep the current behavior; the "fix" is documentation, not code.
- **B4 (cleanup → substrate API).** I assumed `Substrate::write_memory(WriteMode::UpdateExisting)` exists and supports cleanup's needs. If it doesn't, that's an API addition; if it has surprising semantics around classification rerun for unchanged bodies, raise the question to Trey before plumbing.
- **R7 (panic-path Drop test).** Rust does guarantee Drop on unwind for stack values. This is a spec-compliance gap, not a runtime correctness gap. If Codex's planner deems it not worth the test cost, downgrade it to a doc note.
- I did not exercise the gate myself (`bash scripts/check.sh` was not run by this reviewer). Codex's gate report records PASS at `4f96dfb`. None of the issues above should change that gate's outcome; they are operational/correctness gaps not caught by the gate's surface.

---

## 7. Bottom line

Codex shipped a structurally complete Stream F with sound architecture and good narrow-test coverage. The blockers are all wiring/contract gaps — not deep flaws — but they prevent Stream F from delivering its primary "memoryful over time" promise in a deployed memoryd:

- the daily/scheduled half (B1, B2) doesn't run;
- the cleanup mutations bypass Stream A's contract (B4);
- encrypted-substrate fragments contribute zero signal to dreaming (B5);
- two substrate invariants aren't covered by substrate-level tests (B7, B8).

Once those land plus the smaller (B3, B6) blockers, Stream F should be genuinely shipping-ready. Risk and nit categories are improvements, not gates.

Hand this report to Codex; the planner can convert the blockers list into a per-task worktree plan in the established style.
