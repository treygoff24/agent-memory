# Stream F Review Gate C - Test-Hardening Review

**Role:** test-hardening review for Tasks 8-11  
**Scope:** harness CLI, lease election/retry, pass pipeline, masking, evidence validation, grounding rehydration, and frontmatter marker tests.  
**Result:** FAIL

Gate C is not ready to pass. The current tests cover several important happy paths, but severity-1/2 gaps remain and the required Gate C test command is not reliably green.

## Commands run

| Command                                                                                                        | Result | Notes                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------- | -----: | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo test -p memoryd --test dream_harness_cli --test dream_pass_pipeline --test dream_grounding_rehydration` |   FAIL | `dream_grounding_rehydration` passed 7/7, then `dream_harness_cli` failed in `hardened_subprocess_timeout_terminates_child` with missing pid marker. |
| `cargo test -p memoryd --test dream_pass_pipeline`                                                             |   PASS | 8/8 passed.                                                                                                                                          |
| `cargo test -p memoryd --test dream_lease_election --test dream_lease_scheduled_retry`                         |   PASS | 6/6 lease election and 4/4 scheduled retry tests passed.                                                                                             |
| `cargo test -p memory-substrate --test frontmatter_schema`                                                     |   PASS | 16/16 passed.                                                                                                                                        |
| `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`                                          |   PASS | No clippy warnings.                                                                                                                                  |
| `cargo test -p memoryd --test dream_harness_cli hardened_subprocess_timeout_terminates_child -- --nocapture`   |   PASS | The same timeout test passed when filtered to one test, which supports a parallel-test flake diagnosis.                                              |
| `cargo test -p memoryd --test dream_harness_cli`                                                               |   FAIL | Failed again: timeout test missing pid marker; stdin/env/cwd recorder test timed out after 2s.                                                       |

## Severity 1 findings

### S1-1. Gate C's required harness test is flaky/failing under the normal test runner

The Gate C command cannot be treated as green while `dream_harness_cli` fails under the normal test binary run. On one full Gate C run, `hardened_subprocess_timeout_terminates_child` failed because the pid marker was missing; on a second full `dream_harness_cli` run, that test failed again and `hardened_subprocess_sends_prompt_only_on_stdin_with_minimal_env_and_scratch_cwd` timed out. The filtered timeout test passed by itself, so the suite is order/parallelism sensitive.

Evidence:

- Gate C requires `cargo test -p memoryd --test dream_harness_cli --test dream_pass_pipeline --test dream_grounding_rehydration` in `docs/plans/2026-04-30-stream-f-dreaming.md:705-709`.
- The harness timeout test creates a busy child and then immediately expects a pid marker to exist at `crates/memoryd/tests/dream_harness_cli.rs:122-156`.
- The stdin/env/cwd recorder test has a fixed 2s timeout at `crates/memoryd/tests/dream_harness_cli.rs:79-93` and failed under the full harness test run.
- The same timeout test passed when run alone, which means the test does not meet repeatable/FIRST test expectations.

Why this matters:

- This is a release-gate test. If it is nondeterministic under the default cargo test runner, Gate C can fail spuriously or hide real subprocess regressions behind reruns.
- The test also does not verify the full SIGTERM/SIGKILL contract: the script installs a `TERM` trap at `crates/memoryd/tests/dream_harness_cli.rs:124-128`, but assertions only check `HarnessCliError::Timeout` and that the process is no longer alive at `crates/memoryd/tests/dream_harness_cli.rs:151-156`. An implementation that skipped SIGTERM and went straight to kill could still pass.

Required hardening:

- Make subprocess tests deterministic under normal cargo parallelism, for example by avoiding CPU-burning loops, synchronizing on a ready marker before timeout countdown assertions, or serializing this test module if necessary.
- Assert the TERM marker is written before final non-liveness, and add a deterministic child that ignores TERM to prove the SIGKILL fallback path.

### S1-2. Pass 2 evidence validation can be bypassed by an empty evidence array, and tests would still pass

The spec requires every Pass 2 candidate to cite at least one valid catalog ref. Current tests cover a valid ref and a hallucinated non-empty ref, but they do not cover `evidence: []`. The implementation's validation only searches for an invalid ref inside the provided evidence list; an empty list has no invalid item and proceeds to candidate writing with `grounding_rehydration_required: true` but zero citations.

Evidence:

- Spec requires non-empty evidence and catalog membership at `docs/specs/stream-f-dreaming-v0.2.md:662-668` and reiterates this at `docs/specs/stream-f-dreaming-v0.2.md:672-678`.
- The positive Pass 2 test only uses one valid evidence ref at `crates/memoryd/tests/dream_pass_pipeline.rs:73-82`.
- The negative Pass 2 test only uses one hallucinated non-empty ref at `crates/memoryd/tests/dream_pass_pipeline.rs:141-147`.
- `EvidenceCatalog::first_invalid_ref` only returns a problem when an evidence item exists and is not in the catalog at `crates/memoryd/src/dream/pass2.rs:131-136`.
- `run_pass_2` writes the candidate after that check at `crates/memoryd/src/dream/pass2.rs:49-70`; with an empty vector, the check passes and `source_ref_count` becomes 0.
- Rehydration loops over collected citations and returns success when there are no citations at `crates/memoryd/src/dream/rehydration.rs:59-64` and `crates/memoryd/src/dream/rehydration.rs:165-171`.

Why this matters:

- A dream-authored candidate can enter the candidate queue with no grounding refs despite the central Stream F grounding invariant.
- Later grounding rehydration cannot protect this case because there is nothing to re-resolve.

Required hardening:

- Add a behavior test where Pass 2 returns `evidence: []`; assert the candidate is refused before the writer/governance layer, `source_ref_count == 0`, and a stable reason such as `missing_evidence_ref` is reported.
- Add a corresponding implementation guard before `writer.write_candidate`.

## Severity 2 findings

### S2-1. Tests assert the wrong Pass 2 outcome when every candidate is refused

The spec says zero accepted Pass 2 candidates is `pass_2: skipped`, not success. The current hallucinated-ref test asserts `PassStatus::Success` even though its only candidate is refused. The implementation also always returns success after parsing, regardless of whether any candidate was accepted.

Evidence:

- Spec: zero accepted is skipped at `docs/specs/stream-f-dreaming-v0.2.md:669-670`.
- Current hallucinated-ref test asserts success and then a refused candidate result at `crates/memoryd/tests/dream_pass_pipeline.rs:159-167`.
- Implementation always returns `PassStatus::Success` at `crates/memoryd/src/dream/pass2.rs:73-79`.

Why this matters:

- Operators and later `memoryd dream review` logic can be told a pass succeeded even when nothing entered the queue.
- This is exactly a "test passes despite wrong behavior" case.

Required hardening:

- Change/augment the refused-all test to assert `PassStatus::Skipped` when no candidate result has `accepted == true`.
- Add a mixed-output test: one invalid candidate and one accepted candidate should be `Success` and preserve both candidate results.

### S2-2. Pass 2 schema/cap validation is materially under-tested

The spec requires validation of namespace, memory kind, confidence range, non-empty evidence, and max candidate count. Current tests only prove catalog membership for one valid and one invalid ref. Implementation structs and tests also omit the evidence `excerpt` field that the spec requires to be restored along with claim and rationale.

Evidence:

- Spec validation requirements are at `docs/specs/stream-f-dreaming-v0.2.md:662-668`.
- Spec candidate evidence includes an `excerpt` field at `docs/specs/stream-f-dreaming-v0.2.md:650-653`, and restore must cover `claim`, `excerpt`, and `rationale` at `docs/specs/stream-f-dreaming-v0.2.md:668`.
- `Pass2Candidate` has only `claim`, `namespace`, `kind`, `evidence`, `confidence`, and `rationale` at `crates/memoryd/src/dream/pass2.rs:112-120`.
- `CandidateEvidenceRef` only carries `kind` and `reference` at `crates/memoryd/src/dream/run.rs:68-73`.
- `CandidateWriteRequest` has no excerpt-bearing field at `crates/memoryd/src/dream/run.rs:56-66`.
- The restore assertions cover claim/rationale only at `crates/memoryd/tests/dream_pass_pipeline.rs:391-399`; there is no excerpt restore assertion.

Why this matters:

- Tests would pass if invalid namespace/kind/confidence/candidate-count values were accepted.
- Masked evidence excerpts could be dropped or remain unrestored without any failing test.

Required hardening:

- Add table-driven Pass 2 rejection tests for out-of-scope namespace, unknown kind, confidence < 0 / > 1, empty evidence, and candidate count over `dreams.pass_2_max_candidates`.
- Extend the candidate DTO/write request path to include evidence excerpts if that is still the intended v0.2 contract, and assert excerpts are restored before persistence.

### S2-3. Scheduled retry tests can pass without proving a scheduled dream actually runs after lease recovery

The acceptance signal says a transient scheduled lease failure eventually succeeds and runs the dream. Current scheduled tests exercise only `run_scheduled_lease`; the success path writes a cleanup summary and returns a report, but the implementation's acquired lease report is explicitly a stub with Pass 1/2/3 all skipped.

Evidence:

- Spec acceptance says scheduled retry eventually succeeds and runs the dream at `docs/specs/stream-f-dreaming-v0.2.md:825-827`.
- The scheduled success test asserts attempts, missed-run reset, fetch calls, and a cleanup summary containing success at `crates/memoryd/tests/dream_lease_scheduled_retry.rs:12-25`.
- `run_scheduled_lease` treats lease acquisition as success and writes cleanup summary at `crates/memoryd/src/dream/lease.rs:171-187`.
- The acquired lease report is a stub with all dream passes skipped at `crates/memoryd/src/dream/lease.rs:300-318`.
- Task 9 called pass execution out of scope at `docs/plans/2026-04-30-stream-f-dreaming.md:568-570`, but after Task 10 integration Gate C should not leave the lease and pass pipeline unjoined without an explicit remaining task/risk.

Why this matters:

- The retry test can be green while the scheduler never executes Pass 1/2/3 after acquiring a lease.
- This weakens the lease/pipeline integration boundary and risks a false Gate C pass.

Required hardening:

- Add a scheduled-run integration seam test with an injected `EchoCli`/runner that proves a recovered scheduled lease invokes Pass 1 and writes the expected journal/questions/candidate outcomes.
- If full scheduler execution is intentionally deferred to Task 14, document that as a non-Gate-C residual risk and add a failing/ignored tracking test or plan note.

### S2-4. Grounding rehydration lacks file-ref acceptance coverage

Task 11 and the spec cover missing/drifted cited files as well as substrate and memory refs. Current rehydration tests cover missing substrate, aged substrate, drifted substrate, inactive memory, valid substrate, non-dream bypass, and dream-prose refusal, but no `file:` or repo-relative file reference is tested.

Evidence:

- Stream F says a cited file missing or content drift quarantines at `docs/specs/stream-f-dreaming-v0.2.md:131-136`.
- Task 11 asks for content drift above threshold quarantine at `docs/plans/2026-04-30-stream-f-dreaming.md:660-664`.
- Current rehydration tests are substrate/memory/dream-prose focused at `crates/memoryd/tests/dream_grounding_rehydration.rs:14-147`.
- Implementation has a separate file-ref branch at `crates/memoryd/src/dream/rehydration.rs:83-87` and file drift logic at `crates/memoryd/src/dream/rehydration.rs:135-152`, but tests do not exercise it.

Why this matters:

- The implementation can regress for `file:` or repo-relative references while all Gate C rehydration tests stay green.

Required hardening:

- Add tests for missing repo-relative file ref, missing `file:` ref, drifted file ref over threshold, and valid file ref promotion.
- Include a config-threshold fixture to prove `dreams.pass_2_drift_threshold` is honored, not just the default.

## Severity 3 findings

### S3-1. Prompt determinism is not part of the Gate C command

The repo has a prompt determinism test in `dream_scope_and_prompts.rs`, but Gate C's specified command excludes it. The user explicitly called out prompt determinism as a regression concern for this review.

Evidence:

- Prompt determinism test exists at `crates/memoryd/tests/dream_scope_and_prompts.rs:50-123`.
- Gate C command only includes `dream_harness_cli`, `dream_pass_pipeline`, and `dream_grounding_rehydration` at `docs/plans/2026-04-30-stream-f-dreaming.md:705-709`.

Recommendation:

- Add `cargo test -p memoryd --test dream_scope_and_prompts` to Gate C or to the Task 10/11 integration gate, since prompt byte stability is part of the pipeline safety story.

### S3-2. Masking-session teardown coverage does not include harness timeout, cancellation, or panic paths

The tests assert Drop after success and after empty Pass 1 output, but the spec says teardown must happen after success, partial failure, full failure, panic, or cancellation.

Evidence:

- Spec teardown matrix is at `docs/specs/stream-f-dreaming-v0.2.md:708-712`.
- Empty Pass 1 failure drop assertion is at `crates/memoryd/tests/dream_pass_pipeline.rs:335-353`.
- Success/failure drop assertion is at `crates/memoryd/tests/dream_pass_pipeline.rs:356-413`.
- The malformed Pass 2 partial-failure test does not attach a drop observer at `crates/memoryd/tests/dream_pass_pipeline.rs:174-211`.

Recommendation:

- Add drop-observer assertions to malformed Pass 2 partial failure and harness timeout failure.
- If panic/cancellation coverage is too invasive, document it as residual risk and keep the session owned by a guard type whose Drop can be unit-tested directly.

### S3-3. Dream-prose refusal test should be parameterized across source/evidence and journal/questions refs

The current test covers journal in `source.reference` and questions in evidence. The implementation appears broader, but a small parameterized matrix would protect the exact safety invariant from future narrowing.

Evidence:

- Dream prose must never be a grounding source at `docs/specs/stream-f-dreaming-v0.2.md:183-187`.
- Current test cases are `source.reference = dreams/journal/...` and evidence ref `dreams/questions/...` at `crates/memoryd/tests/dream_grounding_rehydration.rs:124-147`.
- Enforcement checks both source and evidence refs at `crates/memory-substrate/src/api.rs:1418-1423`.

Recommendation:

- Add table cases for `source.reference` and `evidence[].ref` across both `dreams/journal/...` and `dreams/questions/...`, including `file:` and fragment suffix forms.

## Coverage that is in good shape

- Exact CLI exit code 5 is covered for `lease_held` and `lease_unavailable` through the real `memoryd dream now` binary at `crates/memoryd/tests/dream_lease_election.rs:56-81`.
- Lease commit staging/identity is behavior-level and checks the actual git commit only contains `leases/journal.lease` at `crates/memoryd/tests/dream_lease_election.rs:10-30`.
- The main pass pipeline tests use `EchoCli` and temp repos, not external LLMs, which keeps them deterministic when subprocess tests are excluded.
- Frontmatter `grounding_rehydration_required` defaulting and round-trip are covered at `crates/memory-substrate/tests/frontmatter_schema.rs:29-48`.

## Final verdict

**FAIL.** Gate C has an actual flaky/failing harness test and severity-1/2 test gaps remain around Pass 2 evidence enforcement, Pass 2 outcome semantics, schema/cap validation, scheduled lease-to-pipeline integration, and file-ref rehydration coverage. Do not advance to Task 12 until severity-1/2 items are fixed and the same test-hardening lane reruns cleanly.
