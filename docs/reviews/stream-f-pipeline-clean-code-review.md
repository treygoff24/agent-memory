# Stream F Review Gate C - Clean-code / correctness review

**Scope:** Tasks 8-11 harness, lease, pass pipeline, masking, evidence validation, and dream grounding rehydration.

**Contract:** `docs/specs/stream-f-dreaming-v0.2.md` and `docs/plans/2026-04-30-stream-f-dreaming.md`.

**Verdict:** FAIL

Gate C should not advance yet. The targeted tests and clippy command are green, but the implementation still has contract-level correctness gaps around stdin timeout enforcement, Pass 2 validation/status semantics, and active lease handling. These are not style issues; they can produce hung dream runs, out-of-scope/no-evidence candidate writes, misleading operator reports, and duplicate same-device journal runs.

## Commands run

```bash
cargo test -p memoryd --test dream_harness_cli --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration && cargo test -p memory-substrate --test frontmatter_schema
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Results:

- `dream_harness_cli`: PASS, 6 tests.
- `dream_lease_election`: PASS, 6 tests.
- `dream_lease_scheduled_retry`: PASS, 4 tests.
- `dream_pass_pipeline`: PASS, 8 tests.
- `dream_grounding_rehydration`: PASS, 7 tests.
- `frontmatter_schema`: PASS, 16 tests.
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`: PASS.

## Intended outcome

Tasks 8-11 appear intended to add the safe harness subprocess boundary, lease acquisition/retry behavior, three-pass masked dream pipeline, Pass 2 candidate queue write path, and deterministic grounding rehydration guard for dream-authored candidates. The business outcome is a dream run that can safely delegate to local agent CLIs without leaking prompts, can elect one device per scope/day, can report model/governance refusals accurately, and cannot promote stale or ungrounded dream-authored candidates.

## Executive summary

The implementation has a good modular start and the narrow behavior tests prove important happy paths, but it is not merge-ready for Gate C. The largest issue is that prompt stdin writing happens synchronously before timeout supervision begins, so a child that does not drain stdin can hang the dream subprocess path despite the timeout contract. Pass 2 validation also accepts candidates that are outside the leased scope or cite no evidence, and reports all- refused/no-accepted runs as `Success` despite the spec's `Skipped` requirement. Lease election only blocks active foreign leases, not any active lease for the same scope, so the same device can rerun within the lease window and overwrite/rewrite same-date outputs without `--force`.

## Findings

### [High] Reliability - Stdin prompt writes can hang before timeout enforcement starts

- **Evidence:** `crates/memoryd/src/dream/harness.rs:416-420` spawns the child and synchronously calls `stdin.write_all(prompt.as_bytes())`; `crates/memoryd/src/dream/harness.rs:423-428` starts stdout/stderr readers and `wait_with_timeout` only after that write completes. The v0.2 contract requires harness calls to fail closed on timeout and the plan explicitly requires timeout kill behavior.
- **Why it matters:** A harness process that exits early, stops reading stdin, or only partially drains stdin can block the daemon's blocking worker before timeout supervision is active. Large dream prompts are plausible because Pass 1/2 include substrate fragments, active memories, and Pass 1 markdown. In the worst case, `memoryd dream now` can hang indefinitely instead of releasing the lease and reporting `timeout`.
- **Reasoning:** Pipe writes can block when the child does not consume stdin and the pipe buffer fills. Because the code writes the whole prompt before `wait_with_timeout`, the timeout cannot kill the child while `write_all` is blocked. The existing timeout test passes because it uses a small prompt and a child shape that does not exercise pipe backpressure.
- **Recommendation:** Move stdin writing under timeout supervision. Options: spawn a dedicated stdin writer thread/task before `wait_with_timeout`, close stdin after write, and let the timeout kill both child and writer path; or use async `tokio::process` with `tokio::time::timeout` around the whole interaction. Add a test with a child that does not read stdin and a prompt larger than the pipe buffer, asserting the configured timeout is honored.
- **Confidence:** High.

### [High] Business Logic - Pass 2 candidates can bypass in-scope namespace and non-empty evidence validation

- **Evidence:** The spec requires Pass 2 validation to check that `namespace` matches the leased/in-scope namespace and that evidence refs come from the catalog (`docs/specs/stream-f-dreaming-v0.2.md:662-669`); the prompt schema also says `evidence` is a non-empty array and `confidence` is finite `[0, 1]` (`crates/memoryd/src/dream/prompts.rs:92-101`). The implementation only calls `catalog.first_invalid_ref(&proposal.evidence)` before writing (`crates/memoryd/src/dream/pass2.rs:49-70`). Empty evidence returns `None` and is written; any `namespace` string is forwarded unchanged.
- **Why it matters:** A model response can write a dream candidate into `me`, `agent`, another project, or an org while the current run holds a lease for `project:proj_abc`. It can also write a candidate with zero source refs, defeating the grounding/evidence requirement and making later rehydration ineffective because there is nothing to re-resolve.
- **Reasoning:** `EvidenceCatalog::first_invalid_ref` only rejects evidence entries whose `(kind, ref)` tuple is not in the catalog (`crates/memoryd/src/dream/pass2.rs:131-136`). It does not reject an empty evidence vector, out-of-scope namespace, or out-of-range confidence. `CandidateWriteRequest` then carries the unvalidated fields directly to the candidate writer (`crates/memoryd/src/dream/run.rs:57-66`).
- **Recommendation:** Add a deterministic `validate_candidate` step before restoration/write that rejects: namespace not equal to the run's scope namespace, empty evidence, any ref not in the catalog, non-finite or out-of-range confidence, and invalid/unsupported candidate kind if governance expects a constrained set. Record each rejection in `CandidateWriteResult { accepted: false, reason: <stable_code>, source_ref_count }` and do not call the writer for invalid candidates. Add tests for out-of-scope namespace, empty evidence, and confidence outside `[0, 1]`.
- **Confidence:** High.

### [Medium] Correctness - Pass 2 reports all-refused or zero-candidate runs as success instead of skipped

- **Evidence:** The v0.2 contract says Pass 2 success requires at least one candidate accepted into the queue and zero accepted is `pass_2: skipped` so operators can inspect refusals (`docs/specs/stream-f-dreaming-v0.2.md:669-670`). `run_pass_2` always returns `PassStatus::Success` after parsing, regardless of `candidate_results` content or acceptance count (`crates/memoryd/src/dream/pass2.rs:73-79`). The hallucinated-ref test currently asserts `Success` for a fully refused run (`crates/memoryd/tests/dream_pass_pipeline.rs:159-167`), locking in the wrong behavior.
- **Why it matters:** Operators and scheduled-run summaries will report a successful Pass 2 even when no candidate entered the queue. That obscures governance/model refusal rates and makes `memoryd dream review`/status less trustworthy.
- **Reasoning:** The function distinguishes malformed JSON failure but does not derive status from accepted results. This contradicts the explicit structured result requirement: refusals should be reported, but the pass status should not imply candidate-queue progress.
- **Recommendation:** Compute `accepted_count = candidate_results.iter().filter(|r| r.accepted).count()`. Return `PassStatus::Success` only when `accepted_count > 0`; otherwise return `PassStatus::Skipped` with candidate refusals preserved and a stable `error_code`/reason such as `no_candidates_accepted` if the protocol wants one. Update the hallucinated-ref test to expect `Skipped` and add a zero-candidate `[]` test.
- **Confidence:** High.

### [Medium] Concurrency - Same-device active leases are ignored unless the active holder is foreign

- **Evidence:** The spec's lease algorithm says to filter active records for scope X and abort if the subset is non-empty, with `--force` as the override (`docs/specs/stream-f-dreaming-v0.2.md:562-570`). The implementation calls `active_foreign_lease` (`crates/memoryd/src/dream/lease.rs:135-137`) and that helper only matches `record.device != device_id` (`crates/memoryd/src/dream/lease.rs:237-247`).
- **Why it matters:** The same device can run `memoryd dream now --scope X` repeatedly within the active lease window without `--force`. That weakens the election invariant, can produce multiple lease records for the same device/scope/window, and can rerun same-date Pass 1/3 writes that are supposed to be protected by the lease.
- **Reasoning:** The lease file is the concurrency primitive for scope/day work, not only a cross-device blocker. Allowing same-device reacquisition means accidental double-submit or scheduler/manual overlap bypasses the normal lease-held signal.
- **Recommendation:** Replace `active_foreign_lease` with an active-scope lease check that returns any active record for the scope when `force == false`. If the holder is the same device, return `lease_held` with `by_device = device_id` or a distinct stable reason if the protocol needs one. Add a test seeding an active same-device lease and asserting manual acquisition fails unless `--force` is supplied.
- **Confidence:** Medium-high.

### [Low] Maintainability - Harness module is doing too many jobs for a clean boundary

- **Evidence:** `crates/memoryd/src/dream/harness.rs` is 598 lines and contains trait definitions, adapter registry-facing types, environment filtering, process spawning, timeout/kill logic, stdout/stderr capture, prompt redaction, executable discovery, FFI signal handling, and JSON validation.
- **Why it matters:** The highest-risk code in this gate is subprocess containment. Keeping adapter declarations, environment policy, IO pump behavior, and kill semantics in one large module makes it easier for future adapter changes to regress the safety boundary.
- **Reasoning:** This is not currently blocking by itself, but it compounds the timeout bug above and makes targeted tests harder to reason about. Clean-code review favors smaller modules with one reason to change.
- **Recommendation:** After the correctness fixes, split the module into at least `harness/adapters.rs`, `harness/command.rs` or `subprocess.rs`, and `harness/env.rs`, preserving public exports through `dream::harness` if desired. Keep the subprocess runner small and heavily tested.
- **Confidence:** Medium.

## Non-blocking simplifications

- `DreamRunner::preview_pass_2_prompt` and `preview_pass_3_prompt` build a masked input twice to seed the masking table (`crates/memoryd/src/dream/run.rs:142-153`). That is understandable for determinism, but a helper like `build_prompt_preview_input(options, pass_1_markdown)` would make the intent clearer and avoid accidental divergence between previews and runtime.
- `pass3::parse_valid_records` currently classifies empty entities, hallucinated entities, empty questions, unmasked private values, and malformed JSON under the same `malformed_record` counter (`crates/memoryd/src/dream/pass3.rs:75-88`). If later status reporting uses omission counters, splitting stable reasons would make operator diagnostics better. This can wait until the Stream E pending-attention hook work if not needed now.

## Test gaps

- Add a harness timeout/backpressure test where the child does not read stdin and the prompt exceeds the pipe buffer; assert timeout returns instead of hanging.
- Add Pass 2 validation tests for out-of-scope namespace, empty evidence, and confidence outside `[0, 1]`.
- Update the hallucinated-ref/all-refused Pass 2 test to expect `Skipped`, and add a parsed empty-array test.
- Add a same-device active lease test to prove manual acquisition fails without `--force`.
- Add a narrow integration test proving `DreamNow` over the daemon protocol once that public protocol path is implemented; current handlers return `not_implemented` for `DreamNow`/`DreamStatus` (`crates/memoryd/src/handlers.rs:119-124`). This may belong to the later CLI/admin task, but the protocol variants already exist.

## Questions / uncertainties

- `memoryd dream now` currently only acquires a lease and returns a stub pass report (`crates/memoryd/src/main.rs:183-208`; `crates/memoryd/src/dream/lease.rs:300-309`). That appears intentional for Task 9's scope, because Task 10's pass pipeline is separately testable and not wired into CLI execution yet. If Review Gate C is meant to validate end-to-end manual dream execution, this is a larger missing integration.
- The grounding rehydration hook is correctly limited to dream-authored candidates with the marker (`crates/memoryd/src/dream/rehydration.rs:67-69`; `crates/memoryd/src/handlers.rs:1012-1019`). I did not inspect every governance write path to prove there is no other approval route bypassing `review_decision_response`.

## Positives

- The harness adapters truthfully declare stdin transport for v0.2 and the tests cover argv exclusion, scratch cwd, minimal env, and no Argv adapters.
- Grounding rehydration is deterministic, dream-only by predicate, and has good coverage for missing, aged, drifted, inactive, dream-prose, valid, and non-dream cases.
- Pass 1/3 masked-output persistence and Pass 2 restoration are covered by behavior tests, including drop-observer checks for success and failure paths.
