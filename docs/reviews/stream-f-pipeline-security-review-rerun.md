# Stream F Review Gate C: Security/Privacy Rerun for Tasks 8-11

**Result: PASS**

No severity-1 or severity-2 security/privacy findings remain in the reviewed Stream F Tasks 8-11 surfaces. The prior Gate C findings in `docs/reviews/stream-f-pipeline-security-review.md` are closed by current code and regression coverage.

## Scope reviewed

Prior report: `docs/reviews/stream-f-pipeline-security-review.md`

Contract: `docs/specs/stream-f-dreaming-v0.2.md`

Primary source paths reviewed:

- `crates/memoryd/src/dream/harness.rs`
- `crates/memoryd/src/dream/error.rs`
- `crates/memoryd/src/dream/pass1.rs`
- `crates/memoryd/src/dream/pass2.rs`
- `crates/memoryd/src/dream/pass3.rs`
- `crates/memoryd/src/dream/rehydration.rs`
- `crates/memoryd/src/dream/lease.rs`
- `crates/memoryd/src/dream/git.rs`
- `crates/memoryd/src/handlers.rs`
- Stream F regression tests under `crates/memoryd/tests/` and `crates/memory-substrate/tests/`

Note: the worktree was already dirty/untracked before this rerun. I did not edit source; this report is the only intended output from this rerun.

## Commands run

```bash
git status --short --branch
cargo test -p memoryd --test dream_harness_cli --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_lease_election --test dream_lease_scheduled_retry
cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives --test dream_merge_rules
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git status --short --branch
```

Results:

- Focused Stream F `memoryd` tests: PASS, 46 tests passed.
- Focused Stream F `memory-substrate` tests: PASS, 17 tests passed.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.

## Severity findings

None.

## Prior finding closures

### Closure: timeout now covers blocking stdin writes and a non-reading child

**Status: closed.**

Evidence:

- `crates/memoryd/src/dream/harness.rs:416-425` starts stdout/stderr reader threads and moves stdin writing to a separate thread before waiting on the child.
- `crates/memoryd/src/dream/harness.rs:427-441` waits with timeout, joins the stdin writer after the child exits or is killed, and returns timeout before surfacing ordinary subprocess status.
- `crates/memoryd/src/dream/harness.rs:452-478` enforces timeout with SIGTERM, grace wait, SIGKILL, and final `wait`.
- `crates/memoryd/src/dream/harness.rs:543-555` performs the blocking stdin write in a joinable writer thread, so the main wait loop is not blocked by a full stdin pipe.
- `crates/memoryd/tests/dream_harness_cli.rs:167-221` regresses a non-reading child with a large prompt, wraps the call in a bounded test timeout, asserts `HarnessCliError::Timeout`, asserts elapsed time remains bounded, and verifies the child is not alive.

Security/privacy assessment: the prior DoS/privacy containment issue for a non-reading child is closed for the reviewed process path.

### Closure: prompt fragments no longer leak through stderr/error surfaces

**Status: closed.**

Evidence:

- `crates/memoryd/src/dream/harness.rs:430-439` converts captured stderr into a redacted diagnostic before storing it in `HarnessCliError::SubprocessExit`.
- `crates/memoryd/src/dream/harness.rs:561-569` redacts malformed JSON raw stdout diagnostics instead of returning raw model output.
- `crates/memoryd/src/dream/harness.rs:572-577` formats diagnostics only as byte count plus short hash.
- `crates/memoryd/src/dream/error.rs:16-21` displays timeout, subprocess-exit, and malformed-JSON errors without raw stderr/stdout text.
- `crates/memoryd/src/dream/pass1.rs:28-31`, `crates/memoryd/src/dream/pass2.rs:95-97`, and `crates/memoryd/src/dream/pass3.rs:39-42` propagate harness failures through the redacted error display surface.
- `crates/memoryd/tests/dream_harness_cli.rs:223-272` regresses partial prompt-line echo to stderr and asserts none of the prompt fragments appear in either `error.to_string()` or the stored `stderr_tail` field.

Security/privacy assessment: the prior partial-prompt stderr/error leak is closed for the reviewed harness error paths.

### Closure: Pass 2 validation completes before candidate write

**Status: closed.**

Evidence:

- `crates/memoryd/src/dream/pass2.rs:50-53` refuses all candidates before writer invocation when the candidate array exceeds the configured cap.
- `crates/memoryd/src/dream/pass2.rs:55-72` validates each proposal before constructing `CandidateWriteRequest` and before calling `writer.write_candidate`.
- `crates/memoryd/src/dream/pass2.rs:108-130` enforces namespace match, supported kind, finite confidence in `[0, 1]`, non-empty evidence, and evidence catalog membership.
- `crates/memoryd/src/dream/pass2.rs:161-168` marks zero-accepted Pass 2 output as `Skipped` with stable `no_candidates_accepted` instead of success.
- `crates/memoryd/tests/dream_pass_pipeline.rs:159-183` verifies empty evidence is refused before the recording writer sees any write.
- `crates/memoryd/tests/dream_pass_pipeline.rs:185-256` verifies out-of-scope namespace, invalid confidence, and invalid kind are refused before writer/governance.
- `crates/memoryd/tests/dream_pass_pipeline.rs:258-294` verifies over-cap candidate arrays are refused before writer/governance.
- `crates/memoryd/tests/dream_pass_pipeline.rs:349-388` verifies hallucinated evidence refs are refused before the writer/governance boundary.

Security/privacy assessment: unsupported, cross-scope, zero-evidence, out-of-range-confidence, and over-cap Pass 2 candidates no longer reach the candidate writer in the reviewed pipeline.

### Closure: rehydration rejects inactive memory refs and covers file refs

**Status: closed.**

Evidence:

- `crates/memoryd/src/dream/rehydration.rs:51-65` gates dream-candidate approval through rehydration when the Stream F marker requires it.
- `crates/memoryd/src/dream/rehydration.rs:76-87` dispatches substrate refs, memory refs, and file refs through separate deterministic checks and rejects dream-prose refs before resolution.
- `crates/memoryd/src/dream/rehydration.rs:107-130` reads cited memory refs and rejects unacceptable lifecycle/trust states before drift checks.
- `crates/memoryd/src/dream/rehydration.rs:133-136` allows only `Active` or `Pinned` memory statuses with `Trusted` or `Pinned` trust levels as grounding.
- `crates/memoryd/src/dream/rehydration.rs:138-155` verifies file refs exist and, when quoted, checks content drift.
- `crates/memoryd/src/dream/rehydration.rs:309-317` resolves `file:` and repo-relative file references.
- `crates/memoryd/src/handlers.rs:1012-1020` quarantines a dream candidate instead of approving it when rehydration fails.
- `crates/memoryd/tests/dream_grounding_rehydration.rs:65-122` verifies missing repo-relative file refs, missing `file:` refs, drifted file refs, and valid file refs.
- `crates/memoryd/tests/dream_grounding_rehydration.rs:124-145` verifies cited `Candidate`, `Quarantined`, `Tombstoned`, `Superseded`, and `Archived` memories quarantine on approval.

Security/privacy assessment: candidate/quarantined/inactive memory refs no longer ground promotion, and file refs are covered by missing/drift/valid regression cases.

### Closure: lease push-race rollback removes failed local lease commits/records

**Status: closed.**

Evidence:

- `crates/memoryd/src/dream/lease.rs:152-160` rolls back a failed lease attempt immediately after push failure and before retry/return.
- `crates/memoryd/src/dream/git.rs:12-18` makes rollback part of the `LeaseGit` boundary, so tests and native git both exercise the explicit rollback contract.
- `crates/memoryd/src/dream/git.rs:44-48` implements native rollback with `git reset --hard HEAD~1` for the failed lease commit.
- `crates/memoryd/src/dream/git.rs:103-110` makes the scripted test implementation remove the appended lease record on rollback.
- `crates/memoryd/tests/dream_lease_election.rs:120-141` uses a rejecting origin hook and asserts persistent push rejection leaves the original commit count unchanged, zero local lease records, and a clean worktree.
- `crates/memoryd/tests/dream_lease_election.rs:143-185` verifies push-race retry count and fetch-between-attempt behavior.

Security/privacy assessment: the prior operational safety issue is closed; failed local lease attempts no longer leave stale local lease records/commits behind in the covered native-git regression.

## Positive confirmations

- Built-in v0.2 adapters still use stdin transport, not argv: `crates/memoryd/tests/dream_harness_cli.rs:34-49` and `crates/memoryd/tests/dream_harness_cli.rs:274-280`.
- Harness env/cwd isolation remains covered: `crates/memoryd/src/dream/harness.rs:19-29`, `crates/memoryd/src/dream/harness.rs:103-107`, `crates/memoryd/src/dream/harness.rs:395-414`, and `crates/memoryd/tests/dream_harness_cli.rs:51-118`.
- Pass 1 and Pass 3 remain masked-only output paths: `crates/memoryd/tests/dream_pass_pipeline.rs:12-28`, `crates/memoryd/tests/dream_pass_pipeline.rs:486-550`.
- Dream prose refs are still refused before disk effects: `crates/memoryd/src/dream/rehydration.rs:76-79`, `crates/memoryd/src/dream/rehydration.rs:319-326`, and `crates/memoryd/tests/dream_grounding_rehydration.rs:182-215`.

## Residual risk

- The reviewed Pass 2 writer path is still exercised through a test `RecordingCandidateWriter`/`NoopCandidateWriter`; a future production `CandidateWriter` should preserve these pre-write validation invariants and keep `grounding_rehydration_required: true` on accepted dream candidates.
- The harness implementation enforces timeout inside the blocking subprocess runner rather than by wrapping `spawn_blocking` itself in `tokio::time::timeout`. The prior non-reading-child issue is covered and passing, but future hardening could add an outer async timeout/process-group cleanup for forked grandchildren or inherited pipe descriptors.
- The worktree had substantial pre-existing modified/untracked files before this rerun, so this report certifies the current dirty-tree snapshot rather than a committed baseline.

## Confidence

High for the focused prior-finding closures and for the statement that no severity-1/2 security/privacy findings remain in Tasks 8-11. The conclusion is based on direct source inspection plus the focused Stream F tests, fmt, and clippy passing in this rerun.
