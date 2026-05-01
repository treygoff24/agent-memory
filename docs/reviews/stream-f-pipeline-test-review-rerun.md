# Stream F Review Gate C - Test-Hardening Rerun

**Role:** test-hardening rerun for Tasks 8-11  
**Prior report:** `docs/reviews/stream-f-pipeline-test-review.md`  
**Scope:** verify prior severity-1/2 test-hardening findings are closed for harness CLI, Pass 2 validation/status, scheduled retry-to-run seam, grounding rehydration file refs, and prompt determinism gate coverage.  
**Result:** PASS

No severity-1 or severity-2 test gaps remain for Tasks 8-11 based on the current tests and the commands below.

## Commands run

| Command                                                                                                                                                                          |        Result | Notes                                                                                                                                                                                                       |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------: | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | --------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | ---------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- | ------------------------------------------------------------- | -------- | ------------------------------------------------------ | ---- | ----------------------------------------------- |
| `cargo test -p memoryd --test dream_harness_cli --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_lease_scheduled_retry --test dream_scope_and_prompts` |          PASS | Integrated rerun including prompt determinism: `dream_grounding_rehydration` 11/11, `dream_harness_cli` 8/8, `dream_lease_scheduled_retry` 6/6, `dream_pass_pipeline` 13/13, `dream_scope_and_prompts` 3/3. |
| `for i in 1 2 3; do echo "--- harness run $i ---"; cargo test -p memoryd --test dream_harness_cli                                                                                |               | exit $?; done`                                                                                                                                                                                              | PASS      | Three consecutive normal Cargo test-binary runs passed under default test parallelism; no reproduction of the prior pid-marker/timeout flake. |
| `cargo test -p memory-substrate --test frontmatter_schema`                                                                                                                       |          PASS | 16/16 passed; keeps `grounding_rehydration_required` frontmatter coverage green.                                                                                                                            |
| `cargo test -p memoryd --test dream_lease_election --test dream_lease_scheduled_retry`                                                                                           |          PASS | Lease election 8/8 and scheduled retry 6/6 passed.                                                                                                                                                          |
| `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`                                                                                                            |          PASS | No warnings.                                                                                                                                                                                                |
| `rg -n "hardened_subprocess                                                                                                                                                      |  non.?reading | large prompt                                                                                                                                                                                                | timeout   | TERM                                                                                                                                          | SIGKILL                                                                                       | pid        | ready" crates/memoryd/tests/dream_harness_cli.rs`                                                        | PASS                                                                                                   | Inspection command for harness closure evidence.                        |
| `rg -n "empty                                                                                                                                                                    |       refused | skipped                                                                                                                                                                                                     | schema    | namespace                                                                                                                                     | kind                                                                                          | confidence | cap                                                                                                      | max                                                                                                    | excerpt                                                                 | mixed                                                         | accepted | evidence" crates/memoryd/tests/dream_pass_pipeline.rs` | PASS | Inspection command for Pass 2 closure evidence. |
| `rg -n "callback                                                                                                                                                                 |     scheduled | retry                                                                                                                                                                                                       | recovered | run                                                                                                                                           | lease" crates/memoryd/tests/dream_lease_scheduled_retry.rs crates/memoryd/src/dream/lease.rs` | PASS       | Inspection command for scheduled retry closure evidence.                                                 |
| `rg -n "file:                                                                                                                                                                    | repo-relative | missing.\*file                                                                                                                                                                                              | drift     | valid.\*file                                                                                                                                  | threshold                                                                                     | promot     | quarantine                                                                                               | rehydrat" crates/memoryd/tests/dream_grounding_rehydration.rs crates/memoryd/src/dream/rehydration.rs` | PASS                                                                    | Inspection command for file-ref rehydration closure evidence. |
| `rg -n "determin                                                                                                                                                                 |        prompt | evidence_catalog                                                                                                                                                                                            | scratch   | mask                                                                                                                                          | canonical                                                                                     | cargo test | dream_scope" crates/memoryd/tests/dream_scope_and_prompts.rs docs/plans/2026-04-30-stream-f-dreaming.md` | PASS                                                                                                   | Inspection command for prompt determinism and documented gate evidence. |

## Severity findings

None.

## Closure evidence

### S1-1 closure: harness subprocess tests are deterministic under normal Cargo parallelism

- The harness tests now serialize subprocess-sensitive cases with `SUBPROCESS_TEST_LOCK` at `crates/memoryd/tests/dream_harness_cli.rs:52-53`, `crates/memoryd/tests/dream_harness_cli.rs:121-122`, and `crates/memoryd/tests/dream_harness_cli.rs:168-169`.
- The original timeout regression remains covered by `hardened_subprocess_timeout_terminates_child`, which writes a pid marker, loops via `sleep`, expects `HarnessCliError::Timeout`, and asserts the child is gone at `crates/memoryd/tests/dream_harness_cli.rs:129-163`.
- The prior missing edge case is now covered by `hardened_subprocess_timeout_covers_non_reading_child_with_large_prompt`: it starts a child that never reads stdin, sends a large prompt, wraps the harness call in an outer timeout, and asserts the harness timeout remains bounded and the child is not alive at `crates/memoryd/tests/dream_harness_cli.rs:176-219`.
- Runtime evidence: the integrated gate passed once, and `dream_harness_cli` passed three additional consecutive normal Cargo test runs.

### S1-2 / S2-1 closure: empty evidence and all-refused Pass 2 outputs are skipped before governance

- Empty evidence is now a behavior test: `pass_2_rejects_empty_evidence_before_governance_and_marks_all_refused_skipped` asserts `PassStatus::Skipped`, `no_candidates_accepted`, refusal reason `missing_evidence_ref`, `source_ref_count == 0`, and no candidate writes at `crates/memoryd/tests/dream_pass_pipeline.rs:159-182`.
- The implementation rejects empty evidence before catalog validation at `crates/memoryd/src/dream/pass2.rs:121-129`.
- Pass 2 outcome semantics now derive success from at least one accepted candidate; otherwise status is skipped with `no_candidates_accepted` at `crates/memoryd/src/dream/pass2.rs:161-168`.
- Empty candidate arrays are separately covered as skipped at `crates/memoryd/tests/dream_pass_pipeline.rs:296-306`.

### S2-2 closure: Pass 2 schema/kind/confidence/cap validation and excerpt restoration are covered

- Out-of-scope namespace, negative confidence, confidence above one, and invalid kind are table-tested as skipped/refused with no governance writes at `crates/memoryd/tests/dream_pass_pipeline.rs:185-255`.
- Candidate-array cap enforcement is tested by setting `pass_2_max_candidates = 1`, returning two proposals, and asserting both are refused as `too_many_candidates` with no writes at `crates/memoryd/tests/dream_pass_pipeline.rs:258-293`.
- Mixed accepted/refused behavior is covered: one hallucinated ref is refused, one valid candidate is accepted, status is `Success`, and only one write occurs at `crates/memoryd/tests/dream_pass_pipeline.rs:310-346`.
- Excerpt restoration is present in the DTO and implementation at `crates/memoryd/src/dream/run.rs:69-75` and `crates/memoryd/src/dream/pass2.rs:136-147`, and is asserted in both the valid-ref and mixed tests at `crates/memoryd/tests/dream_pass_pipeline.rs:74-105` and `crates/memoryd/tests/dream_pass_pipeline.rs:321-346`.

### S2-3 closure: scheduled retry proves recovered lease invokes a dream-run callback

- The scheduled retry seam test `recovered_scheduled_lease_invokes_dream_run_callback` forces an initial fetch failure, recovery on retry, and records the callback's scope/run id at `crates/memoryd/tests/dream_lease_scheduled_retry.rs:32-56`.
- The implementation path invokes `run_dream(&lease)?` only after successful lease acquisition at `crates/memoryd/src/dream/lease.rs:174-188`.
- Runtime evidence: `cargo test -p memoryd --test dream_lease_election --test dream_lease_scheduled_retry` passed with scheduled retry 6/6.

### S2-4 closure: grounding rehydration covers missing, drifted, and valid file refs

- Missing repo-relative file refs quarantine on approval at `crates/memoryd/tests/dream_grounding_rehydration.rs:65-76`.
- Missing `file:` scheme refs quarantine on approval at `crates/memoryd/tests/dream_grounding_rehydration.rs:79-90`.
- Drifted file refs exercise a configured threshold and quarantine at `crates/memoryd/tests/dream_grounding_rehydration.rs:93-106`.
- Valid `file:` refs promote on approval at `crates/memoryd/tests/dream_grounding_rehydration.rs:109-121`.
- Runtime evidence: `dream_grounding_rehydration` passed 11/11 inside the integrated rerun.

### Prompt determinism closure

- Prompt determinism is tested by rendering Pass 1, Pass 2, and Pass 3 twice from the same input and asserting byte equality at `crates/memoryd/tests/dream_scope_and_prompts.rs:50-117`.
- The same test verifies Pass 2 includes `evidence_catalog` and stable refs while Pass 1/3 do not at `crates/memoryd/tests/dream_scope_and_prompts.rs:119-123`.
- The contract map includes `dream_scope_and_prompts`, `dream_harness_cli`, and `dream_pass_pipeline` in the same Task 8/10 narrow-gate row at `docs/reviews/stream-f-contract-map.md:141-145`.
- Runtime evidence: the integrated rerun command included `--test dream_scope_and_prompts` and passed 3/3.

## Non-blocking notes

- The working tree was already dirty before this rerun. I did not edit source; this report is the only intended write from this review.
- The historical task-plan Gate C one-liner still lists only `dream_harness_cli`, `dream_pass_pipeline`, and `dream_grounding_rehydration` at `docs/plans/2026-04-30-stream-f-dreaming.md:705-708`. This rerun treated the integrated gate as the expanded command above, consistent with the current contract-map evidence. If the plan itself must be normalized, that is a docs-only follow-up outside this no-source-edit rerun.

## Final verdict

**PASS.** The prior severity-1/2 test-hardening findings are closed, the focused rerun commands are green, and no severity-1/2 test gaps remain for Tasks 8-11.
