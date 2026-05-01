# Stream F Review Gate C - Clean-code / correctness rerun

**Scope:** Tasks 8-11 harness, lease, pass pipeline, masking, evidence validation, and dream grounding rehydration.

**Prior report:** `docs/reviews/stream-f-pipeline-clean-code-review.md`

**Contract:** `docs/specs/stream-f-dreaming-v0.2.md` and `docs/plans/2026-04-30-stream-f-dreaming.md`.

**Verdict:** PASS

The prior Gate C clean-code/correctness findings are closed. No severity-1/2 findings remain in Tasks 8-11.

## Commands run

```bash
cargo test -p memoryd --test dream_harness_cli --test dream_lease_election --test dream_lease_scheduled_retry --test dream_pass_pipeline --test dream_grounding_rehydration && cargo test -p memory-substrate --test frontmatter_schema
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Results:

- `dream_grounding_rehydration`: PASS, 11 tests.
- `dream_harness_cli`: PASS, 8 tests.
- `dream_lease_election`: PASS, 8 tests.
- `dream_lease_scheduled_retry`: PASS, 6 tests.
- `dream_pass_pipeline`: PASS, 13 tests.
- `frontmatter_schema`: PASS, 16 tests.
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.

## Intended outcome

Tasks 8-11 implement the safe local harness boundary, lease election and scheduled retry behavior, three-pass masked dream pipeline, Pass 2 candidate queue validation/write semantics, and grounding rehydration enforcement for dream-authored candidates. The product outcome is a daily dream run that can use local agent CLIs without prompt leakage, elect exactly one device per scope/window unless forced, reject ungrounded/out-of-scope candidate proposals, report all-refused Pass 2 runs accurately, and quarantine dream candidates whose cited sources no longer rehydrate at approval time.

## Closure evidence

### Harness stdin timeout/backpressure fixed

- Evidence: `crates/memoryd/src/dream/harness.rs:416-428` now spawns stdout/stderr reader threads and a separate stdin writer thread before entering `wait_with_timeout`, so timeout supervision starts without waiting for the entire prompt write to complete.
- Evidence: `crates/memoryd/src/dream/harness.rs:543-554` isolates stdin writing in `spawn_stdin_writer`/`join_stdin_writer`.
- Evidence: `crates/memoryd/tests/dream_harness_cli.rs:167-220` covers a child that never reads stdin with a large prompt and asserts the harness returns a timeout within a bounded outer timeout and leaves no live child process.
- Assessment: Closed. The prior blocking write-before-timeout failure mode is covered by implementation and regression test.

### Pass 2 rejects empty evidence, out-of-scope namespace, invalid confidence/kind/cap; all-refused is skipped

- Evidence: `crates/memoryd/src/dream/pass2.rs:55-73` validates each proposal before constructing a `CandidateWriteRequest`, and only calls the candidate writer after validation passes.
- Evidence: `crates/memoryd/src/dream/pass2.rs:108-130` rejects out-of-scope namespace, unsupported candidate kind, non-finite or out-of-range confidence, empty evidence, and evidence refs absent from the catalog.
- Evidence: `crates/memoryd/src/dream/pass2.rs:161-169` derives Pass 2 status from accepted candidate results: success only when at least one candidate is accepted; otherwise skipped with `no_candidates_accepted`.
- Evidence: `crates/memoryd/tests/dream_pass_pipeline.rs:159-183` covers empty evidence and all-refused skipped semantics.
- Evidence: `crates/memoryd/tests/dream_pass_pipeline.rs:185-256` covers out-of-scope namespace, invalid confidence below/above range, and invalid kind.
- Evidence: `crates/memoryd/tests/dream_pass_pipeline.rs:258-294` covers candidate cap rejection before governance/candidate writer invocation.
- Evidence: `crates/memoryd/tests/dream_pass_pipeline.rs:296-307` covers empty candidate array skipped semantics.
- Evidence: `crates/memoryd/tests/dream_pass_pipeline.rs:349-389` updates hallucinated evidence handling to skipped/no candidates accepted rather than success.
- Assessment: Closed. The prior validation bypass and misleading success semantics are fixed and covered.

### Same-device active leases block unless forced

- Evidence: `crates/memoryd/src/dream/lease.rs:133-139` checks for any active lease for the scope when `force` is false.
- Evidence: `crates/memoryd/src/dream/lease.rs:251-254` implements `active_lease` without excluding the current device.
- Evidence: `crates/memoryd/tests/dream_lease_election.rs:62-96` seeds an active same-device lease, asserts unforced acquisition returns `lease_held`, and asserts `--force` bypasses it.
- Assessment: Closed. Same-device accidental duplicate runs no longer bypass the lease invariant.

### Lease retry rollback avoids failed local lease records/commits

- Evidence: `crates/memoryd/src/dream/lease.rs:146-160` rolls back failed lease attempts after push errors before retrying or returning.
- Evidence: `crates/memoryd/src/dream/git.rs:44-48` implements native rollback with `git reset --hard HEAD~1` for the lease commit.
- Evidence: `crates/memoryd/src/dream/git.rs:103-110` implements scripted rollback by removing the last appended lease record for retry tests.
- Evidence: `crates/memoryd/tests/dream_lease_election.rs:120-141` verifies persistent push rejection leaves the original commit count intact, zero failed lease records, and a clean worktree.
- Evidence: `crates/memoryd/tests/dream_lease_election.rs:143-185` verifies up-to-three retry behavior with a fetch before each retry.
- Assessment: Closed. Failed push attempts no longer leave stray local lease records or commits.

### Scheduled retry has a callback seam proving dream run invocation

- Evidence: `crates/memoryd/src/dream/lease.rs:167-178` exposes `run_scheduled_lease_with_runner`, a testable seam that accepts a dream-run callback.
- Evidence: `crates/memoryd/src/dream/lease.rs:184-201` invokes the callback after successful lease acquisition and writes a success cleanup summary.
- Evidence: `crates/memoryd/tests/dream_lease_scheduled_retry.rs:32-57` proves a recovered scheduled lease invokes the callback exactly once with the acquired lease record.
- Evidence: `crates/memoryd/tests/dream_lease_scheduled_retry.rs:85-116` proves held leases are not retried and do not invoke the dream runner.
- Evidence: `crates/memoryd/tests/dream_lease_scheduled_retry.rs:152-176` proves a disabled retry window does not invoke the callback after the first failure.
- Assessment: Closed. The scheduled lease path now has an explicit dream-run seam and negative coverage for no-run outcomes.

### Grounding rehydration remains covered

- Evidence: `crates/memoryd/tests/dream_grounding_rehydration.rs` passed 11 tests covering valid dream refs, missing substrate refs, aged-out refs, drifted refs/files, inactive memory refs, missing file refs, dream prose ref refusal, and unchanged non-dream behavior.
- Evidence: `crates/memory-substrate/tests/frontmatter_schema.rs` passed the `dream_authored_candidate_frontmatter_supports_grounding_marker` and `grounding_rehydration_required_defaults_false_and_round_trips` coverage in the 16-test frontmatter suite.
- Assessment: Still covered. No new Gate C severity-1/2 issue found on this surface.

## Severity findings

No severity-1/2 findings remain.

## Non-blocking simplifications

- `crates/memoryd/src/dream/harness.rs` remains a large mixed-responsibility module containing adapter types, env policy, subprocess IO, timeout/kill logic, redaction, executable discovery, and JSON validation. This is not release-blocking after the timeout fix, but a later split into adapter/env/subprocess modules would make the safety boundary easier to audit.
- `crates/memoryd/src/dream/run.rs:145-156` still builds preview prompt input twice for Pass 2/3 to seed masking deterministically. The behavior is understandable; a helper that names this intent would reduce future accidental divergence between preview and runtime paths.

## Test gaps

No blocking Gate C test gaps remain for the prior findings. The rerun specifically verified regression coverage for stdin backpressure timeout, Pass 2 validation/status semantics, same-device lease blocking, push-race rollback, scheduled dream-run callback invocation, and grounding rehydration.

## Questions / uncertainties

- This rerun did not broaden into later Stream F Tasks 12-17. `memoryd dream now` lease acquisition and the pass pipeline are still partly separate surfaces per the Task 9 out-of-scope note in `docs/plans/2026-04-30-stream-f-dreaming.md:570`; end-to-end CLI dream execution should be reviewed when the later CLI/admin task wires it fully.
- The worktree contains many pre-existing modified/untracked files outside this rerun report. I did not edit source files.

## Positives

- The fixes are targeted to the prior failure modes and backed by behavior tests rather than implementation-only assertions.
- Pass 2 now has a clear validation boundary before candidate writer/governance effects.
- The lease retry tests now prove both rollback cleanliness and the scheduled callback seam, which materially improves confidence in operational behavior.
