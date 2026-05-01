# Stream F Final Gate E Clean-Code Review

## Verdict

Changes requested.

The Stream F implementation has good modular coverage in the isolated dream pipeline and many targeted behavior tests pass, but the public `memoryd dream now` / daemon `DreamNow` entrypoints still wire the production path through fixture-like inputs and no-op candidate persistence. There are also avoidable error-handling and trust-boundary simplifications in grounding rehydration that make the final implementation harder to reason about and easier to misoperate.

## Findings

### S1 - Public dream entrypoints run a fixture-shaped pipeline, not the real Stream F dream

- **Evidence:** `crates/memoryd/src/main.rs:337-355` builds `DreamRunOptions` with `substrate_fragments: Vec::new()`, `active_memories: Vec::new()`, `previous_questions: Vec::new()`, then runs `DreamRunner` with `NoopCandidateWriter`. The daemon path does the same in `crates/memoryd/src/handlers.rs:165-180`. The only end-to-end CLI test asserts the echo fixture path succeeds and writes a journal (`crates/memoryd/tests/dream_cli.rs:51-72`), but does not require substrate fragments, active memories, prior questions, or candidate queue writes. This contradicts the v0.2 contract that Pass 1 inputs include in-scope substrate fragments, active memories, and recent governance decisions (`docs/specs/stream-f-dreaming-v0.2.md:595-601`) and that Pass 2 writes surviving candidates to the canonical candidate-write path (`docs/specs/stream-f-dreaming-v0.2.md:660-670`).
- **Why it matters:** The feature can appear green while producing low-value empty dreams and silently discarding every valid Pass 2 candidate. That is a business-outcome failure: Stream F is supposed to turn observations into durable journal/question/candidate outputs, not only prove the pass runner works against canned empty inputs.
- **Reasoning:** The isolated `dream_pass_pipeline` tests prove `DreamRunner` can write candidates when given a real writer, but the public surfaces never provide that writer. A real Claude/Codex run can return valid candidates, `run_pass_2` will call `NoopCandidateWriter`, and the report will show refused/noop results rather than creating reviewable canonical candidates. Similarly, the prompt builder never receives the real substrate or active-memory data required for meaningful Pass 1/2/3 output.
- **Required fix:** Add a production dream orchestration seam used by both CLI and daemon entrypoints that: loads in-scope plaintext/encrypted-descriptor substrate fragments for the configured window, loads the active/pinned memory summary set and previous questions, builds `DreamRunOptions` from those real inputs, and supplies a candidate writer backed by the existing governance/canonical write path. Keep `EchoCli` as a test harness, but do not let test-fixture inputs or `NoopCandidateWriter` be the production default.

### S2 - Daemon `DreamNow` exposes a public protocol variant but only supports the test echo harness

- **Evidence:** `crates/memoryd/src/handlers.rs:186-200` accepts only `cli_override=echo`; `crates/memoryd/src/handlers.rs:203-211` returns `dream_unavailable` for known real adapters such as Claude/Codex. The spec defines `RequestPayload::DreamNow` as the daemon protocol surface (`docs/specs/stream-f-dreaming-v0.2.md:236-250`) and says harness selection should use installed/authenticated adapters from the configured priority list (`docs/specs/stream-f-dreaming-v0.2.md:455-477`).
- **Why it matters:** This leaves two divergent public paths: the CLI can select real harness adapters, while daemon `DreamNow` cannot. That is avoidable API complexity and a likely source of production confusion for any client that uses the protocol instead of spawning the CLI binary.
- **Reasoning:** The handler has bespoke validation/selection logic rather than sharing the CLI harness-selection seam. The error message explicitly says the real path is unavailable "until harness runtime context is available," which means the merged API would advertise a surface that is intentionally incomplete. Because `DreamStatus` reports real adapters, a client can see Claude/Codex available and still fail when invoking `DreamNow` through the daemon.
- **Required fix:** Extract harness selection into a shared library function with the same behavior for CLI and daemon protocol, including configured per-scope priority and real adapter support. If daemon `DreamNow` is intentionally test-only for v0.2, remove it from the public protocol or gate it behind an explicit test-only feature; do not ship a normal-looking protocol variant that rejects all real harnesses.

### S2 - Grounding rehydration silently falls back to default config on malformed config

- **Evidence:** `crates/memoryd/src/dream/rehydration.rs:158-165` calls `load_config(...).map(...).unwrap_or_default()`, so a malformed or unreadable dreams config silently becomes default `pass_2_drift_threshold` / `fragment_lifetime_days`. The spec requires stable typed errors and says users should not parse prose to detect failure (`docs/specs/stream-f-dreaming-v0.2.md:746-757`).
- **Why it matters:** Candidate approval can be decided with the wrong drift threshold or lifetime. A stricter local policy can be bypassed accidentally by a config parse/load error; a looser local policy can unexpectedly quarantine valid candidates. Either way, review outcomes become surprising and hard to diagnose.
- **Reasoning:** Rehydration is the trust boundary for dream-authored candidate promotion. Error handling here should fail closed and report the actual configuration problem. Hiding the load failure behind defaults is a classic clean-code smell: the callee's simple signature makes the caller code short but loses an important operational invariant.
- **Required fix:** Make `rehydration_config` return `Result<RehydrationConfig, GroundingRehydrationError>` and propagate load/validation errors as a typed inspect/config error. Add a test with malformed `dreams.pass_2_drift_threshold` or invalid fragment lifetime proving approval does not silently use defaults.

### S2 - File grounding refs can escape the repository root

- **Evidence:** `crates/memoryd/src/dream/rehydration.rs:309-316` accepts absolute `file:` references by returning `path.to_path_buf()` when `path.is_absolute()`. `verify_file_ref` then reads that path directly at `crates/memoryd/src/dream/rehydration.rs:140-154`. The spec describes rehydration as resolving cited refs against the live substrate (`docs/specs/stream-f-dreaming-v0.2.md:129-138`) and Pass 2 evidence refs as entries from the prompt evidence catalog (`docs/specs/stream-f-dreaming-v0.2.md:620-626`).
- **Why it matters:** A dream-authored candidate can make candidate approval depend on ambient local filesystem state outside the memory repo. That broadens the API contract, makes tests less deterministic, and risks leaking local-file existence/content facts into governance decisions.
- **Reasoning:** Grounding should be a narrow repo/substrate contract. Accepting absolute paths creates hidden coupling to whichever machine approves the candidate. It is also inconsistent with the evidence-catalog design: candidates should cite `sub_*`, `mem_*`, or repo-relative refs that were explicitly made available, not arbitrary host paths.
- **Required fix:** Reject absolute file refs and path traversal before any filesystem access. Resolve only normalized repo-relative references under `substrate.roots().repo`, and add tests for `file:/tmp/...`, `/tmp/...`, and `../outside` references.

### S3 - Echo harness fixture construction is duplicated across production entrypoints

- **Evidence:** `crates/memoryd/src/dream/run.rs:45-55` defines `deterministic_echo_harness`, while `crates/memoryd/src/main.rs:386-410` repeats the same prompt-preview/canned-output construction. The daemon path uses the shared helper (`crates/memoryd/src/handlers.rs:193-200`), but the CLI path does not.
- **Why it matters:** Fixture setup is now a small but real maintenance fork. If prompt preview semantics or canned echo output change, tests can keep one path green while another drifts.
- **Reasoning:** This is avoidable duplication in a test-support seam. It also contributes to the larger confusion between test harness behavior and production orchestration.
- **Required fix:** Delete the duplicate `echo_harness` in `main.rs` and call `memoryd::dream::run::deterministic_echo_harness` from the CLI path as well.

### S3 - Pass 3 validation collapses distinct omission reasons into `malformed_record`

- **Evidence:** `crates/memoryd/src/dream/pass3.rs:75-88` increments `malformed_record` for JSON parse errors, empty questions, empty entities, hallucinated entities, and unmasked/private-value leakage. The spec separates malformed records from cap/no-entity/unsafe-fragment omission reasons (`docs/specs/stream-f-dreaming-v0.2.md:742-744`) and explicitly calls for validation of entity size and question length (`docs/specs/stream-f-dreaming-v0.2.md:700-705`).
- **Why it matters:** Operators lose useful diagnostics. A spike in hallucinated entities, oversized questions, or unsafe outputs looks identical to broken JSON, so follow-up tuning/debugging is harder than necessary.
- **Reasoning:** The implementation is compact, but the single `if` condition hides several materially different cases. This is a maintainability smell in observability/error classification, not just style.
- **Required fix:** Split Pass 3 validation into named checks that return a small `QuestionRejectReason` enum. Map each enum to the protocol counter key, and add tests for at least malformed JSON, hallucinated entity, empty entity/no entity match, oversized question truncation, and private-value rejection.

## Required fixes

1. Replace public `memoryd dream now` / daemon `DreamNow` fixture wiring with a real orchestration seam that loads Stream F inputs and writes Pass 2 candidates through governance/canonical write.
2. Unify CLI and daemon harness selection, or remove/gate daemon `DreamNow` if real harness support is intentionally not shipping.
3. Make grounding rehydration config loading fail closed with typed errors instead of defaulting silently.
4. Constrain file grounding refs to normalized repo-relative paths under the substrate repo.
5. Remove duplicated echo harness setup in `main.rs`.
6. Split Pass 3 omission/error reasons so status counters remain actionable.

## Residual risks

- I did not run the full workspace gate or clippy because the requested review asked to avoid expensive full gates unless necessary. The targeted dream tests I ran pass, but they do not cover the production orchestration gaps above.
- The workspace diff is very large and includes many untracked review/docs/test artifacts. I focused on Stream F's final clean-code seams: public entrypoints, dream modules, grounding, CLI/daemon wiring, and tests around those areas.
- Security/performance/API-contract reviewers should still verify their own lanes; this report includes security-adjacent findings only where they directly overlap clean-code/API-boundary simplicity.

## Commands run

```bash
sed -n '1,220p' /Users/treygoff/.agents/skill-library/clean-code/SKILL.md
sed -n '1,220p' /Users/treygoff/.agents/skill-library/tdd/SKILL.md
sed -n '1,220p' /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md
grep -n "Stream F\|dreaming\|agent-memory" /Users/treygoff/.ai-profiles/runtime/codex/personal/memories/MEMORY.md | head -40
git status --short && git diff --stat && git diff --name-only
sed -n '1,260p' docs/plans/2026-04-30-stream-f-dreaming.md
sed -n '1,260p' docs/specs/stream-f-dreaming-v0.2.md
find crates/memoryd/src/dream -type f -maxdepth 3 -print | sort | xargs -I{} sh -c 'echo --- {}; sed -n "1,220p" {}'
git diff -- crates/memoryd/src/handlers.rs crates/memoryd/src/protocol.rs crates/memoryd/src/main.rs crates/memoryd/src/cli.rs | sed -n '1,260p'
git diff -- crates/memory-substrate/src/api.rs crates/memory-substrate/src/config/mod.rs crates/memory-substrate/src/model.rs | sed -n '1,300p'
find crates/memoryd/src/dream -type f -maxdepth 1 -print -exec wc -l {} \; | sort
rg -n "unwrap\(|expect\(|TODO|FIXME|placeholder|NoopCandidateWriter|EchoCli|dream_unavailable|unsupported|to_string\(\)\)\?|map_err\(\|err\| .*to_string\(\)\)" crates/memoryd/src crates/memory-substrate/src crates/memoryd/tests crates/memory-substrate/tests | head -200
rg -n "pub async fn|fn |impl |struct |enum |trait " crates/memoryd/src/dream crates/memoryd/src/handlers.rs crates/memory-substrate/src/api.rs | sed -n '1,260p'
sed -n '120,260p' crates/memoryd/src/handlers.rs && sed -n '1,260p' crates/memoryd/src/main.rs && sed -n '1,240p' crates/memoryd/src/dream/registry.rs
sed -n '1,220p' crates/memoryd/src/dream/run.rs && sed -n '1,260p' crates/memoryd/src/dream/pass2.rs && sed -n '1,180p' crates/memoryd/src/dream/pass3.rs
sed -n '1,360p' crates/memoryd/src/dream/rehydration.rs && sed -n '1,240p' crates/memoryd/src/dream/harness.rs
sed -n '260,560p' crates/memoryd/src/main.rs && sed -n '240,520p' crates/memoryd/src/dream/harness.rs && sed -n '1,260p' crates/memoryd/src/dream/lease.rs
cargo test -p memoryd --test dream_cli --test dream_pass_pipeline --test dream_grounding_rehydration
nl -ba crates/memoryd/src/main.rs | sed -n '320,430p'; nl -ba crates/memoryd/src/handlers.rs | sed -n '140,220p'; nl -ba crates/memoryd/src/dream/run.rs | sed -n '35,95p'
nl -ba crates/memoryd/src/dream/rehydration.rs | sed -n '150,330p'; nl -ba crates/memoryd/src/dream/pass2.rs | sed -n '1,180p'; nl -ba crates/memoryd/src/dream/pass3.rs | sed -n '35,115p'
nl -ba docs/specs/stream-f-dreaming-v0.2.md | sed -n '1,150p'; nl -ba docs/specs/stream-f-dreaming-v0.2.md | sed -n '430,560p'; nl -ba docs/specs/stream-f-dreaming-v0.2.md | sed -n '650,760p'
nl -ba docs/plans/2026-04-30-stream-f-dreaming.md | sed -n '1,120p'; nl -ba docs/plans/2026-04-30-stream-f-dreaming.md | sed -n '520,720p'
rg -n "run_manual_dream|DreamNow|NoopCandidateWriter|substrate_fragments: Vec::new|active_memories: Vec::new|previous_questions: Vec::new|echo_harness|deterministic_echo_harness|grounding_rehydration|load_config.*unwrap_or_default|file_reference_path|path.is_absolute" crates/memoryd/src crates/memoryd/tests docs/specs/stream-f-dreaming-v0.2.md docs/plans/2026-04-30-stream-f-dreaming.md
nl -ba docs/specs/stream-f-dreaming-v0.2.md | sed -n '558,640p'; nl -ba docs/plans/2026-04-30-stream-f-dreaming.md | sed -n '780,840p'; nl -ba crates/memoryd/tests/dream_cli.rs | sed -n '50,85p'; nl -ba crates/memoryd/tests/handler_contract.rs | sed -n '108,150p'
```
