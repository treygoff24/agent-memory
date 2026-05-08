# Dogfood-readiness gap-fix plan (post-2026-05-07 Codex run)

**Plan version:** v1.0
**Authored:** 2026-05-08
**Author:** Claude (audit) + Trey (direction)
**Target runtime:** Codex CLI, autonomous overnight or interactive
**Predecessor plan:** `docs/plans/2026-05-07-dogfood-readiness-codex.md` (v1.3, executed 2026-05-07/08)
**Predecessor commit:** `0fc3d35` ("Land Codex's autonomous dogfood-readiness v1.3 run")
**Audit basis:** five-phase parallel sonnet audit, 2026-05-08

---

## Why this plan exists

The 2026-05-07 run landed ~95% of the v1.3 dogfood-readiness plan cleanly. The full release gate is green. CLI/MCP/daemon/TUI paths are dogfood-ready.

Five gaps did not land, all clustered around one failure mode: **work the gate doesn't enforce didn't get done.** Spec amendments aren't compiled. View components stubbed against fixture data pass tests written against the same fixtures. Structural refactors that don't change behavior leave gates green. Stream A return-shape extensions whose absence doesn't break other tests look fine to a test-runner.

This plan closes those five gaps. It is much smaller than v1.3 — six tasks, mostly bounded — but the frontend port (Task G2) is genuinely large and is gated by all the others.

---

## Operational contract (READ FIRST — non-negotiable)

The 2026-05-07 lessons-learned section in `CLAUDE.md` documents how Codex's literalism + goal-completion drive caused the previous run to drop the worktree-per-task structure, plan-tracker discipline, and stop-and-surface protocol. This plan installs hard contracts to prevent that.

### Hard rules (failures of these are task failures, regardless of whether tests pass)

1. **Worktree-per-task is mandatory.** Each task runs in `../agent-memory-wt/task-G<N>/` on branch `dogfood/task-G<N>-<slug>`. The first action of every task must be a self-check:

   ```bash
   pwd | grep -q '/agent-memory-wt/task-G[0-9]\+/$' || {
       echo "FATAL: not in a task worktree" >&2
       exit 1
   }
   ```

   If you are not in a task worktree, stop and re-read this section. Do not begin task work in the parent repo.

2. **`update_plan` at every task boundary.** Before starting a task, mark it `in_progress`. Before opening a PR or committing the integrated trunk merge, mark it `completed`. The plan tracker is the source of truth, not a side artifact.

3. **Stop-and-surface trigger (mandatory).** If you have been blocked on the same root cause for >30 minutes, stop. Write `docs/plans/2026-05-08-dogfood-readiness-codex-gap-fix-execution-log.md` with: the blocker description, three things you tried, what would unblock. Do not retry until you have written that file. This converts the "loop" appearance from the 5/7 run into actionable handoff.

4. **`scripts/check.sh` runs only on integrated trunk.** Per-task gates are narrow (specified per task). The full release gate runs after integration via `integrate-task-worktree.sh` fast-forwards `main`, never inside a task worktree.

5. **No commits to `main` directly.** Only via `integrate-task-worktree.sh` or fast-forward merge from a task branch.

### macOS Gatekeeper workaround (pre-baked)

Long Rust gates pin `syspolicyd` and `CSExattrCrypto`. Pre-bake the workaround in any script that runs Cargo for >5 minutes:

```bash
export CARGO_TARGET_DIR="$(mktemp -d -t memorum-task-target)"
# Suppress nextest/sccache so plain Cargo is used:
export PATH="$(echo "$PATH" | tr ':' '\n' | grep -v 'nextest\|sccache' | paste -sd ':' -)"
```

The full-gate script `scripts/check.sh` is already patched for this; per-task narrow gates should set `CARGO_TARGET_DIR` if they exceed 5 min.

### Stream A authorization

Stream A modules (`crates/memory-substrate/`) are a frozen contract. Tasks G6 explicitly authorize a Stream A surface touch (return-shape extension only — `ReconcileReport.blocking_conflicts: Vec<String>` field, additive). No other task in this plan may touch `crates/memory-substrate/`. If a task seems to require a Stream A change, stop and surface — do not freelance.

---

## Branch and integration

- Base branch: `dogfood/codex-readiness-2026-05-07` (currently head `0fc3d35`)
- New tasks land via worktree-per-task on `dogfood/task-G<N>-<slug>`
- Final integration: rebase the gap-fix branch chain onto `dogfood/codex-readiness-2026-05-07`, full release gate, then PR to `main` as a single fat commit OR per-task commits (Trey to decide at integration time).

The release gate and `bench-regression-check` already pass against the rebaselined `bench/baseline.darwin-arm64.json` (commit `856da00`). Do not re-baseline. If a task introduces real perf regression, surface it; do not silently rebaseline.

---

## Tasks

### Task G1: Split `handlers.rs` into `handlers/` module directory

**Status from v1.3:** Planned (Task 9 in v1.3) but skipped. Plan called this "non-negotiable" because subsequent Cluster A tasks were supposed to edit `handlers/mod.rs`, not the flat file. Codex landed the `--reindex` flag but skipped the module split.

**What's wrong:**
- `crates/memoryd/src/handlers.rs` is a flat 4,315-line file (~167 KB).
- v1.3 §424–432 prescribed converting to `handlers/mod.rs` with `handlers/doctor.rs` extracted.
- Future tasks editing `handlers.rs` get a file-ownership conflict surface 4× larger than necessary.

**Parallel:** no (must land before any other task in this plan that touches handlers — currently only G6, but future plans will assume the split)
**Blocked by:** none
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/handlers/mod.rs` (new), `crates/memoryd/src/handlers/doctor.rs` (new), any callers whose imports break (expect zero — see below)
**Worktree:** `../agent-memory-wt/task-G1/` on `dogfood/task-G1-handlers-split`
**Per-task gate:** `cargo build -p memoryd && cargo test -p memoryd && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`

**Steps (do these in order, do not deviate):**

1. `git mv crates/memoryd/src/handlers.rs crates/memoryd/src/handlers/mod.rs` (preserves history; Rust treats `handlers/mod.rs` and `handlers.rs` identically for the `mod handlers;` declaration in `lib.rs`).
2. `cargo build -p memoryd` — must compile green before any code move.
3. Identify the doctor block: search `crates/memoryd/src/handlers/mod.rs` for `handle_doctor`, `doctor_is_healthy`, and any `doctor_check_*` helpers. The block is roughly 50–100 lines around the existing `--reindex` implementation.
4. Move that block into a new file `crates/memoryd/src/handlers/doctor.rs`.
5. In `mod.rs`, add `pub mod doctor;` near the other module declarations and `pub use doctor::{handle_doctor, doctor_is_healthy};` (and any helpers that callers in `server.rs` / `mcp_stdio.rs` reference) near the existing public re-exports.
6. `cargo build -p memoryd` — must compile green again. Run the per-task gate.
7. Confirm zero call sites broke: `rg -n 'handle_doctor\|doctor_is_healthy' crates/memoryd/src/` should show the same import paths as before.

**Do not** rename existing functions, change signatures, or extract anything other than the doctor block. This is a behavior-preserving structural refactor only. If the diff includes any change other than file moves + module declarations + extracted code, you have scope-crept.

**Acceptance:**
- `crates/memoryd/src/handlers.rs` no longer exists.
- `crates/memoryd/src/handlers/mod.rs` exists.
- `crates/memoryd/src/handlers/doctor.rs` exists with the doctor block extracted verbatim.
- All existing tests pass with no source change beyond imports.
- `git log --follow crates/memoryd/src/handlers/mod.rs` shows the full pre-rename history.

**Commit:**
```bash
git commit -m "refactor(memoryd): split handlers.rs into module directory; extract doctor.rs"
```

---

### Task G2: Frontend ports — replace stubs with real components and live data

**Status from v1.3:** Tasks 17D–17K were ostensibly executed. In reality the views are placeholders. This is the largest gap by far and the only one materially affecting end-user-visible behavior.

**What's wrong:**

The plan called for porting a large React handoff prototype (Inspector with 15 metadata cards × 10 kind dispatchers, Inbox with 4 layout variants, RealityCheck with 5 sub-components, full Recall + Dreams + Peers + Governance + Entities views, and full data wiring via TanStack Query + CSRF + SSE + MSW). What landed:

- `crates/memoryd-web/frontend/src/views/Inbox.tsx` — **96 lines.** Single hard-coded two-pane layout. Inline `ItemInspector` (not a separate component). No FilterPills component. No layout variants. Imports `inboxItems` from `data/fixtures` directly.
- `crates/memoryd-web/frontend/src/views/RealityCheck.tsx` — **49 lines.** Iterates over `inboxItems` from fixtures (not a real reality-check session). Buttons advance an index but don't dispatch. No QuestionStage / AnswerCards / SessionSidebar / CorrectEditor / ScoreBreakdown / CompletionCard.
- `crates/memoryd-web/frontend/src/views/Recall.tsx`, `Dreams.tsx`, `Peers.tsx`, `Governance.tsx`, `Entities.tsx` — all 1–2 KB stubs, all import from `data/fixtures`.
- No `crates/memoryd-web/frontend/src/inspector/` directory at all. Inspector composition (Task 17D) was completely skipped.
- `crates/memoryd-web/frontend/src/api/queries.ts` defines exactly four hooks: `useStatusQuery`, `useEntityGraphQuery`, `usePolicyEditorQuery`, `useSyncDashboardQuery`. **None of them are called by any view.** Every view bypasses the data layer.
- The 16-route enumeration the v1.3 plan §1165 listed (`/api/recall-hits`, `/api/audit/{id}`, `/api/audit/{id}/walk`, `/api/audit/{id}/temporal`, `/api/review`, `/api/review/action`, `/api/notifications/stream`, `/api/reality-check/respond`, `/api/reality-check/history`, etc.) has zero query/mutation hooks.

The dashboard renders, the build succeeds, the gate is green — because the gate tests fixture-data shape, not user-visible behavior.

**Scope.** This task ports the real components and wires live data. It is large enough to warrant subtasks. The original v1.3 broke this into 17D–17K (eight subtasks). We keep that structure but rename and renumber.

**Pre-task verification (mandatory):**
1. Read `docs/handoff-2026-04-23.md` — locates the React handoff prototype that 17D–17K was porting from. If you cannot find the source prototype, **stop and surface to Trey before writing any code.** Do not freelance components from imagination; the design is specified, not improvised.
2. Read the v1.3 plan §1028–1235 (Tasks 17D–17K) for the full file inventory and visual-snapshot counts.
3. Read `crates/memoryd-web/src/server.rs` `router_with_state` function to enumerate the live route set.

#### G2.A — Inspector composition (was 17D)

**Worktree:** `../agent-memory-wt/task-G2A/` on `dogfood/task-G2A-inspector`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run inspector`
**Files:** Per v1.3 §1032 — full `inspector/Inspector.tsx`, `inspector/cards/` directory (15 card components), `inspector/kinds/` directory (10 kind dispatchers), `inspector/types.ts`, `inspector/index.ts`, plus `tests/inspector/` and `tests/visual/inspector.spec.ts`.

**Acceptance:**
- `crates/memoryd-web/frontend/src/inspector/Inspector.tsx` exists and is the single Inspector entry point used by all consuming views.
- All 10 kind dispatchers exist in `inspector/kinds/`.
- All 15 metadata card components exist in `inspector/cards/`.
- Discriminated-union `InspectorItem` type covers the 10 kinds.
- Component tests assert keyboard contract (`a`/`r`/`e`/`f` for inbox-review kind, etc.).
- Visual snapshots: 10 kinds × 6 themes = 60.
- Inbox.tsx's inline `ItemInspector` is removed; Inbox imports `Inspector` from the new directory.

#### G2.B — Inbox view (was 17E)

**Worktree:** `../agent-memory-wt/task-G2B/` on `dogfood/task-G2B-inbox-view`
**Blocked by:** G2.A
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run inbox && pnpm run test:visual --run inbox && pnpm run test:e2e -- --grep inbox`
**Files:** Per v1.3 §1059 — full Inbox.tsx port + FilterPills + InboxList + 4 layout components (TwoPane/ThreePane/Drawer/ModalSheet) + view registration + tests.

**Acceptance:**
- 4 layout variants exist as separate components and are selectable via the `layout` prop.
- Drawer defaults open (per v1.3 design correction).
- 6 filter pills with `1–6` keyboard shortcuts.
- `j`/`k` row navigation, selection vs focus separation.
- Inbox row selection populates G2.A Inspector via kind dispatch.
- Visual snapshots: 4 layouts × 6 themes = 24.
- Component test, e2e test, visual snapshots all in place.

#### G2.C — RealityCheck focus mode (was 17F)

**Worktree:** `../agent-memory-wt/task-G2C/` on `dogfood/task-G2C-reality-check`
**Blocked by:** G2.B
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run realityCheck && pnpm run test:visual --run realityCheck && pnpm run test:e2e -- --grep realityCheck`
**Files:** Per v1.3 §1082 — RealityCheck.tsx + QuestionStage + AnswerCards + SessionSidebar + CorrectEditor + ScoreBreakdown + CompletionCard + view registration + tests.

**Acceptance:**
- Chrome dissolution: sidebar collapses, top bar shrinks to single strip per v1.3 §1090.
- Action buttons dispatch `RealityCheckRequest::Respond { session_id, memory_id, action: RealityCheckAction }` (verified against `crates/memoryd/src/protocol.rs:195-212` — confirm line range still accurate, `rg -n RealityCheckAction crates/memoryd/src/protocol.rs`).
- All 5 `RealityCheckAction` variants wired: `Confirm`, `Correct { new_body }`, `Forget { reason }`, `NotRelevant`, `SkipThisWeek`.
- 5 visual variants × 6 themes = 30 snapshots.
- Inline correct editor replaces card area on `k` press.

#### G2.D — Recall + Dreams views (was 17G)

**Worktree:** `../agent-memory-wt/task-G2D/` on `dogfood/task-G2D-recall-dreams`
**Blocked by:** G2.C
**Files:** Per v1.3 §1107.
**Acceptance:** Per v1.3 §1115–1119, including the 9k-event scroll-perf assertion (60fps target, virtualization required if measurement fails).

#### G2.E — Peers + Governance + Entities views (was 17H)

**Worktree:** `../agent-memory-wt/task-G2E/` on `dogfood/task-G2E-peers-gov-entities`
**Blocked by:** G2.D
**Files and acceptance:** Per v1.3 §1128–1152.

#### G2.F — Wire real data: queries, mutations, SSE, MSW (was 17I)

**Worktree:** `../agent-memory-wt/task-G2F/` on `dogfood/task-G2F-real-data-wiring`
**Blocked by:** G2.E
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run && pnpm run test:e2e && pnpm run test:visual --run`
**Files:** Per v1.3 §1157.

**This is the most important subtask.** The current `api/queries.ts` has 4 hooks; the production route set is 16 (per v1.3 §1165). Every GET route gets a `useXQuery` hook; every POST gets a `useXMutation` hook with optimistic + rollback semantics; SSE wires `/api/notifications/stream` once at App mount.

**Mandatory verification before marking this complete:**

```bash
# Every view must use queries, not fixtures, in production code:
rg -n "from.*['\"]\.\.?/data/fixtures['\"]" crates/memoryd-web/frontend/src/views/ \
  && echo "FAIL: views still import fixtures in production" && exit 1
# Fixtures may only be referenced from tests/ (MSW handlers):
rg -n "from.*['\"]\.\.?/data/fixtures['\"]" crates/memoryd-web/frontend/src/ \
  --glob '!**/tests/**' && echo "FAIL: production code imports fixtures" && exit 1
```

If those checks fail, the task is not done. Fixtures move under `tests/msw/` or remain in `data/fixtures.ts` referenced only by MSW handlers and visual-test setup — never by view components in production.

**Acceptance:**
- Every view component renders from a TanStack Query hook, not from `data/fixtures`.
- POST mutations have optimistic + rollback.
- 403 / 409 / 503 error paths render the toast/banner per v1.3 §1167.
- MSW handlers cover every route with happy / empty / heavy / error / 403 / 409 / 503 named overrides.
- SSE EventSource is created once at App mount and dispatches to a shared store.
- The grep checks above pass.

#### G2.G — Settings + keyboard + command palette (was 17J)

Already partially landed (Settings.tsx, Keymap.ts, CommandPalette.tsx all exist as ~2-3 KB). Audit them against v1.3 §1184–1196 — verify that all 5 settings tabs render, the 6 theme presets are selectable, the `?tweaks=1` dev mode works, the global keymap dispatches without firing inside text inputs, and `fuse.js` powers the palette. If anything is missing, port it; if it's already done, mark this subtask complete.

**Worktree:** `../agent-memory-wt/task-G2G/` on `dogfood/task-G2G-settings-audit`
**Blocked by:** G2.F
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run "settings|keyboard|palette" && pnpm run test:e2e -- --grep "settings|keyboard|palette"`

#### G2.H — Surface state coverage + a11y + bundle budgets + integration sweep (was 17K)

**Worktree:** `../agent-memory-wt/task-G2H/` on `dogfood/task-G2H-final-frontend-sweep`
**Blocked by:** G2.G
**Files and acceptance:** Per v1.3 §1205–1235. Bundle-size budget enforced in CI (`vite-bundle-visualizer` or equivalent); a11y axe scan on every view; e2e walks every view through happy and error paths.

**Final commit (single, after all G2.A–H land via worktree integration):**
```bash
git commit -m "feat(web): complete handoff port — Inspector, Inbox, RealityCheck, Recall, Dreams, Peers, Governance, Entities, real data wiring"
```

(One commit per subtask is also acceptable if integration sequence prefers it; coordinate with Trey at integration time.)

---

### Task G3: System spec §14.1 dated amendment block

**Status from v1.3:** Task 22 wired the 10-tool MCP manifest correctly in code, but skipped the spec-side amendment block. This is doc-only, no code impact.

**What's wrong:**
- `docs/specs/system-v0.2.md` §14.1 already lists 10 tools in the table (line 454–465).
- v1.3 Task 22 §1361 required appending a dated amendment block **inside §14.1** (immediately after the original tool-count statement, NOT at end-of-file): an explicit "**2026-05-07 amendment:** v1 MCP surface ratified at 10 tools (adds `memory_capture_source` shipped 2026-05-06 in `ab66a34`). Surface frozen at 10 for v1.x. Daemon-protocol commands (`Status`, `Doctor`, etc.) are not part of the MCP surface and are exposed via socket only."
- That block is not present.

**Parallel:** yes
**Blocked by:** none
**Owned files:** `docs/specs/system-v0.2.md`
**Worktree:** `../agent-memory-wt/task-G3/` on `dogfood/task-G3-spec-amendment`
**Per-task gate:** none (doc only — verify by `grep`)

**Steps:**

1. Open `docs/specs/system-v0.2.md`. Find §14.1 (currently at line 450).
2. Append the amendment block immediately after the existing line "The v1 contract is ten MCP tools..." (currently line 452), before the table:

   ```markdown
   **2026-05-07 amendment:** v1 MCP surface ratified at 10 tools (adds `memory_capture_source`, shipped 2026-05-06 in commit `ab66a34`). Surface frozen at 10 for v1.x. Daemon-protocol commands (`Status`, `Doctor`, `RealityCheck`, peer admin, etc.) are not part of the MCP surface and are exposed via the daemon socket only.
   ```

3. Verify the block parses as Markdown and renders correctly in any preview.

**Acceptance:**
- `grep -n "2026-05-07 amendment" docs/specs/system-v0.2.md` returns one match inside §14.1.
- The amendment text matches the exact wording above.

**Commit:**
```bash
git commit -m "docs(spec): append §14.1 dated amendment block ratifying 10-tool MCP surface"
```

---

### Task G4: `ReconcileReport.blocking_conflicts: Vec<String>` field

**Status from v1.3:** Task 28 wired 5 of 6 notification variants (LeakedSecretDetected explicitly deferred). The dispatcher and triggers landed correctly. However, `BlockingMergeConflict` was supposed to fire from the daemon's reconcile call site by reading a new field on `ReconcileReport` populated by the substrate. That field doesn't exist.

**What's wrong:**

`crates/memory-substrate/src/runtime/reconcile.rs:89-106` defines `ReconcileReport` with these fields:
- `phases_run`, `vector_repairs`, `event_repairs`, `pending_index_replays`, `reindexed_memories`, `operator_action_required`, `recovery_required`, `auto_committed`

v1.3 §1538 + §1543 prescribed adding `blocking_conflicts: Vec<String>` (additive, Stream A return-shape extension), populated from the existing quarantine path during reconcile. The daemon's reconcile call site in `crates/memoryd/src/server.rs` was supposed to read `report.blocking_conflicts` after `reconcile_all_phases` returns and emit `NotificationEvent::BlockingMergeConflict` for each entry.

Without the field:
- `BlockingMergeConflict` either never fires, or fires from somewhere else that doesn't have the right signal.
- The substrate's quarantine state is invisible to the notification dispatcher except via filesystem inspection.

**Parallel:** no (touches Stream A; do this after G1 lands so handlers/mod.rs path is stable)
**Blocked by:** G1
**Owned files:** `crates/memory-substrate/src/runtime/reconcile.rs`, `crates/memoryd/src/server.rs`, `crates/memoryd/tests/notification_fanout.rs` (extend existing test), and any reconcile callers in `crates/memory-substrate/src/runtime/` that construct `ReconcileReport`
**Worktree:** `../agent-memory-wt/task-G4/` on `dogfood/task-G4-blocking-conflicts-field`
**Per-task gate:** `cargo test -p memoryd --test notification_fanout && cargo test -p memory-substrate && cargo clippy -p memoryd -p memory-substrate --tests -- -D warnings && cargo fmt -p memoryd -p memory-substrate -- --check`

**Stream A authorization:** explicit — return-shape extension only (additive `Vec<String>` field with `#[derive(Default)]`-friendly empty-vec default). No async dependency injected into the substrate crate; the daemon owns the channel side per v1.3 §1538.

**Steps:**

1. Add the field to `ReconcileReport`:
   ```rust
   /// Memory IDs whose merge was quarantined and require operator attention.
   /// Populated by the quarantine phase; consumed by the daemon to emit
   /// NotificationEvent::BlockingMergeConflict per entry.
   pub blocking_conflicts: Vec<String>,
   ```
   The struct already derives `Default` (line 88: `#[derive(Clone, Debug, Default)]`), so the empty-vec default is automatic.

2. Find the quarantine phase in `crates/memory-substrate/src/runtime/`. It populates the on-disk quarantine files; locate via `rg -n 'quarantine' crates/memory-substrate/src/runtime/`. Wherever a memory is quarantined with a known ID, push that ID onto `report.blocking_conflicts`.

3. In `crates/memoryd/src/server.rs`, find the `reconcile_all_phases` call site (`rg -n 'reconcile_all_phases' crates/memoryd/src/`). After it returns, iterate `report.blocking_conflicts` and emit one `NotificationEvent::BlockingMergeConflict` per entry through the dispatcher. Reuse the existing dispatcher API; do not invent a new emit path.

4. Extend `crates/memoryd/tests/notification_fanout.rs` to: (a) seed a quarantined memory state, (b) run reconcile, (c) assert `report.blocking_conflicts` is non-empty, (d) assert the dispatcher received `NotificationEvent::BlockingMergeConflict` per entry.

**Acceptance:**
- `ReconcileReport.blocking_conflicts: Vec<String>` field exists.
- The quarantine phase populates it.
- The daemon emits one notification per entry.
- A test asserts the end-to-end path with a real quarantined memory.
- All existing Stream A tests still pass (no behavior change for non-quarantine phases).

**Commit:**
```bash
git commit -m "feat(substrate): add ReconcileReport.blocking_conflicts; daemon emits BlockingMergeConflict per entry"
```

---

### Task G5: Pre-integration verification sweep

**Status:** new (not in v1.3). This task exists because the 5/7 run shipped lots of "looks done" work that wasn't. We don't repeat that.

**What this task does:**

After G1–G4 land (G2 may complete in parallel via subtask integration), run a verification sweep against this entire plan:

1. **Re-run the audit greps** that exposed the original gaps:
   ```bash
   # Production views must not import fixtures:
   rg -n "from.*['\"]\.\.?/data/fixtures['\"]" crates/memoryd-web/frontend/src/views/
   # Should print nothing.

   # Spec amendment must exist:
   grep -n "2026-05-07 amendment" docs/specs/system-v0.2.md
   # Should print exactly one match.

   # blocking_conflicts field must exist:
   grep -n "blocking_conflicts" crates/memory-substrate/src/runtime/reconcile.rs
   # Should match the field declaration.

   # handlers must be a directory:
   test -d crates/memoryd/src/handlers || echo "FAIL: handlers/ not a directory"
   test ! -f crates/memoryd/src/handlers.rs || echo "FAIL: flat handlers.rs still exists"
   ```

2. **Run the full release gate:** `BENCH_PROFILE=darwin-arm64 ./scripts/check.sh`. All phases must pass: fmt, oxfmt, oxlint, baseline-discipline, specgate, clippy, debug + release tests, doctests, rustdoc, rust-boundary, two-clone-convergence, durability-probe-gate, bench-gate, bench-regression-check.

3. **Run the frontend gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run && pnpm run test:visual --run && pnpm run test:e2e`.

4. **Smoke test the dashboard against a live daemon** (manual but mandatory):
   - Start `memoryd serve` in one terminal.
   - Open the localhost dashboard; navigate every view (Inbox, RealityCheck, Recall, Dreams, Peers, Governance, Entities, Settings).
   - Confirm each view renders with real data — open the browser DevTools network tab and verify each view fires API calls. **If any view renders without firing a network request, G2.F is incomplete and must be reopened.**
   - Click through one Reality Check session (Confirm / Correct / Forget / Skip). Verify each action fires the correct mutation and the UI updates.
   - Trigger a notification (e.g., manually quarantine a memory to fire `BlockingMergeConflict`); verify the SSE bell shows it.

5. **Document the sweep result** in `docs/reviews/2026-05-08-gap-fix-verification.md` with timestamps, gate output summary, and any new findings.

**This task does not write code.** It verifies. If verification fails, it reopens the relevant G-task; it does not silently patch.

**Worktree:** none (run from `dogfood/codex-readiness-2026-05-07` after G1–G4 integrate).

---

## Sequencing summary

```
G1 (handlers split) ────┐
                        │
G3 (spec amendment) ────┤── parallel ────────────────┐
                        │                            │
G2 (frontend, 8 subs)───┤                            │
                        │                            │
G1 → G4 (blocking_conflicts) ────────────────────────┤
                                                     │
                                                     v
                                              G5 (verification)
                                                     │
                                                     v
                                              integrate to main
```

G1 must land before G4 (handlers path stability). G3 is doc-only and parallel-safe. G2 is large but independent of the others; its 8 subtasks (G2.A–H) run in their own dependency chain (A → B → C → D → E → F → G → H).

---

## Out of scope (do not freelance)

- LeakedSecretDetected notification variant: explicitly deferred to the post-dogfood privacy refactor per v1.3 §1530. Do not wire it.
- Performance baseline updates: the rebaselined `bench/baseline.darwin-arm64.json` (commit `856da00`) is current. Do not overwrite. If a real regression appears, surface it via the stop-and-surface protocol; do not silently rebaseline.
- Stream A surface touches beyond `ReconcileReport.blocking_conflicts`: any other Stream A change is out of scope and requires explicit Trey authorization.
- New test infrastructure (nextest replacement, alternative fuzzers, etc.): out of scope.
- Documentation rewrites beyond the single §14.1 amendment in G3.

---

## What "done" looks like

- All G-tasks marked `completed` in `update_plan`.
- `docs/reviews/2026-05-08-gap-fix-verification.md` exists with green sweep results.
- Full release gate green on the integration branch.
- Manual dashboard smoke test (G5 step 4) passes.
- A single integration commit (or per-task commit chain) lands on `dogfood/codex-readiness-2026-05-07`.
- Trey reviews and decides whether to PR to `main` or hold for further dogfood.

---

## Plan revision history

- **v1.0 (2026-05-08):** Initial. Authored after the 5-phase parallel audit of commit `0fc3d35` exposed five gaps from the v1.3 run. Core insight from the audit: gate-green is necessary but not sufficient; gaps cluster in work the gate doesn't enforce (spec amendments, view↔data wiring, structural refactors that don't change behavior, additive struct fields whose absence doesn't break compiling tests).
