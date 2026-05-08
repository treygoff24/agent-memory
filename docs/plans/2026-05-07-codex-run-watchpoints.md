# Codex Autonomous Run — Watchpoints & Triage Guide

**Status as of write:** Codex is executing the v1.3 dogfood-readiness plan in `/goal` mode with `approval_policy: never`. Trey explicitly said "we are yeeting it" / "find Codex's max-autonomy limits" / expects it to "shit the bed at some point." This file is for the Claude session that picks up triage when Codex stops (whether by completion, partial success, or full bedshit).

## TL;DR for the future Claude reading this

1. Read `git log --oneline main` to see which tasks integrated. Tasks land as `feat(...)` / `fix(...)` commits with `task-NN-<slug>` branches.
2. Read `docs/plans/dogfood-execution-log.md` (Codex writes one-line `task-NN | <slug> | <reason>` entries for every blocked task per the plan's failure-mode policy at line ~88).
3. Cross-reference against the plan's task list to identify which phase/cluster blocked.
4. The Cluster D-frontend chain is the highest-novelty work and the most likely to block. See the dependency map below.
5. Trey wants empirical data, not magical success. Partial completion + a clean punch list is a *good* outcome.

## Key paths

| What | Path |
|------|------|
| The plan (v1.3, the contract) | `docs/plans/2026-05-07-dogfood-readiness-codex.md` |
| Codex's execution log | `docs/plans/dogfood-execution-log.md` (Codex authors) |
| Design handoff (the contract for Phase 5) | `docs/design/dashboard-handoff/` |
| Design system reference | `docs/design/dashboard-handoff/README.md` |
| Brief that drove the design pass | `docs/design/claude-design-brief/` |
| TUI redesign reference | (Phase 4, see plan v1.0/v1.1 entries — Task 10A theme crate, 11/11B/12/13/14B Cluster C) |

## Strategic decisions baked into v1.3 (don't relitigate without cause)

- **Option B over Option A** for the dashboard frontend: React + Vite + TypeScript build pipeline integrated via `build.rs`, NOT a vanilla JS port of the React handoff. Rationale in v1.2 revision entry. If 17A wrecks, A→B revisit is on the table for v1.4 — but only if the toolchain is *fundamentally* incompatible with the workflow, not if Codex just couldn't get pnpm + cargo to play nice once.
- **17H bundles three views** (Peers / Governance / Entities) explicitly, accepting it as the heaviest single task. Plan-reviewer R6 flagged this as a critical-path risk; Trey accepted because run framing is "find limits."
- **Phase 5 expansion to 11 sub-tasks (17A–17K)** vs. v1.1's single Task 17. This is the deliberate scope-stretch test.
- **`bash scripts/check.sh` is NOT in 17K's per-task gate** (R4 fix). It runs as the post-Phase-5 trunk gate per existing protocol. Don't assume a green 17K means the trunk gate passed.
- **`pnpm-lock.yaml` is workered + committed**, not orchestrator-merged like Cargo.lock. The R5 fix is in the lockfile cadence section.

## Specific watchpoints (in expected fragility order)

### High-probability failure points

1. **Task 17A — `build.rs` invoking pnpm from cargo.** Classic seam. Failure modes: pnpm not on PATH inside cargo's env, corepack not enabled, wrong cwd, env var leakage, `--frozen-lockfile` mismatch. If 17A blocks, the entire Cluster D-frontend chain (17B–17K) is gated behind it. Triage: read `crates/memoryd-web/build.rs` actual content, compare against what 17A's spec said, see if Codex deviated.

2. **Task 17B — visual snapshot first-run determinism.** Plan says "Run gate twice" with explicit second-pass diff requirement. If Codex's mock data has timestamps / random IDs / current-time dependencies, the second pass fails for the wrong reason. Watch: if 17B blocked with "visual diff non-zero on second pass," check `tests/visual/__snapshots__/macos/` for the generated baselines and inspect what's varying.

3. **Task 17H — three-view bundle.** R6 was explicitly accepted. If 17H blocks, 17I/17J/17K block downstream. Triage: split into 17H1 (Peers) + 17H2 (Governance + Entities) for v1.4 retry.

4. **Lockfile reconciliation under parallel 17B/17C/17D.** Three concurrent worktrees each potentially adding deps means three pnpm-lock regenerations the orchestrator merges via `pnpm install --no-frozen-lockfile`. The cadence section's procedure is theoretical until it runs.

5. **MSW handler matrix in 17I.** 17I claims "every server route" gets a handler. If Tasks 15+16 add routes Codex doesn't enumerate correctly, e2e tests fail because requests go un-mocked. Triage: diff `tests/msw/handlers.ts` against actual `server.rs` `router_with_state` after 15+16 land.

### Medium-probability failure points

6. **Cluster A sequential through `handlers/mod.rs`** (Tasks 2, 9, 11A, 19–25, 28). If any task fails, downstream Cluster A tasks block. This is unrelated to dashboard work but the longest sequential chain in the plan.

7. **Task 1 OnceLock test isolation.** v0.6 split into three test binaries to avoid the OnceLock-process-global problem. If a worker collapses them back into one file, all three tests pass for the wrong reason.

8. **Visual baseline cross-platform drift.** `snapshotPathTemplate` per-platform should mitigate, but if CI regenerates Linux baselines on first run and commits them, that's a CI workflow change Codex may not have made.

9. **Reduced-motion 3-way state.** 17B's ThemeProvider needs to handle `'os' | 'on' | 'off'` correctly. Easy to ship a 2-state toggle by accident.

### Lower-probability but high-impact

10. **`scripts/check.sh` regressions from non-Cluster-D work.** Phase 5 is the most novel, but Phases 0–4 + 6–8 also execute. If any of those break the trunk gate, the whole plan stalls per failure-mode policy step 4.

11. **Worktree cleanup hygiene.** Failure-mode policy says `git worktree remove` runs after integration. If a task blocks, the worktree may persist. Manual cleanup if needed: `git worktree list`, `git worktree remove <path>`.

12. **`memoryd-web/static/` deletion in 17A breaking something not enumerated.** Plan-reviewer checked this and said clean. But empirical reality may differ.

## Dependency map for fast triage

```
Phase 0 (foundation):   Task 1 → 2 → 3 → 4 → 5 → 6      (Cluster B mostly)
Phase 1 (MCP):          7
Phase 2 (install):      8
Phase 3 (doctor/docs):  10  (10A is Cluster C blocker)
Phase 4 (TUI):          10A → 11 → 11B → 12 → 13 → 14 → 14B   (Cluster C)
                        11A is Cluster A (sequential there)
Phase 5 (dashboard):    
  Cluster D-backend:    16 → 15
  Cluster D-frontend:   17A → {17B, 17C, 17D} → 17E → 17F → 17G → 17H → 17I → 17J → 17K
  Independent:          18  (memory-source/, parallels everything)
Phase 6 (MCP fixes):    19 → 20 → 21 → 22 → 23 → 24 → 25  (Cluster A sequential)
Phase 7 (dreams):       26 → 27, 29 (28 is Cluster A)
Phase 8 (cleanup):      30
```

If 17A blocks, Phase 5 frontend is dead. Backend (15, 16) and Task 18 still run. All other phases still run.

If Cluster A blocks early (Task 2 or 9), 11A/19–25/28 all block. TUI (Cluster C) still runs because it doesn't depend on Cluster A.

If `scripts/check.sh` fails on `main` after a phase batch, orchestrator bisects by reverting most-recent integrations until trunk is green.

## Reading `dogfood-execution-log.md`

Per failure-mode policy line ~88, format is one line per blocked task:
```
task-NN | <slug> | <reason-code>
```

Reason codes seen in v1.3 plan: `merge-conflict`, `lockfile-resolve-fail`, `pnpm-lockfile-resolve-fail`, `trunk-gate-regression`, plus subagent-emitted reasons like `gate-failed-twice` or `subagent-context-exhausted`.

## When Codex stops, do this

1. **Read the log + git log + `git status`.** Identify the integration boundary (last fast-forward to `main`).
2. **Categorize tasks: integrated / blocked / unstarted.** Use the dependency map.
3. **For blocked tasks, read the worker's diagnostic.** Codex worktrees may persist with the failed branch — `git worktree list` shows them. The branch's last commit (or the lack of one) tells you where the worker got stuck.
4. **Run `bash scripts/check.sh` on `main`** to verify trunk health. If trunk is red, identify which integration broke it (orchestrator should have already bisected; verify by reading the bisect commits).
5. **Draft a v1.4 (or `2026-05-XX-dogfood-cleanup.md` if v1.3 is mostly done)** that picks up the unfinished work.
6. **Send the v1.4 to plan-reviewer** before relaunch.

## What "shitting the bed" vs. "graceful degradation" looks like

**Graceful** (good outcome, even if partial):
- Some tasks integrated cleanly, some blocked with clear log entries.
- Trunk gate green between phases.
- Worktrees cleaned up correctly.
- `dogfood-execution-log.md` has a readable punch list.

**Bedshit** (real problems):
- Trunk gate red on `main` (orchestrator bisect failed).
- Worktrees in inconsistent state (failed merges, leftover branches with `dogfood/` prefix).
- `dogfood-execution-log.md` empty or malformed despite blocked tasks.
- Cargo.lock or pnpm-lock.yaml in conflict on `main`.
- Random files modified outside any task's owned-files declaration (worker sandbox escape).

If bedshit: don't try to fix forward. Stash, reset to a known-good main, document what happened, plan v1.4 differently.

## Trey's framing (don't lose this voice)

- "We are yeeting it" — this is a stress test, not a polished delivery
- "Find Codex's max-autonomy limits" — partial failure is data, not failure
- "Robust ass e2e tests" — Trey wants objective verifiable success criteria; that's why every Task 17X gate is multi-layer
- "MIND BLOWN if this doesn't shit the bed" — expectations are calibrated
- Empirical data > magical success — the goal is to learn what Codex max-autonomy can actually do, not to prove it can do everything

When Codex stops, Trey will want a tight summary: what landed, what didn't, why, and what v1.4 looks like. Don't sandbag with caveats. Lead with the headline.
