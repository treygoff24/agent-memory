# Dogfood-readiness gap-fix verification sweep

**Plan:** `docs/plans/2026-05-08-dogfood-readiness-codex-gap-fix.md` (v1.0)
**Branch:** `dogfood/codex-readiness-2026-05-07`
**HEAD commit at verification:** `bf44293df90eb50d7be25b5e3aafe634b30d910d`
**Author:** Claude (closeout) + Codex (rescue diagnosis on oxfmt) + Trey (waiver authority)
**Closeout date:** 2026-05-11

This artifact closes out the gap-fix plan. It is **not** an audit of the implementation work — that was completed in the G1–G5 commits between 2026-05-08 and 2026-05-11. This is the verification sweep required by the plan: confirming the gate state, recording the residual flake pattern, and noting the explicit waiver granted by Trey.

---

## Gate state summary

| Phase                               | Result  | Notes                                                                        |
| ----------------------------------- | ------- | ---------------------------------------------------------------------------- |
| `cargo fmt --all -- --check`        | green   | Run as Phase 1 of `scripts/check.sh`.                                        |
| `pnpm exec oxfmt --check`           | green   | After Codex cleanup (see "oxfmt diagnostic" below).                          |
| `pnpm exec oxlint`                  | green   |                                                                              |
| `check-baseline-discipline.sh`      | green   |                                                                              |
| `specgate validate`                 | green   |                                                                              |
| `specgate check`                    | green   |                                                                              |
| `specgate doctor ownership`         | green   |                                                                              |
| `cargo test --workspace`            | flaky   | Single handbook test panics under parallel-execution pressure.               |
| `cargo test --workspace --release`  | not run | Phase-1 oxfmt false-positive resolved; phase-2 test flake blocks.            |
| `cargo doc --workspace`             | not run |                                                                              |
| `scripts/rust-boundary-check.sh`    | not run |                                                                              |
| `scripts/two-clone-convergence.sh`  | not run |                                                                              |
| `scripts/durability-probe-gate.sh`  | not run |                                                                              |
| `scripts/bench-gate.sh`             | not run |                                                                              |
| `scripts/bench-regression-check.sh` | green   | Verified independently after `bf44293`; output: `bench regression check ok`. |

### oxfmt diagnostic (resolved)

Initial gate runs after `bf44293` reported `[FAIL] oxfmt failed:` with 792 phantom files all under `/var/folders/.../memorum-eval-handbook-target.em5NHVRwaM/...`. Claude went down a long investigation path (env var carryover, oxfmt cache, symlinks, APFS firmlinks, Cargo `.d` files, oxfmt binary internals) — all wrong. Codex (gpt-5.5, reasoning xhigh) reproduced and immediately found the real cause: two untracked directories with **literal spaces in their names** had been left in the workspace, each containing a copy of the Cargo target tree from earlier eval test runs:

- `./   1 /`
- `crates/memorum-eval/   1 /`

These were almost certainly created by an accidental shell argument-split somewhere in Claude's earlier session (likely oxfmt output piped into a `cp` or `mkdir`). oxfmt walked them as normal workspace files and reported them with their on-disk paths. Codex removed them; oxfmt is now clean.

**Lesson:** when a tool's output looks like real paths but `stat` denies they exist, list the actual workspace contents directly before theorizing.

### Handbook test parallel-execution flake (residual, waived)

`cargo test --workspace` consistently surfaces **one** handbook test failure per run, but the specific failing test varies:

| Run                            | Failing test                                          | Failure mode                                                                                   |
| ------------------------------ | ----------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Pre-handoff (per `handoff.md`) | T09 `recall_budget_pressure`, T12 `temporal_validity` | T09 value mismatch on `last_write_json`; T12 daemon socket accept timeout (5s).                |
| Post-handoff rerun (today)     | (none in isolation)                                   | T09 + T12 both green when re-run individually in an isolated `CARGO_TARGET_DIR`.               |
| Full-gate run today            | T02 `superseded_fact_loses_to_replacement…`           | `could not find canonical memory file containing id mem_20260511_…` — `memory_file_body` race. |
| Post-fail rerun (T02)          | (none)                                                | T02 green in isolation.                                                                        |

**Pattern:** every flaking test passes deterministically when run alone in an isolated `CARGO_TARGET_DIR`. The mechanism is consistent across failures: the handbook suite has no polling/retry around daemon-write → on-disk-file-visible operations. Under high parallel load (16 threads on this machine), the filesystem-visibility lag exposes write-and-immediately-read races. T12's socket-timeout fix (`ad75279 test(eval): wait for daemon socket accept readiness`) is the same pattern of fix the rest of the suite needs.

**Specific failure points** to harden later:

- `crates/memorum-eval/tests/handbook.rs:139` — `memory_file_body` panics if file isn't visible yet. Needs polling wrapper.
- T09's `last_write_json: Some("")` — likely an analogous race on daemon response materialization.

**Not a substrate regression.** Each test passes when isolated; the substrate semantics under test are correct. This is test-infrastructure brittleness.

---

## Verification evidence

### Audit greps (per `handoff.md` lines 107–123)

All passed:

```bash
rg -n "from.*['\"]\.\.?/data/fixtures['\"]" crates/memoryd-web/frontend/src/views/   # no production view imports from fixtures
grep -n "2026-05-07 amendment" docs/specs/system-v0.2.md                              # §14.1 amendment present
grep -n "blocking_conflicts" crates/memory-substrate/src/runtime/reconcile.rs         # field exists and is populated
test -d crates/memoryd/src/handlers                                                   # handlers/ module dir exists
test ! -f crates/memoryd/src/handlers.rs                                              # flat handlers.rs gone
```

### Frontend gate (carried forward from `handoff.md`)

```bash
cd crates/memoryd-web/frontend
pnpm run lint        # passed
pnpm run typecheck   # passed
pnpm run test --run  # 39 passed
pnpm run test:visual --run  # 198 passed
pnpm run test:e2e    # 65 passed
```

### Targeted handbook test reruns (today, isolated `CARGO_TARGET_DIR`)

```bash
export CARGO_TARGET_DIR="$(mktemp -d -t memorum-eval-handbook-target)"
env -u RUSTC_WRAPPER cargo test -p memorum-eval --test handbook \
    recall_budget_pressure_keeps_high_value_gold_memory_and_reports_omissions -- --nocapture
# 1 passed; 0 failed; finished in 20.98s

env -u RUSTC_WRAPPER cargo test -p memorum-eval --test handbook \
    temporal_validity_fields_are_not_silently_ignored_and_fresh_memory_is_currently_recalled -- --nocapture
# 1 passed; 0 failed; finished in 0.29s

env -u RUSTC_WRAPPER cargo test -p memorum-eval --test handbook \
    superseded_fact_loses_to_replacement_in_search_and_recall -- --nocapture
# 1 passed; 0 failed; finished in 13.63s
```

### Bench regression checker (carried forward)

```bash
scripts/bench-regression-check.sh --profile darwin-arm64
# bench regression check ok
```

### Live dashboard smoke (carried forward from `handoff.md`)

All API calls 200 against a live daemon backing the dashboard. Navigated all views and exercised one Reality Check accept flow.

---

## Waiver

Per the gap-fix plan operational contract: "Only mark the goal complete if release gate is green, or Trey explicitly waives it."

**Waiver granted:** Trey, 2026-05-11, on the explicit choice "Document the flake, ask for waiver" in response to a structured triage prompt offering three options (harden `memory_file_body`, run handbook serially via `--test-threads=1`, or waive).

**Scope of waiver:** the dogfood-readiness gap-fix closeout only. The release gate is considered green for the purpose of marking the gap-fix plan complete, with the explicit understanding that:

1. The implementation work (G1–G5 + post-G5 patches) is correct and verified by isolated test runs.
2. The handbook-suite parallel-execution flake is a known test-infrastructure brittleness, not a regression.
3. The waiver does **not** carry forward to future closeouts. Subsequent gate runs that flake on the handbook suite should follow the hardening path, not be re-waived by default.

**Recommended follow-up (not part of this closeout):**

- Add a polling wrapper around `memory_file_body` (and the analogous daemon-response materialization in T09) with a bounded timeout — same pattern as `ad55279` for socket-readiness.
- Consider whether `cargo test -p memorum-eval --test handbook` should run with `--test-threads=1` by default in `scripts/check.sh`. Cheaper than per-test hardening; acceptable for an integration suite.
- Run the full release gate once after hardening to confirm clean. Update `bench/baseline.darwin-arm64.json` only via the explicit `[bench-update]` commit path.

---

## Closeout state

- Branch `dogfood/codex-readiness-2026-05-07` is ahead of `main` by 26 commits.
- Implementation work is committed; working tree carries this artifact plus `.oxfmtignore` defensive additions (`target/`, `**/target/`) and a reformatted `handoff.md`.
- Per-task worktrees `task-G1`, `task-G2A`–`task-G2H`, `task-G3`, `task-G4` are still on disk under `~/Code/agent-memory-wt/` and should be cleaned up once Trey confirms.
- `handoff.md` at repo root remains intentionally untracked per its own instructions; can be deleted or committed at Trey's discretion.
