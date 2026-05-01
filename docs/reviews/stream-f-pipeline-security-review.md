# Stream F Review Gate C: Security/Privacy Review for Tasks 8-11

**Result: FAIL**

Severity-2 security/privacy findings remain. Current tests and clippy pass, but the implementation does not yet satisfy the Stream F v0.2 contract for subprocess timeout containment, prompt-bearing stderr/error surfaces, Pass 2 validation-before-write, and rehydration treatment of inactive memory refs.

## Scope reviewed

Contract: `docs/specs/stream-f-dreaming-v0.2.md`

Primary source files reviewed:

- `crates/memoryd/src/dream/{harness,registry,error,git,lease,run,pass1,pass2,pass3,evidence,masking,rehydration,mod}.rs`
- `crates/memoryd/src/{cli,main,handlers}.rs`
- `crates/memory-substrate/src/{api,error,model,git/commit}.rs`
- Stream F dream tests under `crates/memoryd/tests/` and `crates/memory-substrate/tests/`

## Commands run

```bash
git status --short
cargo test -p memoryd --test dream_harness_cli --test dream_pass_pipeline --test dream_grounding_rehydration --test dream_lease_election --test dream_lease_scheduled_retry
cargo test -p memory-substrate --test dream_canonical_isolation --test dream_substrate_primitives --test dream_merge_rules
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Results:

- Focused Stream F `memoryd` tests: PASS, 31 tests passed.
- Focused Stream F `memory-substrate` tests: PASS, 17 tests passed.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.

Note: the worktree was already dirty/untracked before this review. This review did not edit source; it added only this report file.

## Findings

### Severity 2 - Harness timeout does not cover stdin prompt write, so a non-reading child can hang indefinitely with prompt-bearing stdin open

**Evidence**

- `crates/memoryd/src/dream/harness.rs:130-134` moves the whole blocking subprocess run into `spawn_blocking` and awaits it directly; there is no outer `tokio::time::timeout` around the blocking task.
- `crates/memoryd/src/dream/harness.rs:416-421` spawns the child and synchronously writes the entire prompt to `child.stdin` before stdout/stderr reader threads are started and before timeout handling begins.
- `crates/memoryd/src/dream/harness.rs:428` enters `wait_with_timeout` only after the prompt write returns.
- `crates/memoryd/src/dream/harness.rs:451-473` sends SIGTERM/SIGKILL only from `wait_with_timeout`, so that kill path is unreachable while blocked in `stdin.write_all`.
- The contract requires per-pass timeout and fail-closed subprocess behavior in `docs/specs/stream-f-dreaming-v0.2.md:478-489`, including timeout termination and no prompt-bearing surfaces left behind.

**Exploitability**

A selected harness binary, wrapper, or broken stub can accept the process start but never read stdin. For prompts larger than the OS pipe buffer, `stdin.write_all(prompt.as_bytes())` can block forever. Because timeout enforcement starts only after that write completes, the daemon cannot terminate the child or close the prompt-bearing pipe.

**Impact**

- Dream run can hang indefinitely instead of failing closed.
- A prompt-bearing child process/pipe can remain live beyond the configured timeout.
- The daemon's blocking task can be consumed indefinitely, creating a reliability DoS and violating the privacy contract for prompt containment.

**Minimal remediation**

- Apply the timeout around the entire subprocess lifecycle, including stdin writing.
- Prefer async `tokio::process::Command` with concurrent stdin write, stdout/stderr drains, and timeout cancellation; or keep blocking execution but move stdin writing to a separately controlled thread and kill the child if the write exceeds the deadline.
- Start stdout/stderr readers before writing stdin.
- Explicitly close stdin after writing.
- Add a regression test with a child that never reads stdin and a prompt large enough to exceed the pipe buffer; assert timeout kills the process and the test completes boundedly.

---

### Severity 2 - Prompt fragments can leak through `stderr_tail` and error/report strings because redaction only removes an exact full-prompt substring

**Evidence**

- `crates/memoryd/src/dream/harness.rs:423-432` captures stderr and retains a tail for diagnostics.
- `crates/memoryd/src/dream/harness.rs:437-440` returns `HarnessCliError::SubprocessExit { stderr_tail }` to callers.
- `crates/memoryd/src/dream/harness.rs:557-562` redacts only `text.replace(prompt, "[prompt-redacted]")`, which catches the full prompt only when the child emits it as one exact contiguous string.
- `crates/memoryd/src/dream/error.rs:16-21` includes timeout, subprocess-exit stderr, and malformed JSON raw output in `Display` text.
- `crates/memoryd/src/dream/pass1.rs:28-31`, `crates/memoryd/src/dream/pass2.rs:99-101`, and `crates/memoryd/src/dream/run.rs:184-186` convert harness errors into user/report-facing strings.
- The contract requires prompt text to never appear in argv/stderr/logs in `docs/specs/stream-f-dreaming-v0.2.md:478-489` and requires stdin prompt transport in `docs/specs/stream-f-dreaming-v0.2.md:201-202`.

**Exploitability**

A harness can accidentally or maliciously echo only a prompt prefix, one prompt line, a transformed prompt, or a truncation to stderr. That output will not match the full prompt string and therefore will not be redacted. The tail then flows into error strings and can be printed by CLI callers or persisted in run reports.

**Impact**

Masked dream prompts are still private per the spec. Partial prompt disclosure through stderr/report surfaces violates the prompt privacy boundary and could expose masked-but-sensitive substrate, active-memory summaries, entity IDs, or prompt context.

**Minimal remediation**

- Do not surface raw stderr for dream pass subprocess failures. Prefer `stderr_hash`, exit code, and byte counts unless an operator explicitly requests a local diagnostic mode with disclosure.
- If stderr must be retained, redact conservatively: strip all prompt lines/fragments above a small length, known masked-token patterns, evidence catalog excerpts, and original private values tracked by the masking session.
- Add tests where the child echoes partial prompt lines and transformed prompt fragments to stderr and assert no prompt substring reaches `HarnessCliError`, `PassOutcome.error_code`, CLI stderr, or logs.

---

### Severity 2 - Pass 2 validation is incomplete; unsupported or cross-scope candidates can reach the candidate writer before required schema/evidence checks

**Evidence**

- Contract requires Pass 2 validation to enforce namespace-in-scope, known kind, confidence in `[0, 1]`, non-empty evidence, every evidence ref in the catalog, and candidate count cap in `docs/specs/stream-f-dreaming-v0.2.md:660-669` and `docs/specs/stream-f-dreaming-v0.2.md:672-679`.
- `crates/memoryd/src/dream/pass2.rs:46-58` constructs an `EvidenceCatalog` and rejects only invalid `(kind, ref)` tuples. It does not reject an empty `evidence` array because `first_invalid_ref(&[])` returns `None`.
- `crates/memoryd/src/dream/pass2.rs:60-70` forwards `proposal.namespace`, `proposal.kind`, `proposal.confidence`, and `proposal.evidence` directly into `CandidateWriteRequest` without validating scope, kind, confidence, evidence non-emptiness, or max candidate count.
- `crates/memoryd/src/dream/pass2.rs:73-79` reports Pass 2 success regardless of whether every candidate was refused or whether zero candidates were accepted, weakening operator signals.
- `crates/memoryd/src/dream/rehydration.rs:59-64` treats an empty citation set as success because it simply iterates `grounding_citations(candidate)` and returns `Ok(())` if there are none.
- `crates/memoryd/src/handlers.rs:1012-1021` approves a dream candidate when `verify_dream_candidate` returns `Ok(())`.

**Exploitability**

A malformed or adversarial harness response can return a Pass 2 candidate with `evidence: []`, a namespace outside the leased scope, an unknown kind, non-finite/out-of-range confidence, or too many candidates. The current pipeline forwards those candidates to the writer instead of rejecting them at the deterministic validation boundary. If a zero-evidence dream candidate is written, rehydration later has no citations to verify and can approve it.

**Impact**

- Hallucinated/unsupported dream candidates can bypass the evidence validation boundary before candidate write.
- Cross-scope candidate writes can bypass the lease scope boundary once a real writer maps `namespace` to canonical write scope.
- A zero-citation dream candidate can pass the approval-time rehydration hook because no refs are checked.

**Minimal remediation**

- Add a `validate_candidate` function before restoration/write that enforces the full spec: candidate count cap, namespace equals the leased scope/in-scope namespace set, kind is canonical, confidence is finite and within `[0, 1]`, evidence is non-empty, and every `(kind, ref)` exists in the prompt evidence catalog.
- Reject invalid candidates with stable refusal reason codes before calling `CandidateWriter`.
- Make approval-time rehydration fail closed for dream candidates with `grounding_rehydration_required: true` and zero source/evidence citations.
- Add regression tests for empty evidence, cross-scope namespace, unknown kind, NaN/negative/>1 confidence, and over-cap candidate arrays.

---

### Severity 2 - Rehydration treats candidate/quarantined memory refs as valid grounding even though only active/pinned memory refs should ground approval

**Evidence**

- `crates/memory-substrate/src/model.rs:120-134` defines `MemoryStatus::{Candidate, Active, Pinned, Superseded, Archived, Tombstoned, Quarantined}`.
- `crates/memoryd/src/dream/rehydration.rs:122-127` rejects only `Tombstoned | Superseded | Archived` refs. It permits `Candidate` and `Quarantined` cited memories.
- `crates/memoryd/src/handlers.rs:1012-1021` uses `verify_dream_candidate` as the approval gate before `ReviewDecision::Approve`.
- `crates/memoryd/src/handlers.rs:1870-1874` approval sets the dream candidate to `status = Active` and `trust_level = Trusted`.
- Existing rehydration tests cover tombstoned/superseded/archived in `crates/memoryd/tests/dream_grounding_rehydration.rs:65-84`, but they do not include candidate or quarantined cited refs.

**Exploitability**

A dream candidate can cite another memory that is itself unreviewed (`candidate`) or quarantined. If the cited body has not drifted, `verify_memory_ref` returns success and the reviewing approval can promote the dream candidate as trusted.

**Impact**

The approval hook can promote a dream-authored candidate grounded on untrusted or quarantined material. That violates the "active memory set" grounding model and the requested inactive-ref boundary.

**Minimal remediation**

- Change `verify_memory_ref` to accept only `MemoryStatus::Active | MemoryStatus::Pinned` as valid grounding statuses.
- Treat every other status, including `Candidate` and `Quarantined`, as `GroundingRehydrationError::Inactive`.
- Add regression tests for cited `Candidate` and `Quarantined` memories.

---

### Severity 3 - Lease push-race retry leaves local lease commits/records behind after failed pushes, which fails safe for the current run but can wedge later lease attempts

**Evidence**

- `crates/memoryd/src/dream/lease.rs:133-154` fetches, appends a lease record, commits it, attempts push, and on push failure loops to retry.
- `crates/memoryd/src/dream/lease.rs:146-154` does not roll back the appended lease record or the local lease commit before retrying.
- `crates/memoryd/src/dream/lease.rs:134` re-fetches on each attempt, but there is no merge/rebase/reset before the next append/commit/push attempt.
- `crates/memoryd/src/dream/git.rs:97-100` dirty-tree detection intentionally allows `leases/journal.lease`, so local failed lease changes do not block the retry as user dirt.

**Exploitability**

A non-fast-forward push race or persistent push failure after the lease commit leaves local lease state ahead of the remote. The current run eventually aborts, but local unpushed lease commits can accumulate and continue to make future pushes fail until the operator manually reconciles the repo.

**Impact**

This appears fail-safe for the current dream run because no run proceeds without successful push. However, it can wedge the device's future lease acquisition and produce confusing local lease history. It is an operational safety issue rather than an immediate privacy exposure.

**Minimal remediation**

- Use a lease-specific temporary worktree/index for acquisition, or roll back the local lease commit and lease-file append after push failure before retrying.
- After a push rejection, fetch and explicitly rebase/merge/reset according to the repo's safe synchronization contract before appending a new lease record.
- Add a real-git test with a remote non-fast-forward race that asserts no failed lease commits remain locally after the function returns `lease_unavailable`.

## Positive confirmations

- **Built-in v0.2 adapters do not put prompts in argv.** Claude uses `claude --print` and stdin transport in `crates/memoryd/src/dream/harness.rs:200-205`; Codex uses `codex exec -` / `codex exec --json -` and stdin transport in `crates/memoryd/src/dream/harness.rs:275-282`; the v0.2 registry enables only Claude/Codex and leaves Gemini disabled in `crates/memoryd/src/dream/registry.rs:13-28`; the argv regression test is `crates/memoryd/tests/dream_harness_cli.rs:32-47`.
- **Harness env/cwd isolation is mostly in place.** The documented env allowlist is `crates/memoryd/src/dream/harness.rs:19-29`; `env_clear` plus allowlisted env application is `crates/memoryd/src/dream/harness.rs:103-107`; the child cwd is a tempdir under the scratch root in `crates/memoryd/src/dream/harness.rs:395-407`; tests cover this in `crates/memoryd/tests/dream_harness_cli.rs:49-113`.
- **Pass 1 and Pass 3 outputs remain masked in normal pipeline tests.** Pass 1 writes harness output directly without restore in `crates/memoryd/src/dream/pass1.rs:40-52`; Pass 3 validates and writes JSONL without restore in `crates/memoryd/src/dream/pass3.rs:35-59`; tests assert no `Alice` leak in `crates/memoryd/tests/dream_pass_pipeline.rs:12-28` and `crates/memoryd/tests/dream_pass_pipeline.rs:269-304`.
- **Pass 2 restore is currently contained to candidate fields.** Restore calls happen only for `claim` and `rationale` before candidate write in `crates/memoryd/src/dream/pass2.rs:60-69`. The current candidate evidence struct carries only `kind/ref`, so no excerpt restoration occurs in this implementation.
- **Dream prose refs are refused before canonical write.** The substrate write path calls `enforce_no_dream_prose_sources` before frontmatter validation and disk write in `crates/memory-substrate/src/api.rs:263-269`; the detector covers source and evidence refs in `crates/memory-substrate/src/api.rs:1418-1438`; tests cover journal/question source refusals in `crates/memoryd/tests/dream_grounding_rehydration.rs:124-147`.
- **Lease commits avoid staging unrelated dirty/prestaged work in the covered path.** Dirty-user-work detection rejects any status path other than `leases/journal.lease` in `crates/memoryd/src/dream/git.rs:97-100`; lease commit stages only `leases/journal.lease` in `crates/memory-substrate/src/git/commit.rs:95-104` and commits with a pathspec and fixed identity in `crates/memory-substrate/src/git/commit.rs:133-154`; tests cover unrelated dirty work in `crates/memoryd/tests/dream_lease_election.rs:155-174`.
- **Rehydration does not affect non-dream candidates.** `requires_rehydration` is gated on `AuthorKind::Dreaming` and `grounding_rehydration_required` in `crates/memoryd/src/dream/rehydration.rs:67-69`; the non-dream regression is `crates/memoryd/tests/dream_grounding_rehydration.rs:104-122`.

## Residual risk

- The actual production `CandidateWriter` is not wired in the reviewed code; most Pass 2 candidate-write coverage uses a recording test writer or `NoopCandidateWriter`. The validation defects above therefore need to be fixed before wiring a real writer.
- `memoryd dream now` in the CLI currently acquires a lease and returns a skipped stub report rather than running the full dream pipeline (`crates/memoryd/src/main.rs:183-208`), while daemon `RequestPayload::DreamNow` / `DreamStatus` return `not_implemented` (`crates/memoryd/src/handlers.rs:119-124`). That limits end-to-end security coverage for the final operational surface.
- `DreamMaskingSession` derives `Debug` and stores `original_private_values` in memory (`crates/memoryd/src/dream/masking.rs:21-28`). I did not find current logging of the session, but future debug logging could expose sensitive values; consider removing `Debug` or redacting the field.
- The focused tests and clippy passing should not be read as a security pass. The failing issues are validation/containment gaps not currently covered by tests.

## Confidence

High for the listed findings. They are based on direct source evidence and the active Stream F v0.2 contract. The main uncertainty is production exploitability of the Pass 2 write path because a real `CandidateWriter` is not yet wired; the validation boundary should still be fixed before that wiring lands.
