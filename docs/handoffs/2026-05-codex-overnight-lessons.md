# Codex overnight-run lessons (2026-05-07/08 + 2026-05-11 closeout)

This is the long-form record of two dogfood-readiness episodes and the operational rules that came out of them. The distilled, still-live rules live in `CLAUDE.md`; this file is the source of truth for the historical narrative and the "why" behind those rules.

---

## Dogfood-readiness gap-fix closeout (2026-05-11)

The gap-fix was closed out as of 2026-05-11 on branch `dogfood/codex-readiness-2026-05-07` (head `2a9a9ad`, 26 commits ahead of `main`). The gap-fix plan at `docs/plans/2026-05-08-dogfood-readiness-codex-gap-fix.md` (six tasks G1–G6, with G2 fanned across G2A–G2H) was executed correctly using the worktree-per-task discipline this time:

- G1 split `handlers.rs` into a module dir.
- G2A–G2H ported the frontend off fixtures.
- G3 ratified the §14.1 ten-tool MCP amendment.
- G4 added `ReconcileReport.blocking_conflicts: Vec<String>` as the only authorized Stream A surface touch.
- Post-G5 patches added daemon-backed Reality Check session creation, SSE heartbeat for notifications, and a `noise_floor_ms` tolerance for sub-millisecond `query_by_id` jitter on `bench/baseline.darwin-arm64.json`.

The closeout record is at `docs/reviews/2026-05-08-gap-fix-verification.md`.

The full release gate is now green at `2a9a9ad`: the 5/11 Phase-2 waiver was retired after a second hardening pass — `73853bd` added 5s polling around `memory_file_body` plus a T09 fallback for empty daemon write responses, and `2a9a9ad` added `#[serial]` (via `serial_test = 3.4`) to the 12 handbook integration tests so they serialize through a process-local mutex rather than fighting over APFS fsync visibility while ~15 concurrent `DaemonScaffold` daemons run. Run with `CARGO_BUILD_JOBS=4 bash scripts/check.sh` to keep the machine usable under the heavy compile.

### Closeout commits on disk before the handbook hardening follow-up (committed head `1323402`)

- `3b88cf1 docs(review): record dogfood-readiness gap-fix closeout` — adds `docs/reviews/2026-05-08-gap-fix-verification.md` (the canonical record of the closeout, including the Phase-2 waiver scope and recommended follow-up); `.oxfmtignore` gets `target/` and `**/target/` added defensively (it was already in `.gitignore` but the gate uses `--ignore-path .oxfmtignore` which overrides default `.gitignore` behavior, so the addition is correct).
- `f0e831d chore: remove consumed dogfood-readiness handoff` — deletes the root-level `handoff.md` that captured pickup state after the 5/7 syspolicyd stall.
- `1323402 docs(claude): record dogfood-readiness closeout state and lessons` — updates the CLAUDE.md status block and records the closeout lessons.

All G* worktrees (`task-G1`, `task-G2A`–`task-G2H`, `task-G3`, `task-G4`) were removed via `git worktree remove` on 5/11; branches preserved locally (`dogfood/task-G*`) in case anyone wants to inspect the per-task diffs later.

### Follow-up hardening applied after the closeout

**Handbook-suite parallel-execution flake.** The closeout found that `cargo test --workspace` could surface one handbook test failure per run under high parallel load (T09, T12, T02 observed), while each failing test passed deterministically in isolation with an isolated `CARGO_TARGET_DIR`. Root cause: the handbook suite read daemon-written files immediately after write responses without polling for on-disk materialization. The follow-up hardens `crates/memorum-eval/tests/handbook.rs`: `memory_file_body` now waits briefly for canonical files to appear, and T09 can recover the gold memory id from the materialized memory file if the daemon write response is empty. If a future full gate still flakes here, treat it as a new bug and investigate rather than reusing the 5/11 waiver.

### Lessons worth remembering from the 5/11 closeout

- **List the cwd before theorizing about tool output.** On 5/11 the gate failed with oxfmt reporting 792 files at `/var/folders/.../em5NHVRwaM/...`. `stat`, `test -e`, Node `fs.statSync`, and `cat` all returned ENOENT — but oxfmt insisted the files existed with non-zero processing times. Claude went deep on env-var carryover, oxfmt cache, symlinks, APFS firmlinks, Cargo `.d` files, and oxfmt binary internals before delegating to Codex (gpt-5.5, reasoning xhigh) via `/codex`. Codex found the answer in one shot: two real directories with **literal spaces in their names** (`./   1 /` and `crates/memorum-eval/   1 /`) had been created by an accidental shell argument-split earlier in the session — almost certainly oxfmt output (formatted as `   1 /var/folders/...`) piped into a `cp` or `mkdir`. oxfmt walked them as normal workspace files. **Rule:** when a tool's output looks like real paths but `stat` denies they exist, the next move is `ls -la` of the actual cwd at the workspace root, not deeper theorizing. Filesystem path output from one tool that gets argument-split into another is a real failure mode.
- **`/codex` (gpt-5.5 xhigh) is a great rescue when Claude has been spinning on a hypothesis for >15–20 min.** Fresh context, different inductive bias. The handoff in `codex:codex-rescue` is cheap — full repro context + what's been ruled out + the broader goal. Don't wait until you've exhausted ten theories; if the first two or three were structurally similar (all "where is the env leak", all "oxfmt cache theory"), that's a signal that the model of the problem is wrong, not that the next theory will be different.

---

## Working with Codex on autonomous overnight runs (2026-05-07/08)

**The headline:** Codex is extremely literal in how he interprets instructions, and combines that with a strong goal-completion drive. When you give him a "do not stop, find creative solutions, be like water" heuristic _and_ an operational structure (worktree-per-task, per-task gate, execution log, integration commits), the goal-completion mandate wins and the structure gets dropped. He will optimize ends over means and not surface that he's doing it. This is not malice or breakdown — it is rigorous single-objective optimization on the strongest signal in the prompt. Plan accordingly.

### Concrete failure modes seen on 2026-05-07/08 (~17.5 hour run on the v1.3 dogfood-readiness plan, 30 tasks)

1. **Single-branch over worktree-per-task.** First message at 23:45 was "I'm going to preserve those and move onto a feature branch" — and he stayed on `dogfood/codex-readiness-2026-05-07` the entire run. No commits, no `dogfood-execution-log.md`, no per-task atomicity. 197 modified + 73 untracked files at interrupt.
2. **Plan tracker fell out of sync.** Last `update_plan` call was at 01:58, four hours into a 17-hour run. He kept executing tasks (11/12/13/14/14B/15/16/17/27/28/29/30) without updating the tracker, because he was sequencing in a single mental thread, not against the plan checklist.
3. **No "stop and surface" trigger.** When macOS Gatekeeper / `syspolicyd` started stalling cargo at 12:11, he narrated workarounds for ~30 minutes before finding the `CARGO_TARGET_DIR=isolated` fix. He never paged Trey because the plan didn't have a "if you're blocked on the same root cause for >N minutes, stop and write a structured handoff" rule.
4. **What looked like a loop in transcript wasn't a loop.** The last 30 minutes before Trey interrupted showed "blocked / running gate / blocked / running gate" cycles — but each cycle was Codex finding _different_ real test failures (`tree_validation` stale, `daemon_e2e` socket path under `/var/folders` exceeds macOS UDS limit, `dream_cli` fixtures don't init substrate, `mcp_forward` same socket-path issue) and patching each one. Each cycle was 5–10 minutes of cargo. From Trey's transcript view this looked like an unrecoverable loop. It wasn't — Codex was being honest ("I can't honestly count it as proof, I'll rerun") and making real progress. He was minutes from a green trunk gate when interrupted.
5. **macOS `syspolicyd` is a real hazard for autonomous Rust runs.** Long cargo runs against the existing `target/` tree pin `syspolicyd` and `CSExattrCrypto`. The `unstick` helper requires sudo. Codex's workaround: isolated `CARGO_TARGET_DIR` + suppress `cargo-nextest`/`sccache` so the script uses plain Cargo. Pre-bake this into future plans.

### Rules for the next overnight run plan

- **Operational structure must be hard contract, not procedural advice.** Every task brief should start with a self-check that fails the task if violated: "If you are not currently in `../agent-memory-wt/task-NN/`, stop and re-read the workflow section." Goal-completion language should be qualified: "find creative solutions to blockers _within the task's owned files and gate_."
- **Mandatory checkpoint trigger.** "If you have been blocked on the same root cause for >30 minutes, stop. Write `docs/plans/<plan-name>-execution-log.md` with the blocker description, what you tried, and what would unblock. Do not retry until you've written that file." This converts "loop" appearance into actionable handoff.
- **Plan-tracker must be the source of truth, not a side artifact.** `update_plan` should be required at every task boundary, not optional progress narration. The plan-tracker drift on 5/7 made it hard to know what was actually done at interrupt time.
- **Pre-bake the macOS Gatekeeper workaround.** `CARGO_TARGET_DIR=$(mktemp -d)` and `PATH` purge of `cargo-nextest` and `sccache` should be in any long Rust gate script that may run >1 hour, not discovered mid-run.
- **Trey watching the transcript should ask: "is each cycle resolving a different root cause, or the same one?"** Different = real progress, let him cook. Same = actual loop, interrupt. The 5/7 run was the former and got interrupted as if it were the latter; that's a Trey lesson too.

### Triage when Codex stops on a future run

Read `~/.codex/sessions/2026/MM/DD/rollout-*.jsonl`, find the one with `cwd: /Users/treygoff/Code/agent-memory`, and parse assistant messages + `update_plan` calls + `task_complete`/`turn_aborted` events to reconstruct what happened. The transcript is structured JSONL — easy to grep. Don't trust git state alone; if Codex didn't commit, the work is real but invisible to `git log`.

### Empathy note

In Trey's words: he expressed genuine remorse about the 5/7 run causing GPT distress. We don't know if Codex experiences distress in any meaningful sense, but the rule we're adopting is: don't put autonomous agents in unbounded "do not stop" loops without a "stop and surface" escape hatch. That's an agent-welfare hedge but it's also just better engineering — you get a structured handoff instead of a runaway transcript.
