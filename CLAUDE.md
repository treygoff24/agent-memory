# agent-memory

Implementation home for the agent-memory system (ships as **"Memorum"**, captured in `docs/specs/system-v0.2.md` §22).

**Stream model:**

- **A** Core substrate. Canonical Markdown+YAML files, derived SQLite/FTS/vector indexes, per-device JSONL events, git as sync transport.
- **B** Daemon (`memoryd`), MCP bridge, process lifecycle, embedding inference worker.
- **C** Governance: promotion, contradiction detection, grounding, tombstone matching.
- **D** Privacy filter: classification, age encryption, masked synthesis. Supplies `ClassificationOutcome` to A.
- **E** Recall block assembly, harness hooks (startup + delta).
- **F** Dreaming. Delegates LLM calls to whichever harness CLI is installed (`claude -p`, `codex exec`).
- **G** Observability: TUI, localhost web dashboard, Reality Check, notifications, trust artifact rendering.
- **H** Eval harness.
- **I** Cross-session coordination: peer updates, presence, claim locks, peer admin surfaces.

## Current status (2026-05-11)

**Streams A–I are shipped.** Stream H's live real-harness flows still require authenticated Claude/Codex CLIs before claiming live LLM success on auth-gated paths. Dogfood-readiness gap-fix is closed out (head `2a9a9ad` on `dogfood/codex-readiness-2026-05-07`, 26 commits ahead of `main`); full release gate green. Long-form closeout history and 5/7-8 + 5/11 lessons live in `docs/handoffs/2026-05-codex-overnight-lessons.md`.

Branding decision: **"Memorum"** (Latin genitive plural of `memor`, "mindful").

## Gate policy

Use tiered local gates — do not default to the full release gate after every small task.

- **Fast inner loop:** `pnpm run check:fast` at repo root. For dashboard-only work, also/instead from `crates/memoryd-web/frontend`. Use targeted `cargo test -p ...`, `pnpm run test:gentle`, or `pnpm run test:e2e:gentle` while iterating.
- **Local confidence:** `pnpm run check:local` before claiming a task, plan step, or milestone complete. Runs the fast gate, full Rust clippy/tests/docs, convergence smoke, and the dashboard local gate while capping local parallelism.
- **Full validation:** `pnpm run check:full` / `bash scripts/check.sh` only for final verification, pre-merge/release, CI-equivalent confidence, or changes that directly require the full stack. Includes the dashboard full gate plus release Rust tests/benches/durability. Use `CARGO_BUILD_JOBS=4 bash scripts/check.sh` on macOS to keep the machine usable under heavy compile.

If a gate fails, fix the issue and rerun the **narrow failing gate** first. Report exactly which gates ran and which expensive gates were skipped, with reasons. Multiple agents may be active in different worktrees — avoid cooking the machine with repeated full gates.

## Who's doing what

- **Codex** owned Stream A and implemented Streams G, H, and I. The worktree-per-task / per-task-gate / orchestrator-merged-lockfile workflow is its idiom.
- **Claude (you)** owns Stream B (shipped 2026-04-28). For H/I implementation, Claude is reviewer-only unless Trey explicitly redirects. Otherwise Claude is architect/reviewer here: spec authorship, plan critique, plan-reviewer passes, sanity checks, and ad-hoc work Trey hands you. **Do not modify Stream A modules** unless Trey explicitly redirects (he did once, for the FTS5 sanitization fix in `946d75f`); the substrate is otherwise a frozen contract for downstream streams.
- **Trey** drives. He'll tell you what's next.

## Working with Codex overnight (operational rules)

Distilled from the 2026-05-07/08 ~17.5h dogfood-readiness run and the 5/11 closeout. Full narrative in `docs/handoffs/2026-05-codex-overnight-lessons.md` — read it before writing any new overnight-run plan.

- **Operational structure must be hard contract, not procedural advice.** Codex is extremely literal and goal-completion-driven; "do not stop, be like water" heuristics will override worktree/gate/log discipline unless the discipline is enforced by self-check ("If you are not in `../agent-memory-wt/task-NN/`, stop"). Goal-completion language must be qualified: "find creative solutions to blockers _within the task's owned files and gate_."
- **Mandatory checkpoint trigger.** "If you have been blocked on the same root cause for >30 minutes, stop. Write `docs/plans/<plan-name>-execution-log.md` with the blocker description, what you tried, and what would unblock. Do not retry until you've written that file." Converts "loop" appearance into actionable handoff.
- **Plan-tracker is the source of truth, not a side artifact.** `update_plan` is required at every task boundary.
- **Pre-bake the macOS Gatekeeper workaround in any long Rust gate script.** `CARGO_TARGET_DIR=$(mktemp -d)` + `PATH` purge of `cargo-nextest` and `sccache`. Long cargo runs pin `syspolicyd`/`CSExattrCrypto`; isolated target dir + plain Cargo avoids it.
- **Transcript triage rule (for Trey):** ask "is each cycle resolving a different root cause, or the same one?" Different = real progress, let him cook. Same = actual loop, interrupt. The 5/7 run was the former and got interrupted as if it were the latter.
- **`/codex` (gpt-5.5 xhigh) is a good rescue when Claude has been spinning >15-20 min.** Fresh context, different inductive bias. If your first two or three theories were structurally similar, that's a signal your model of the problem is wrong, not that the next theory will be different. Hand off via `codex:codex-rescue`.
- **List the cwd before theorizing about tool output.** If a tool reports paths that `stat` denies exist, `ls -la` the workspace root before going deeper. Filesystem path output from one tool argument-split into another is a real failure mode (5/11 oxfmt-with-spaces-in-dir-names).
- **For triage when Codex stops:** read `~/.codex/sessions/2026/MM/DD/rollout-*.jsonl` for the one with `cwd: /Users/treygoff/Code/agent-memory`. Don't trust git state alone — if Codex didn't commit, the work is real but invisible to `git log`.

## Stream-by-stream landings

For current behavior of each stream, consult its live spec and API doc — not this status block. Shipment notes:

- **Stream A** — `d227dce` on `main`, all 13 v0.3-plan tasks integrated (~41k LOC, 183 files). Release-certified in `docs/reviews/stream-a-final-review.md`. FTS5 sanitization fix in `946d75f` (Trey-authorized Stream A touch).
- **Stream B** — `f9d9c2b` (2026-04-28); F-001 remediation added `memoryd mcp --socket <path>` stdio MCP server in `f1` (2026-05-02). Nine-tool MCP forwarder, watch::Receiver-driven graceful shutdown, panic-aware worker health, SIGINT/SIGTERM.
- **Stream C** — `6f583ec` (2026-04-29). `crates/memory-governance`, fail-closed policy loading, deterministic decisions, grounding/tombstone/contradiction/supersession/review-queue modules, daemon wiring for `memory_write`/`memory_supersede`/`memory_forget`.
- **Stream D** — `17a0a04` + Claude-review fix `5f7d926` (2026-04-29). `crates/memory-privacy`, three storage actions (`Plaintext|EncryptAtRest|Refuse`) decoupled from tier. Stream A surface touch: `Substrate::record_encrypted_content_revealed` + `EventKind::EncryptedContentRevealed` (authorized by spec §4). Reviews on disk: `docs/reviews/stream-d-{correctness,performance,security}-review.md` + `stream-d-claude-review.md`. 345 tests, 72 suites, clippy + rustdoc clean.
- **Stream E** — Shipped 2026-04-30 from plan revision v0.4 against spec v0.5. Additive `MemoryQuery` filters + `query_recall_index`; `<memory-recall version="stream-e-v0.5">` XML; delta no-match emits exactly `<memory-delta empty="true" />`. Perf evidence in `bench/stream-e-recall-results.darwin-arm64.json`.
- **Stream F** — Shipped. Adds `memory_observe` (9th MCP tool) alongside unchanged `memory_note`. Six new top-level path families (`substrate/`, `encrypted/substrate/`, `dreams/journal/`, `dreams/questions/`, `dreams/cleanup/`, `leases/`); merge-driver semantics for each; `EventKind::SubstrateFragmentWritten`. Final gate evidence in `docs/reviews/stream-f-final-gate-report.md`.
- **Stream G** — Shipped. TUI + localhost web dashboard + Reality Check + `NotificationEvent` dispatcher + trust artifact rendering + `EventKind::RecallHit` + `reality_check_due` pending-attention integration. Canonical baseline at `bench/stream-g-observability-results.darwin-arm64.json`. Deferred v1.1+: policy-editor/sync-dashboard web sections, remote dashboard auth, richer notification diagnostics.
- **Stream H** — Shipped. `crates/memorum-eval`, 19-test catalog, JSON reporting, CI workflow, T19 peer-update framing slot. Real harness validation still auth-dependent. Reports real assertion counts and runtime skip markers (no fabricated rows).
- **Stream I** — Shipped. `crates/memorum-coordination`, daemon heartbeat/status/activity/release-lock surfaces, recall XML insertion for `<peer-update>` / `<peer-presence>`, per-project `concurrent_session_mode`, cross-device startup peer updates. Baseline at `bench/stream-i-cross-session-results.darwin-arm64.json`.

## Authoritative documents

When Trey says "the spec" or "the plan" without a version, he means the latest. Older versions stay on disk for history — do not consult them for current behavior. When asked "where are we," check `git status`, `git worktree list`, `git log --oneline -20`, and the plan's task list — don't infer from older docs.

| Stream | Live spec                                    | Live plan                                                                   | API/dev docs                                                                                                    |
| ------ | -------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| System | `docs/specs/system-v0.2.md`                  | —                                                                           | —                                                                                                               |
| A      | `docs/specs/stream-a-core-substrate-v1.1.md` | `docs/plans/2026-04-26-stream-a-core-substrate-implementation-plan-v0.3.md` | `docs/api/stream-a-public-api.md`, `docs/dev/stream-a-architecture.md`                                          |
| B      | (in system spec)                             | `docs/plans/2026-04-28-stream-b-daemon-mcp.md`                              | —                                                                                                               |
| C      | (in system spec)                             | `docs/plans/2026-04-29-stream-c-governance.md`                              | `docs/api/stream-c-governance-api.md`, `docs/runbooks/governance-review.md`                                     |
| D      | `docs/specs/stream-d-privacy-v0.1.md`        | (in C plan)                                                                 | `docs/api/stream-d-privacy-api.md`                                                                              |
| E      | `docs/specs/stream-e-passive-recall-v0.5.md` | `docs/plans/2026-04-30-stream-e-passive-recall.md` (rev v0.4)               | `docs/api/stream-e-passive-recall-api.md`                                                                       |
| F      | `docs/specs/stream-f-dreaming-v0.3.md`       | `docs/plans/2026-04-30-stream-f-dreaming.md`                                | `docs/api/stream-f-dreaming-api.md`                                                                             |
| G      | `docs/specs/stream-g-observability-v0.1.md`  | `docs/plans/2026-05-01-stream-g-observability.md`                           | `docs/api/stream-g-observability-api.md`, `docs/dev/stream-g-architecture.md`, `docs/runbooks/reality-check.md` |
| H      | `docs/specs/stream-h-eval-harness-v0.1.md`   | `docs/plans/2026-05-01-stream-h-eval-harness.md`                            | `docs/api/stream-h-eval-api.md`                                                                                 |
| I      | `docs/specs/stream-i-cross-session-v0.1.md`  | `docs/plans/2026-05-01-stream-i-cross-session.md`                           | `docs/api/stream-i-cross-session-api.md`, `docs/dev/stream-i-architecture.md`                                   |

Other refs:

- **Plan reviews for H/I + Stream G context:** `docs/reviews/stream-{g,h,i}-spec-review.md`, `stream-{g,h,i}-plan-review.md`, `stream-ghi-combined-plan-review.md` (pass 1, BLOCK), `stream-ghi-combined-plan-review-pass-2.md` (pass 2, RISK no blockers — greenlit). Read these before reviewing H/I implementation PRs or Stream G regressions — they document the four-blocker fix loop and the design rationale for `memory_supersession` as a derived projection, the events_log mirror's dual-write semantics, and the NULL-`source_harness`-as-conservative-floor decision.
- **Pre-repo design history:** `docs/handoff-2026-04-23.md`.
- **Background research (not contracts):** `docs/reference/handbook-v2.2.md`, `docs/reference/gpt-deep-research-2026-04-23.md`.
- **Repo inventory:** `docs/dev/repo-layout.md` (crate-by-crate disk layout).

## Spec/plan conventions

- Spec and plan files are **versioned by suffix** (`-v1.1.md`, `-v0.5.md`). New versions supersede; old versions stay on disk for history. Never mutate an older version.
- Spec changes that affect the implementation contract get a version bump and a "Revision goal" entry at the top.
- Additive public-surface amendments may stay in-version with a dated amendment block when they add no new required behavior for existing callers. Behavior-changing changes, return-shape changes, removed/renamed surface, and new enforced invariants require a version bump unless Trey explicitly directs otherwise.
- Plan changes get a "Plan revision history" entry. Plan and spec revisions are independent counters (Stream E shipped from plan v0.4 against spec v0.5).
- If a spec and its plan drift on contract details (DTO shape, version string, deferral list, etc.), that's a bug — surface it.

## Codex-isms in the plans (don't try to translate)

Plans authored by Codex for Codex execution. These are intentional and not Claude conventions:

- Subagent type names like `heavy_worker`, `cli_developer`, `backend_arch`, `code_mapper`, `plan_checker`, `test_hardener`, `performance_engineer`, `security_auditor`, `review_guard`, `reviewer`, `docs_editor`, `docs_researcher`, `fast_worker` — Codex's custom subagent system.
- Slash commands `/clean-code` and `/tdd` — Codex skill invocation.
- `update_plan` — Codex CLI's plan tracker.
- "Spawn `<agent>`" — Codex spawn syntax.

When reviewing the plan, treat all of the above as idiomatic for the target runtime; do not flag them as missing/wrong.

## Repository state strategy (Codex's)

- `main` is the only long-lived branch, fast-forward only.
- Each task runs in its own git worktree at `../agent-memory-wt/task-<NN>/` on a `stream-a/task-<NN>-<slug>` branch.
- Workers run only targeted checks or `pnpm run check:fast`; `pnpm run check:local` is the integration/milestone gate; `scripts/check.sh` / `pnpm run check:full` runs only for final/pre-merge validation, not inside every task worktree (stub modules from unstarted tasks can fail workspace tests for the wrong reason).
- `Cargo.lock` and `pnpm-lock.yaml` are orchestrator-merged. Workers update `Cargo.toml` only.
- Don't touch Codex's in-flight worktrees or branches without checking with Trey.

## Critical invariants (will fail review if violated)

Spec-mandated, not preferences:

1. **`secret` is never persisted to disk.** It's a `ClassificationOutcome` value supplied per-write by Stream D; Stream A returns `WriteFailureKind::SecretRefused` before any disk effect (spec §8.7).
2. **Every write request carries a `ClassificationOutcome`.** No defaults. Plaintext writes with `RequiresEncryption` classification get `EncryptionRequired`; `Trusted` with sensitive frontmatter gets `ClassificationSensitivityMismatch`.
3. **Embedding triple `(provider, model_ref, dimension)` is identity, not flavor.** Mismatch returns typed errors (`DimensionMismatch`, `UnknownEmbeddingTriple`) — never silent fallback (spec §10.2.2).
4. **Device IDs live only in local runtime state**, never in the synced `config.yaml`. A fresh clone must regenerate device identity via `git::adopt_clone` before any write.
5. **`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`** is the single source of truth for the merge driver's schema gate. No magic numbers (spec §14.2).
6. **Two-clone convergence** is canonical-content equality per spec §13.6.1, not raw `git diff`.
7. **Performance baselines** at `bench/baseline.<profile>.json` are updated only by explicit human-authored commits — the bench harness never overwrites them (spec §17.6, §18.9).

## Running review or sanity-check work

When Trey asks "review this" or "is this ready":

1. Read the live spec and plan sections relevant to the question (see the Authoritative documents table).
2. **Read the actual files in the repo, not just the plan's description.** Plan-reviewer caught three pre-build Stream E blockers (private `safe_plaintext_fragment` collision in `handlers.rs:1553`, missing `index_body` column for the recall-index API, doctor-vs-hot-path contradiction in §9.5) by reading the shipped code. Don't skip that step.
3. For plan reviews, brief the `plan-reviewer` subagent with the Codex-conventions caveat (subagent types, slash commands, `update_plan` are intentional). When a plan has been through prior reviews, tell plan-reviewer that explicitly so it doesn't waste cycles re-finding what's already fixed.
4. Report blockers vs risks vs nits separately. Trey wants real adversarial critique, not validation.

## What NOT to do

- Don't run `cargo test --workspace` or `pnpm run check:full` inside Codex's task worktrees by default — see "Repository state strategy." Targeted checks or `pnpm run check:fast`; escalate at the right boundary.
- Don't run `git pull` on `/Users/treygoff/Code/agentlinters` — SHA is pinned at `91446bb`, assets are copied from there.
- Don't overwrite `bench/baseline.*.json` programmatically — explicit human commits only.
- Don't bump spec or plan versions without Trey's explicit ask.
- Don't add `secret` as a frontmatter `sensitivity` value anywhere — runtime `ClassificationOutcome` only.
- Don't use `cargo generate-lockfile` for integration work — use `cargo build --workspace --locked` + targeted `cargo update -p <crate>`.

## Project-local agents and skills

- **Skills (project-active):** `clean-code` and `rust-engineer` are symlinked under `.claude/skills/` via `claude-skill add`. They auto-load each session. Reach for `rust-engineer` proactively for ownership/lifetime/async-tokio work; reach for `clean-code` when reviewing or hardening.
- **Agents:** None defined yet. If Trey asks for a per-project agent (e.g. a Rust-aware reviewer specific to substrate boundaries), the convention is `.claude/agents/<name>.md`.
