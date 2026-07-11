# agent-memory

Implementation home for the agent-memory system. Ships as **Memorum** (Latin genitive plural of _memor_, "mindful"). All nine streams (A–I) are shipped; see the Stream model below for the one-line breakdown.

## Current status

Streams A–I are shipped. The alpha-readiness arc is substantively shipped: latest in-flight plan is `docs/plans/2026-05-25-alpha-core-gap-closeout.md` (8/10 Done, 2 Partial) with audit closeout at `docs/reviews/2026-05-26-alpha-gap-audit.md`. Stream H live real-harness validation remains environment-dependent on authenticated Claude/Codex CLIs.

**Runtime-loop foundation (2026-06-25→07-08, MERGED to `main` and live):** a 6-day single-device dogfood found the runtime loop never closed end-to-end (writes never git-commit → dreaming dead on a no-remote install → no ambient capture → doctor blind). The P0 fix set (F1 commit-on-write, F2 single-device lease, F3 recovery/dedup, F4 doctor) shipped on `foundation/runtime-loop-closure`, was superseded into `perf/daemon-memory` (embedding-lifecycle memory reduction, spec `stream-e-passive-recall-v0.7`, serve-path tracing), and **merged to `main` 2026-07-08 (`74e3250`, pushed)** after a full `scripts/check.sh` pass (green except the known-flaky bench-regression stage — see the bench-corpus note in project memory). Live spec: **`docs/specs/memorum-runtime-loop-foundation-v0.2.md`**. Plan: `docs/plans/2026-06-25-runtime-loop-foundation-implementation.md`. The full foundation dogfood gate completed live on `~/memorum`: F1 drained the 559-file backlog, the 2 governance-quarantined memories were promoted via `memoryd review approve`, doctor is `healthy: true`, and the first dream ran (pass 1/3 ok; pass 2 `malformed_pass_2_json` — model-output quality, watch on future runs). Closing the gate surfaced and fixed 4 latent bugs (`ad596a6`, `74167f5`): governance quarantines had no CLI exit, `review reject` was refused for all reasons since 6/4 (SEC-001 Me-namespace floor), reject wrote an invalid lifecycle pair, and the first dream journal crashed startup reconcile (daemon crash-loop after reboot). The live daemon runs the merged `main` build; recall hooks, MCP wiring, and the dream-scheduler plist remain deliberately torn off since 6/25 — re-wire is the next dogfood decision. This foundation is Phase 0 of the v3.0 ambient-recall redesign (`docs/specs/stream-e-ambient-recall-v3.0.md`).

**CLI-first agent surface (plan `docs/plans/2026-07-08-agent-cli-first-surface.md` — SHIPPED, merged to local `main` 2026-07-08, unpushed):** the hardened `memoryd` CLI + the `using-memorum` skill (`skills/using-memorum/SKILL.md`) are now the Tier-1 agent surface, with passive-recall lifecycle hooks wired by default (`memoryd init` defaults `--wire-mcp none`). Covered agent commands emit a v1 JSON agent envelope (contract: `docs/api/memoryd-cli-contract-v1.md`; `memoryd schema` prints it); `doctor`/`recall` keep their raw daemon frames. The MCP bridge is demoted to an **opt-in** compatibility surface (still shipped, frozen at 10 tools, wired only on `memoryd init --wire-mcp <harness>`). Live system spec is now **`docs/specs/system-v0.3.md`** (supersedes v0.2).

All 10 plan tasks are done; an Opus `/goal` run executed it end-to-end (commits `5224b14..d088899` on `main`, 11 ahead of `origin/main`). Full `scripts/check.sh` is green except the known-flaky bench-regression stage (confirmed flaky via 3-run evidence — the tripping metric changed each run: `query_by_id`, then `cold_reindex` twice). The live `~/memorum` daemon was rebuilt (`cargo install`), restarted (`launchctl kickstart -k`), re-imported (786 active memories), and re-wired with recall hooks (MCP left unwired) — the recall loop closed live in-session. Dogfood artifacts: `docs/reviews/2026-07-08-cli-first-dogfood-notes.md` and `docs/reviews/2026-07-08-canonical-loop-live-transcript.md`.

**API embedding lane (plan `docs/plans/2026-07-09-api-embedding-lane.md` — SHIPPED + LIVE 2026-07-09, on local `main`, unpushed):** opt-in, privacy-fenced Gemini embedding lane (`gemini-api` / `gemini-embedding-2` / 768, dims ratified by a real-API bake-off — table in `docs/reviews/2026-07-09-t42-api-lane-dogfood-notes.md`). Stream A fence: `EmbeddingLaneEligibility::{AllTiers, PlaintextOnly}` — API lanes embed persisted `public`/`internal` only, fail-closed; consent key `api_embedding_consent` in synced config, written only by the `memoryd config embedding-lane` ceremony and enforced by the daemon. Spec: Stream A v1.1 Amendment 2026-07-09 + `docs/specs/system-v0.3.md` pointer; operator runbook `docs/runbooks/api-embedding-lane.md`. The live `~/memorum` daemon runs the API lane at **11–17 MB footprint** (vs 6.2 GB local baseline); 28 confidential/personal jobs held local by the fence; switch-back verified with zero re-embed. The T4.2 dogfood caught and fixed three production bugs (singular `:embedContent` response shape; 250→750 ms query-embed budget vs real ~210 ms p50 latency; `import --repo` cwd default). The 7/8 idle-unload follow-up is mooted by the API lane. `using-memorum` skill installed globally for both Claude (`~/.claude/skill-library`, always-on) and Codex (`~/.codex/skills`).

**Open follow-ups:** (1) the grounding→privacy catch-22 for self-referential governed writes with no public source; (2) cosmetic — `--lane local` switch envelope still prints `approximate_tokens`/`estimated_usd`; (3) dream-scheduler plist still deliberately unwired. Next planned arc: ambient-recall v4.0 (`docs/specs/stream-e-ambient-recall-v4.0.md`).

For narrative history: `git log`. For per-stream state: read the live spec/plan listed in **Authoritative documents** below. For runtime ground truth: `git status`, `git worktree list`, `git log --oneline -20`, and the latest plan's task list — don't infer current state from older docs.

## Stream model (one-liners)

- **A** Core substrate. Canonical files, index, events, git, merge driver.
- **B** Daemon, MCP server, process lifecycle, embedding inference worker.
- **C** Governance: promotion, contradiction detection, grounding, tombstone matching.
- **D** Privacy filter: classification, age encryption, masked synthesis. Supplies `ClassificationOutcome` to A.
- **E** Recall block assembly, harness hooks.
- **F** Dreaming.
- **G** Observability: TUI, localhost web dashboard, Reality Check, notifications, trust artifact rendering.
- **H** Eval harness.
- **I** Cross-session coordination: peer updates, presence, claim locks, peer admin surfaces.

## Who's doing what

- **Codex** owned Stream A and implemented G/H/I. The worktree-per-task / per-task-gate / orchestrator-merged-lockfile workflow described below is its idiom.
- **Claude (you)** owns Stream B. Otherwise reviewer/architect in this repo: spec authorship, plan critique, plan-reviewer passes, sanity checks, ad-hoc work Trey hands you. Stream A modules are fair game for fixes.
- **Trey** drives. He'll tell you what's next.

## Authoritative documents

When Trey says "the spec" or "the plan" without a version, he means the latest. Live spec = highest-versioned `docs/specs/stream-X-...-vN.M.md` (older versions stay on disk for history; never mutate them). The parent system spec is `docs/specs/system-v0.2.md`.

| Stream | Live spec                                    | Live plan                                                                   |
| ------ | -------------------------------------------- | --------------------------------------------------------------------------- |
| A      | `docs/specs/stream-a-core-substrate-v1.2.md` | `docs/plans/2026-04-26-stream-a-core-substrate-implementation-plan-v0.3.md` |
| B      | (in system spec)                             | `docs/plans/2026-04-28-stream-b-daemon-mcp.md`                              |
| C      | (in system spec)                             | `docs/plans/2026-04-29-stream-c-governance.md`                              |
| D      | `docs/specs/stream-d-privacy-v0.1.md`        | (in Stream D plan)                                                          |
| E      | `docs/specs/stream-e-passive-recall-v0.7.md` | `docs/plans/2026-04-30-stream-e-passive-recall.md`                          |
| F      | `docs/specs/stream-f-dreaming-v0.3.md`       | `docs/plans/2026-04-30-stream-f-dreaming.md`                                |
| G      | `docs/specs/stream-g-observability-v0.1.md`  | `docs/plans/2026-05-01-stream-g-observability.md`                           |
| H      | `docs/specs/stream-h-eval-harness-v0.1.md`   | `docs/plans/2026-05-01-stream-h-eval-harness.md`                            |
| I      | `docs/specs/stream-i-cross-session-v0.1.md`  | `docs/plans/2026-05-01-stream-i-cross-session.md`                           |

API/architecture/runbook docs live at `docs/api/stream-X-*-api.md`, `docs/dev/stream-X-architecture.md`, and `docs/runbooks/*.md`. Reviews live at `docs/reviews/`. Background research (not implementation contracts): `docs/reference/handbook-v2.2.md`, `docs/reference/gpt-deep-research-2026-04-23.md`, `docs/handoff-2026-04-23.md`.

## Spec/plan conventions

- Spec and plan files are **versioned by suffix**. New versions supersede; old versions stay on disk for history. Never mutate an older version.
- Contract-affecting spec changes get a version bump and a "Revision goal" entry at the top describing what changed and why. Plan changes get a "Plan revision history" entry. Plan revisions and spec revisions are independent counters.
- Additive public-surface amendments may stay in-version with a dated amendment block when they add no new required behavior. Behavior changes, return-shape changes, removed/renamed surface, and new enforced invariants require a version bump unless Trey explicitly directs otherwise.
- If a spec and its plan drift apart on contract details (DTO shape, version string, deferral list, etc.), that's a bug — surface it.

## Codex-isms in the plan (don't try to translate)

Plans authored by Codex for Codex execution. These are intentional and not Claude Code conventions:

- **Subagent type names** like `heavy_worker`, `cli_developer`, `backend_arch`, `code_mapper`, `plan_checker`, `test_hardener`, `performance_engineer`, `security_auditor`, `review_guard`, `reviewer`, `docs_editor`, `docs_researcher`, `fast_worker` — Codex's custom subagent system.
- **Slash commands** `/clean-code` and `/tdd` — Codex skill invocation.
- **`update_plan`** — Codex CLI's plan tracker.
- **"Spawn `<agent>`"** — Codex spawn syntax.

When reviewing the plan, treat all of the above as idiomatic for the target runtime; do not flag them as missing/wrong.

## Repository state strategy

- `main` is the only long-lived branch, fast-forward only.
- Each task runs in its own git worktree at `../agent-memory-wt/task-<NN>/` on a `stream-a/task-<NN>-<slug>` branch.
- Workers run only their per-task narrow gate; **`scripts/check.sh` runs only on the integrated trunk after `integrate-task-worktree.sh` fast-forwards `main`**, never inside a task worktree (stub modules from unstarted tasks would fail workspace tests for the wrong reason).
- `Cargo.lock` and `pnpm-lock.yaml` are orchestrator-merged. Workers update `Cargo.toml` only.
- Don't touch Codex's in-flight worktrees or branches without checking with Trey.

## Build / lint / test CPU discipline (read before running ANY cargo gate)

Running cargo across the whole workspace spawns **one compiler process per crate** — 12 of them — at once. On macOS every freshly built binary is re-validated by `syspolicyd` on launch, so a 12-wide swarm pegs a core and roasts the machine. This is the single most common way an agent cooks Trey's laptop. It is not a style preference; treat it as a hard rule.

**Inner loop — always scope to the crate you touched.** You always know which crate: it's the directory under `crates/` you edited.

- `cargo check -p <crate>` — fastest; "does it still compile."
- `cargo clippy -p <crate> --all-targets -- -D warnings` — lint that crate.
- `cargo test -p <crate> -- --test-threads=2` — test that crate.

**Leaf vs foundational.** Scoping to a leaf crate (an app like `memoryd` that nothing depends on) is complete on its own. If you edit a foundational crate (`memory-substrate`, `memory-source`, …), `-p` on it alone won't catch breakage in the crates that depend on it — that's what the final full gate is for. `cargo tree -i <crate>` lists who depends on a crate if you need the ripple set mid-loop.

**Full gate — `bash scripts/check.sh`, and only this — runs ONLY:**

- on the integrated trunk (`main`), never inside a task worktree, **and**
- once, when the task is completely done — never mid-task as a progress check.

It already bakes in the macOS syspolicyd mitigation (isolated `mktemp` target dir; sccache and nextest off unless opted in) and ends with perf-sensitive benchmarks, so it must not be throttled or run speculatively. It is the one blessed heavy command.

**Never, mid-task:** bare `cargo clippy`, `cargo test` / `cargo test --workspace`, `cargo nextest run --workspace`, or `bash scripts/check.sh`. A Claude Code `PreToolUse` hook (`.claude/settings.json`) hard-blocks the bare whole-workspace clippy/test forms when you're in the main checkout and points you back here. The hook can't reach task worktrees or non-Claude harnesses — there the discipline above is yours to keep.

## Critical invariants (will fail review if violated)

These are spec-mandated, not preferences:

1. **`secret` is never persisted to disk.** It's a `ClassificationOutcome` value supplied per-write by Stream D; Stream A returns `WriteFailureKind::SecretRefused` before any disk effect (spec §8.7).
2. **Every write request carries a `ClassificationOutcome`.** No defaults. Plaintext writes with `RequiresEncryption` classification get `EncryptionRequired`; `Trusted` with sensitive frontmatter gets `ClassificationSensitivityMismatch`.
3. **Embedding triple `(provider, model_ref, dimension)` is identity, not flavor.** Mismatch returns typed errors (`DimensionMismatch`, `UnknownEmbeddingTriple`) — never silent fallback (spec §10.2.2).
4. **Device IDs live only in local runtime state**, never in the synced `config.yaml`. A fresh clone must regenerate device identity via `git::adopt_clone` before any write.
5. **`MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION`** is the single source of truth for the merge driver's schema gate. No magic numbers (spec §14.2).
6. **Two-clone convergence** is canonical-content equality per spec §13.6.1, not raw `git diff`.
7. **Performance baselines** at `bench/baseline.<profile>.json` are updated only by explicit human-authored commits — the bench harness never overwrites them (spec §17.6, §18.9).

## Lessons from past autonomous runs

Compressed from the 5/7, 5/11, and 5/22 post-mortems. The stories are in `git log` and `docs/reviews/`; the durable craft sits here.

- **Codex follows literal instructions hard.** Operational structure (worktree-per-task, gate scope, execution-log discipline) must be a hard contract with a self-check at task start, not procedural advice further down the plan. Goal-completion phrasing ("do not stop, be like water") must be qualified to the task's owned files and gate.
- **Mandatory checkpoint trigger:** blocked on the same root cause >30min → write `docs/plans/<plan>-execution-log.md` with blocker, what was tried, what would unblock. No retry until that file exists. Converts apparent-loop into actionable handoff.
- **`update_plan` is source of truth, not progress narration.** Update at every task boundary. Drift makes interrupt-time triage hard.
- **Pre-bake the macOS `syspolicyd` workaround** for any Rust gate that might run >1hr: `CARGO_TARGET_DIR=$(mktemp -d)` plus PATH purge of `cargo-nextest` and `sccache`.
- **Reading a stuck transcript:** ask "different root cause each cycle, or same one?" Different = real progress, let it cook. Same = actual loop, interrupt.
- **Triage when Codex stops mid-run:** `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` for the one with `cwd: /Users/treygoff/Code/agent-memory`. Don't trust `git status` alone — uncommitted work is invisible there.
- **Phantom paths that don't `stat`:** check `ls -la` of the actual cwd at the workspace root before deeper theorizing. Argument-split shell mishaps can create literal-space-named dirs (`./   1 /`) that tools then walk as if they were real.
- **`/codex` (gpt-5.5 xhigh) is a cheap rescue** after 15–20 min spinning on structurally-similar hypotheses. Don't wait for theory #11. Fresh context + different inductive bias.
- **`oxfmt --ignore-path .oxfmtignore` does NOT honor `.gitignore`.** Every non-source workspace-root dir (tooling output dirs like `.clawpatch/`, `.delegate/`, `.tldr/`; hand-authored prose docs whose paragraphs oxfmt would mangle) needs an explicit `.oxfmtignore` entry.
- **Self-selected verification skews green.** For gates (not inner-loop): `cargo test -p <crate> --tests` (whole crate) or `bash scripts/check.sh` (workspace), never `cargo test -p <crate> --test <file>`. Use `--no-fail-fast` with nextest. Before claiming verified: 30 seconds of `grep -rn` for "what other files reference what I edited?"
- **Don't put autonomous agents in unbounded "do not stop" loops** without a "stop and surface" escape hatch. Better engineering + agent-welfare hedge.
- **Peer-note pattern:** when you catch bugs another agent shipped, leave `docs/<date>-for-<agent>-<context>.md` explaining what shipped, what slipped, why their gate showed green, and what would help next time. Iron sharpens iron.

## Running review or sanity-check work

Standard recipe when Trey asks "review this" or "is this ready":

1. Read the live spec and plan sections relevant to the question (see Authoritative documents above).
2. Read the actual files in the repo, not just the plan's description of them. Plan-reviewer caught three pre-build Stream E blockers by reading shipped code rather than trusting the plan's prose. Don't skip that step.
3. For plan reviews, brief the `plan-reviewer` subagent with the Codex-conventions caveat. When the plan has been through prior reviews, tell plan-reviewer that explicitly so it doesn't waste cycles re-finding what's already fixed.
4. Report blockers vs risks vs nits separately. Trey wants real adversarial critique, not validation.

## What NOT to do

- Don't run `cargo test --workspace` inside Codex's task worktrees — see Repository state strategy.
- Don't run `git pull` on `/Users/treygoff/Code/agentlinters` — the SHA is pinned at `91446bb` and assets are copied from there.
- Don't overwrite `bench/baseline.*.json` programmatically — they require explicit human commits.
- Don't bump spec or plan versions without Trey's explicit ask.
- Don't add `secret` as a frontmatter `sensitivity` value anywhere. It's a runtime `ClassificationOutcome` only.
- Don't use `cargo generate-lockfile` for any integration work — use `cargo build --workspace --locked` + targeted `cargo update -p <crate>`.

## Project-local agents and skills

- **Skills (project-active):** `clean-code` and `rust-engineer` are symlinked under `.claude/skills/` via `claude-skill add`. They auto-load each session. Reach for `rust-engineer` proactively for ownership/lifetime/async-tokio work; reach for `clean-code` when reviewing or hardening.
- **Agents:** None defined yet. If Trey asks for a per-project agent (e.g. a Rust-aware reviewer specific to substrate boundaries), the convention is `.claude/agents/<name>.md`.
