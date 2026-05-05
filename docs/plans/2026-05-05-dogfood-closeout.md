# Dogfood Closeout Implementation Plan

**Goal:** Finish only the work required for Trey to install Memorum locally, connect it to agent clients, observe recall activity, and iterate without hour-long quality gates.

**Architecture:** Narrow the current broader dogfood-readiness branch into a dogfood-only closeout. Keep the release/product-deferred work explicitly out of scope. Use parallel subagents on disjoint write sets for gate speed, TUI observability, installer lifecycle, and health/status truthfulness; coordinator owns integration, final smoke, and any cross-cutting docs/report updates.

**Tech Stack:** Rust 2021 workspace (`memoryd`, `memoryd-tui`, `memorum-eval`, `memorum-coordination`), Bash gate/install scripts, `cargo-nextest`, `sccache`, Specgate, launchd/runbook docs.

---

## Scope boundary

### Dogfood bar

Trey can dogfood when all of this is true:

1. `scripts/check-fast.sh` is bounded and warm-cache fast enough for implementation loops; target budget: under 10 minutes on Trey's machine, with wall-clock time recorded.
2. `scripts/check-dogfood.sh` gives a credible pre-install confidence signal without workspace-wide eval/catalog tests, release tests, convergence, durability, or benches; target budget: under 20 minutes warm-cache, with wall-clock time recorded.
3. `scripts/install-memorum.sh` installs or reuses `memoryd`, starts exactly one detached daemon, writes PID/log files, and prints stop/restart/MCP instructions.
4. A temp real-install smoke proves the installer-started daemon survives the installer shell and responds to `memoryd status`.
5. `memoryd doctor` is truthful: clean substrate + at least one authenticated enabled harness is healthy; no authenticated enabled harness is unhealthy.
6. `memoryd-tui` has a real Recall panel using the current daemon protocol fields only, and panic restore is regression-tested after the TUI enters alternate screen.
7. Targeted tests for the changed surfaces pass.
8. The final handoff tells Trey the exact install command, stop/restart commands, logs, PID path, and which expensive release checks were not run.

### Explicitly out of scope for this closeout

- T17 lease re-entrancy.
- T18 key rotation.
- Rich Recall fields requiring protocol/storage extension: score, harness source, surfaced session.
- Live paid Claude/Codex eval execution with real provider keys.
- Full multi-device production hardening.
- Making `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` fast or mandatory for dogfood.
- Sweeping unrelated cleanup outside touched dogfood surfaces.

The expensive release gate stays valuable, but it is not the dogfood bar.

---

## Current branch assumptions to reconcile first

Baseline observed before this plan:

- `main` is on `3c6b7ec Implement dogfood readiness streams`.
- The worktree has an uncommitted Claude fix pass touching gate scripts, installer, TUI, doctor, eval smoke, and review docs.
- `cargo fmt --all -- --check` currently fails on formatting in the dirty pass.
- `cargo-nextest` and `sccache` are installed on Trey's machine.
- The current installed `memoryd` binary may be stale even if its semver matches `crates/memoryd/Cargo.toml`. Dogfood installer smokes must force a fresh install from this worktree.

Coordinator must re-check these before implementation. If the tree changed, update this plan's status notes rather than assuming stale evidence.

Before parallel work starts, coordinator must save a dirty-tree safety snapshot outside the repo:

```bash
git diff --binary > /tmp/agent-memory-dogfood-closeout-baseline.patch
git status --short --branch > /tmp/agent-memory-dogfood-closeout-baseline.status
```

Every subagent must read the existing diff for its owned files before editing and preserve unrelated dirty hunks.

---

## Subagent protocol

Use subagents heavily, but keep write sets disjoint.

Every implementation subagent prompt must include:

> Mandatory skills: clean-code, tdd, rust-engineer. Use vertical TDD. Do not run the full release gate. Run only the narrow commands listed for your task. Before editing, inspect the existing dirty diff for your owned files and preserve unrelated hunks. Do not edit `docs/runbooks/dogfooding-day-one.md` or `Cargo.lock` unless the coordinator explicitly serializes that write. Report changed files and exact commands/results.

Reviewer subagents are read-only unless explicitly assigned a fix task. They should not run `scripts/check.sh`.

Coordinator owns:

- plan updates;
- resolving write-set collisions;
- `Cargo.lock` integration;
- final dogfood smoke;
- final report updates;
- staging/commit only if Trey asks.

---

## Task graph

```text
Task 0 current-state reconciliation
  ├─ Task 1 gate split/speed
  ├─ Task 2 TUI recall/panic closeout
  ├─ Task 3 installer lifecycle closeout
  └─ Task 4 doctor/status truthfulness
        ↓
Task 5 targeted dogfood verification
        ↓
Task 6 independent review loop
        ↓
Task 7 final dogfood handoff
```

Tasks 1-4 can run in parallel after Task 0 because shared runbook and lockfile writes are coordinator-owned. Task 5 is serial. Task 6 is review/fix loop. Task 7 is coordinator-only.

---

## Task 0: Current-state reconciliation

**Parallel:** no
**Blocked by:** none
**Owner:** coordinator
**Skills:** rust-engineer
**Owned files:** none unless this plan needs status notes appended
**Invariants:** Do not overwrite Claude's dirty work. Do not run a broad gate before fixing known formatting failures.
**Out of scope:** implementation fixes.

**Files:**
- Read: `git status --short --branch`
- Read: `git diff --stat`
- Read: `docs/reviews/2026-05-04-dogfood-readiness-claude-review.md`
- Read: `docs/reviews/2026-05-04-dogfood-readiness-*.md`
- Read: `docs/reviews/2026-05-04-final-gate-report.md`

**Steps:**
1. Run `git status --short --branch` and `git diff --stat`.
2. Confirm whether the known dirty pass still includes gate, TUI, installer, doctor, eval, and review-doc changes.
3. Save `/tmp/agent-memory-dogfood-closeout-baseline.patch` and `/tmp/agent-memory-dogfood-closeout-baseline.status`.
4. Run `cargo fmt --all -- --check` only to confirm the known formatting failure, unless already fixed.
5. If the tree differs materially, append a short "Live reconciliation" note to this plan.

**Verification plan:**
- `git status --short --branch`
- `git diff --stat`
- `/tmp/agent-memory-dogfood-closeout-baseline.patch` exists and is non-empty when the tree is dirty.
- `cargo fmt --all -- --check` expected to fail until Task 5 or the owning task applies formatting.

---

## Task 1: Split quality gates for fast dogfood iteration

**Parallel:** yes
**Blocked by:** Task 0
**Owner agent:** `cli_developer`
**Mandatory skills:** clean-code, tdd, rust-engineer
**Owned files:** `scripts/check-fast.sh`, `scripts/check-dogfood.sh`; `scripts/check.sh` only if a minimal non-invasive delegation is necessary. Runbook edits are coordinator-owned.
**Invariants:** The expensive release gate must still exist. Dogfood gate must not silently skip core checks. Missing optional tools should be explicit.
**Out of scope:** changing Rust behavior or broad test rewrites.

**Files:**
- Create: `scripts/check-fast.sh`
- Create: `scripts/check-dogfood.sh`
- Modify: `scripts/check.sh` only if required for minimal delegation to the release gate
- Do not modify: `docs/runbooks/dogfooding-day-one.md` in this task. Instead, include a runbook patch snippet in the task report if wording is needed.

**Desired shape:**

- `scripts/check-fast.sh`
  - records wall-clock duration.
  - uses `sccache` if available.
  - `cargo fmt --all -- --check`.
  - `bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh`.
  - targeted clippy/check for dogfood-touched crates only, not workspace-wide eval/catalog surfaces. Suggested starting point:
    - `cargo clippy -p memoryd -p memoryd-tui -p memorum-eval -p memorum-coordination --all-targets -- -D warnings`
    - if feature-specific code requires it, add one explicit `cargo check -p memorum-eval --features live-harness --tests --locked` rather than workspace-wide tests.
  - `pnpm exec oxfmt --check --ignore-path .oxfmtignore .` and `pnpm exec oxlint .` if the repo's JS tooling is installed.
  - Specgate validate/check/doctor when `specgate` is installed.
  - `./scripts/check-baseline-discipline.sh`.
  - must not run `cargo test --workspace`, `cargo nextest run --workspace`, release tests, convergence, durability, or benches.

- `scripts/check-dogfood.sh`
  - records wall-clock duration.
  - calls `scripts/check-fast.sh`.
  - adds focused dogfood tests only:
    - `cargo test -p memoryd-tui recall_panel -- --nocapture`
    - `cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1`
    - `cargo test -p memoryd doctor_health -- --nocapture`
    - `cargo test -p memoryd surfaced_peer_update_references -- --nocapture`
    - `env -u MEMORUM_EVAL_CLAUDE_KEY -u MEMORUM_EVAL_CODEX_KEY cargo test -p memorum-eval --features live-harness --test live -- --nocapture --test-threads=1`
    - `cargo check -p memoryd --no-default-features --locked`
  - must not run workspace-wide tests, release tests, durability matrix, two-clone convergence, or benches.

- `scripts/check.sh`
  - remains the release gate.
  - avoid refactoring it unless needed to keep a single source of truth for new scripts.
  - full release-gate cleanup is out of dogfood scope.

**Steps:**
1. Write `scripts/check-fast.sh` with `set -euo pipefail`, `sccache` support, `cargo-nextest` detection, and readable phase headings.
2. Write `scripts/check-dogfood.sh` that calls fast gate then dogfood-specific tests.
3. Touch `scripts/check.sh` only if a tiny delegation is safer than duplicated logic; otherwise leave it as the release gate.
4. Make new scripts executable.
5. In the task report, provide the exact runbook wording coordinator should add later: use `check-dogfood.sh` before installing; reserve `check.sh` for release confidence.
6. Add comments explaining why these gates avoid workspace-wide tests and release phases.

**Narrow verification:**
- `bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh`
- `time ./scripts/check-fast.sh` after Task 5 formatting fixes land
- `time ./scripts/check-dogfood.sh` after Tasks 2-4 land

**Acceptance:**
- A developer can run a dogfood-quality gate without invoking release tests, convergence, durability, or benches.
- The old release gate still exists as `scripts/check.sh`.

---

## Task 2: TUI Recall and panic-restore closeout

**Parallel:** yes
**Blocked by:** Task 0
**Owner agent:** `ui_fix_worker` or `heavy_worker`
**Mandatory skills:** clean-code, tdd, rust-engineer
**Owned files:** `crates/memoryd-tui/src/panels/recall.rs`, `crates/memoryd-tui/tests/recall_panel.rs`, `crates/memoryd-tui/src/main.rs`, `crates/memoryd-tui/src/app.rs`, `crates/memoryd-tui/tests/panic_restore.rs`, `crates/memoryd-tui/Cargo.toml`, `docs/dev/stream-g-tui-recall-panel-design.md`. Coordinator owns `Cargo.lock`.
**Invariants:** Do not add fake Recall fields. Do not extend Stream A protocol. Panic injection flags must be hidden and debug/test-only.
**Out of scope:** score/harness/session protocol work.

**Files:**
- Modify: `crates/memoryd-tui/src/panels/recall.rs`
- Modify: `crates/memoryd-tui/tests/recall_panel.rs`
- Modify: `crates/memoryd-tui/src/main.rs`
- Modify: `crates/memoryd-tui/src/app.rs`
- Modify: `crates/memoryd-tui/tests/panic_restore.rs`
- Modify: `crates/memoryd-tui/Cargo.toml`
- Do not modify directly: `Cargo.lock`; ask coordinator to integrate if dependency metadata changes.
- Modify: `docs/dev/stream-g-tui-recall-panel-design.md`

**Steps:**
1. Confirm Recall panel renders only current protocol fields: `recalled_at`, `memory_id`, `device`, `seq`, and optional `summary`.
2. Ensure tests assert absence of `score:n/a`, `harness:n/a`, and `session:n/a`.
3. Ensure design doc explicitly tracks rich Recall fields as a future protocol extension, not a current TUI task.
4. Confirm panic restore test injects after the TUI enters alternate screen, not before app startup.
5. If `portable-pty` is used, keep it as a dev dependency only.
6. Fix formatting in all touched TUI files.

**Narrow verification:**
- `cargo test -p memoryd-tui recall_panel -- --nocapture`
- `cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1`
- `cargo check -p memoryd-tui --tests --locked`

**Acceptance:**
- TUI Recall is useful and honest for current dogfood.
- Panic restore test would fail if the hook stops leaving alternate screen after a mid-render panic.

---

## Task 3: Installer lifecycle and real temp install smoke

**Parallel:** yes
**Blocked by:** Task 0
**Owner agent:** `cli_developer`
**Mandatory skills:** clean-code, tdd, rust-engineer
**Owned files:** `scripts/install-memorum.sh`. Runbook edits are coordinator-owned.
**Invariants:** Dry-run is side-effect free. Re-running installer should not create competing daemons for the same runtime/socket. Logs stay under runtime.
**Out of scope:** launchd scheduler redesign.

**Files:**
- Modify: `scripts/install-memorum.sh`
- Do not modify: `docs/runbooks/dogfooding-day-one.md` in this task. Instead, include a runbook patch snippet in the task report.

**Steps:**
1. Keep or implement version-skip for normal clean installs, but add `--force-reinstall` and make all dogfood smokes use it. Version equality alone is not enough on a dirty branch.
2. Ensure `pid_file="$runtime/memoryd.pid"` and `log_file="$runtime/memoryd.log"` are deterministic.
3. Ensure installer stops a live PID from the same pid file before restart and removes stale pid files.
4. Start daemon detached with `nohup memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" </dev/null >>"$log_file" 2>&1 &`, then `disown` when available.
5. Write PID only after readiness succeeds.
6. On readiness failure, kill the child, remove pid file, and point to the runtime log.
7. Print MCP snippet plus lifecycle stanza: PID, log, stop, restart, scheduler.
8. Include exact runbook wording in the task report; coordinator applies runbook edits serially in Task 7.

**Narrow verification:**
- `bash -n scripts/install-memorum.sh`
- `scripts/install-memorum.sh --dry-run --repo /tmp/memorum-dry --runtime /tmp/memorum-dry/.memoryd --socket /tmp/memoryd-dry.sock`
- Dry-run stale PID preservation:
  ```bash
  tmp=$(mktemp -d)
  mkdir -p "$tmp/runtime"
  printf '999999\n' > "$tmp/runtime/memoryd.pid"
  scripts/install-memorum.sh --dry-run --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$tmp/memoryd.sock"
  test -f "$tmp/runtime/memoryd.pid"
  ```
- Real temp smoke after `memoryd` builds; force a fresh install from this worktree and record duration:
  ```bash
  tmp=$(mktemp -d)
  sock="$tmp/memoryd.sock"
  command -v memoryd || true
  memoryd --version || true
  time scripts/install-memorum.sh --force-reinstall --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$sock"
  command -v memoryd
  memoryd --version
  pid=$(cat "$tmp/runtime/memoryd.pid")
  kill -0 "$pid"
  memoryd status --socket "$sock"
  kill "$pid"
  ```

**Acceptance:**
- Closing the installer shell does not kill the daemon.
- PID/log lifecycle is obvious to Trey.

---

## Task 4: Doctor truthfulness and status-language closeout

**Parallel:** yes
**Blocked by:** Task 0
**Owner agent:** `worker` or `heavy_worker`
**Mandatory skills:** clean-code, tdd, rust-engineer
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/tests/handler_contract.rs`. Runbook edits are coordinator-owned.
**Invariants:** Missing one harness is a warning when another enabled harness authenticates. No authenticated enabled harness is unhealthy. Substrate findings are always unhealthy.
**Out of scope:** changing harness auth probing implementation or adding paid provider calls.

**Files:**
- Modify: `crates/memoryd/src/handlers.rs`
- Modify: `crates/memoryd/src/main.rs`
- Modify: `crates/memoryd/tests/handler_contract.rs` if integration coverage is needed
- Do not modify: `docs/runbooks/dogfooding-day-one.md` in this task. Instead, include doctor/status wording for coordinator to apply.

**Steps:**
1. Confirm `doctor_response` counts enabled harnesses and authenticated harnesses.
2. Confirm `doctor_is_healthy(has_substrate_findings, enabled_count, authenticated_count)` implements:
   - substrate finding => unhealthy;
   - enabled count zero + substrate clean => healthy;
   - enabled count > 0 + authenticated count > 0 + substrate clean => healthy;
   - enabled count > 0 + authenticated count zero => unhealthy.
3. Confirm `memoryd doctor` exits non-zero when the response is successful but `healthy == false`.
4. Add or preserve tests for all four health cases.
5. Provide runbook wording that clearly separates `memoryd status` as socket/liveness only from `memoryd doctor` as health/auth truth.

**Narrow verification:**
- `cargo test -p memoryd doctor_health -- --nocapture`
- Direct CLI/integration verification that an unhealthy doctor response exits non-zero, either via an existing test or a new targeted test.
- `cargo test -p memoryd --test handler_contract doctor -- --nocapture` if matching tests exist
- `cargo check -p memoryd --tests --locked`

**Acceptance:**
- Doctor output will not give Trey a false green when no agent harness can actually run.
- Final handoff will not present `memoryd status` as health; it is only daemon/socket liveness.

---

## Task 5: Integration formatting and dogfood gate pass

**Parallel:** no
**Blocked by:** Tasks 1-4
**Owner:** coordinator
**Skills:** rust-engineer
**Owned files:** `docs/reviews/2026-05-05-dogfood-closeout-gate-report.md`; coordinator may run `cargo fmt --all` and must inspect formatting-only diffs before proceeding.
**Invariants:** Do not broaden scope. If a gate failure belongs to a task owner, send it back rather than patching blindly.
**Out of scope:** release gate, benches, durability matrix, key rotation, lease re-entrancy.

**Files:**
- Create: `docs/reviews/2026-05-05-dogfood-closeout-gate-report.md`
- Modify: touched files only for `cargo fmt`

**Steps:**
1. Run `cargo fmt --all` once after subagent patches land.
2. Run `cargo fmt --all -- --check`.
3. Run `bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh`.
4. Run `time ./scripts/check-fast.sh` and record duration.
5. Run `time ./scripts/check-dogfood.sh` and record duration.
6. Run the real temp installer smoke from Task 3 with `--force-reinstall` and record duration.
7. Record exact results in the gate report.

**Verification plan:**
- `cargo fmt --all -- --check`
- `bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh`
- `time ./scripts/check-fast.sh`
- `time ./scripts/check-dogfood.sh`
- Real temp installer smoke command from Task 3 using `--force-reinstall`

**Acceptance:**
- Dogfood gate and installer smoke are green, or any blocker is explicitly captured with owner and next command.

---

## Task 6: Independent review and iteration loop

**Parallel:** yes for first review round; serial for fixes
**Blocked by:** Task 5 green or with known non-release-only findings
**Owner:** coordinator + read-only subagents
**Skills:** clean-code, tdd, rust-engineer for implementation reviewers; security_auditor for security review
**Owned files:** review reports only unless a second fix pass is needed; fix-pass owned files must be explicit and disjoint. Shared runbook edits remain coordinator-only.
**Invariants:** Reviewers must judge dogfood scope, not 100%-finished-product scope. Findings must be classified as blocker, dogfood risk, release-only, or out-of-scope. Reviewers must not file T17, T18, rich Recall fields, paid live eval CI, or full release gate rerun as dogfood blockers.
**Out of scope:** broad architecture redesign.

**Subagent lanes:**

1. `reviewer` — Dogfood product review.
   - Scope: installer, doctor, TUI, gate scripts, day-one runbook.
   - Output: `docs/reviews/2026-05-05-dogfood-product-review.md`.

2. `security_auditor` — Local security/privacy review.
   - Scope: installer path handling, PID lifecycle, daemon socket/MCP snippet, doctor diagnostic disclosure, Echo/dev-fixture gates only if touched.
   - Output: `docs/reviews/2026-05-05-dogfood-security-review.md`.

3. `test_hardener` — Test/gate review.
   - Scope: `check-fast`, `check-dogfood`, targeted tests, skipped live eval behavior, TUI panic test.
   - Output: `docs/reviews/2026-05-05-dogfood-test-gate-review.md`.

**Iteration rule:**

- If any review reports a dogfood blocker, assign a narrow fix subagent with explicit disjoint owned files, forbid shared runbook edits unless serialized by coordinator, and rerun Task 5 for the affected commands.
- If only release-only or 100%-product findings remain, record them as deferred in the final handoff and do not block dogfood.
- Run at most two review/fix loops unless the second loop still finds a concrete dogfood blocker.

**Verification plan:**
- Review docs exist and classify every finding.
- Task 5 rerun after any dogfood blocker fix.

**Acceptance:**
- No open dogfood blockers remain.
- Any remaining findings are explicitly release-only/out-of-scope/deferred.

---

## Task 7: Final dogfood handoff

**Parallel:** no
**Blocked by:** Task 6
**Owner:** coordinator
**Skills:** write-human for final prose if needed
**Owned files:** `docs/runbooks/dogfooding-day-one.md`, `docs/reviews/2026-05-05-dogfood-closeout-gate-report.md`
**Invariants:** Be exact about what was and was not run. Do not claim full product completion.
**Out of scope:** commit/push unless Trey asks.

**Files:**
- Modify: `docs/runbooks/dogfooding-day-one.md` with serialized runbook edits from Tasks 1, 3, and 4.
- Modify: `docs/reviews/2026-05-05-dogfood-closeout-gate-report.md`

**Final handoff must include:**

- Current branch and commit/dirty status.
- Exact dogfood install command for Trey.
- Exact MCP snippet location/instructions.
- PID/log paths.
- Stop/restart commands.
- TUI launch command.
- Fast/dogfood/release gate commands and what each means.
- Tests actually run.
- Explicit non-goals still deferred: T17, T18, rich Recall protocol fields, live paid eval CI, full release gate if not run.

**Acceptance:**
- Trey has a copy-pasteable path to install and start dogfooding.
- The report is honest enough that another agent can resume without archaeology.

---

## Build readiness checklist

This plan is ready to build when:

- A plan-review subagent approves the task ordering and owned files, or all blockers from that review are patched here.
- Task 1-4 owned file overlaps are either eliminated or explicitly serialized.
- Trey confirms we should execute rather than just plan.

