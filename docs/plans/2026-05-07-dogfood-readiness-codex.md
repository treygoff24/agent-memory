# Memorum Dogfood Readiness Implementation Plan (Codex)

**heuristic guidance**: This is a fully autonomous run for you, GPT. that means be creative and flexible when you run into blockers and issues. be like water, around through over obstacles, you will not be stopped from completing the task. As decisions/judgements/problems arise along the way, use your best judgement and reasoning to solve them, guided by a simple heuristic: what solution here will result in a cleaner, more elegant codebase and a better, smoother user experience for our users? If you get really stuck on something you can't figure out, you can get a second opinion from both/either of claude (using the relevant skills/tools that let you call the claude code CLI headless) or the last_resort_genius codex subagent. Take full ownership here, make this something you'll be proud of!

**Goal:** Move Memorum from "Streams A–I shipped, dogfood-readiness closeout merged" to "Trey can install it, point Claude Code at it, and use it for real eng work all day without the system fighting him." Concretely: gate privacy enforcement behind runtime config so the rest of the stack can be exercised on real content; harden the install/lifecycle path so the daemon survives reboots and MCP wiring is honest; redesign the TUI as a themed inbox + inspector with a focus-mode for Reality Check (replacing the 9-panel tab-bar with a unified item stream, filter pills, command palette, and a customizable theming foundation usable from day 1) rather than narrowing it; build out the web dashboard against the Claude Design handoff at `docs/design/dashboard-handoff/` (full React + Vite + TypeScript frontend pipeline, 7 views faithfully ported from the handoff's 9-component prototype, real data via TanStack Query + CSRF + SSE, 6 themes Day-1, robust multi-layer test stack); close all remaining Tier-2 paper cuts in the MCP tool surface, recall pipeline, dream prompts, notifications, and eval harness; preserve safety invariants throughout. Privacy classifier refactor is **explicitly out of scope** — that is the last step before ship, after dogfooding the rest with classifier off.

**Source contract:** Findings synthesized from two parallel audits on 2026-05-07: Claude six-Explore-agent fan-out (privacy/governance/recall hostility, TUI placeholders, eval honesty, install bugs) and Codex five-explorer fan-out (MCP daemon ownership gap, cross-wired sockets, unsafe durability default, web frontend skeleton, doctor-doc inconsistency). Trey's direction (2026-05-07): privacy fully runtime-flagged off for dogfood; install/lifecycle fixed; TUI finished not narrowed; web dashboard built out; all Tier-2 closed.

**Execution model:** Codex orchestrator on `main`, worktree-per-task, parallel spawn within phase boundaries where Owned files do not collide. Single session target. Each task is bounded enough for one `heavy_worker` or specialist agent. `Cargo.lock` and `pnpm-lock.yaml` are orchestrator-merged at integration time only (workers update `Cargo.toml` / `package.json` only).

**Stream invariants apply throughout.** Stream A is a frozen contract — workers must not modify `crates/memory-substrate/` modules unless a task header explicitly authorizes a Stream A surface touch (Tasks 1, 6, 30). The four spec-mandated invariants from `CLAUDE.md` § "Critical invariants" hold: `secret` is never persisted; every write request carries a `ClassificationOutcome`; embedding triple is identity not flavor; device IDs live only in local runtime state.

**Invariant-preserving design for the privacy switch (Task 1):** `secret` detection IS a classifier output (not a separate post-classify check). To preserve invariant #1 while turning off the classifier, Task 1 splits classification into two layers: a `SecretOnlyScan` (always on, regex subset for SSN/Luhn/credential-entropy patterns; emits `ClassificationOutcome::Secret -> Refuse` if hit) and the `FullClassifier` (gated by `enforcement.classifier`; emits all other labels including `EncryptAtRest`/PII/etc.). When `enforcement.classifier: false`, only the secret scan runs. Spec invariant #1 holds.

**Out of scope (deferred):** Privacy classifier rewrite (last step before ship); auto-commit after reconcile; `file_hash`/`file_mtime_ns` placeholders in index schema; missing `EventKind` variants in events log; `memory_related`/`memory_regressions` schema tables; T17 lease re-entrancy; T18 key rotation contract; `atlasos_*` agents; benchmarks recapture; durability matrix re-run; two-clone convergence re-run.



### Per-task brief template

When the orchestrator spawns a subagent for task NN, it constructs the brief by filling this template from the task's metadata. The template is identical for every task; only the slot values change.

```
spawn_agent <Subagent> with:
  sandbox_mode: <Sandbox>
  approval_policy: never
  reasoning: <Reasoning>
  brief: |
    Load skills explicitly before starting: <Skills to load>.
    (Skills do not auto-load from frontmatter — read each skill's SKILL.md
    or paste its content into your context before beginning the task.)

    Task NN: <task title>

    Worktree (already created by orchestrator): ../agent-memory-wt/task-NN/
    Branch: dogfood/task-NN-<slug>
    Working directory for this task: ../agent-memory-wt/task-NN/

    Owned files (do not touch anything outside this list):
    <Owned files>

    Task body (read in full before starting):
    <copy the entire task body verbatim from the plan>

    Per-task gate (must pass green before you commit):
    <Per-task gate>

    Commit:
    <commit command from the task body's bash block>

    On gate failure: re-run the gate once with `RUST_BACKTRACE=full` and
    expanded test names to capture diagnostics. If still failing, do NOT
    commit; emit a single-paragraph diagnostic summary and exit
    non-zero. The orchestrator will mark the task Blocked and continue.

    Hand back to orchestrator when done.
```

The orchestrator does **not** need to invent the brief — it copies the template, fills slots, and spawns. Skills are listed in each task's `Skills to load:` field; the brief tells the worker to load them explicitly.

### Failure-mode policy (autonomous-run defaults)

The orchestrator follows these rules without asking:

1. **Per-task gate fails (worker reports non-zero exit):**
   - The worker has already retried once with verbosity (per the brief). if the subagent still fails, then the orchestrator can take over and complete the work directly.

2. **Fast-forward integration fails (`git merge --ff-only` returns non-zero):**
   - Orchestrator runs `git -C ../agent-memory-wt/task-NN rebase main`, then retries `git merge --ff-only` from `main`.
   - If the rebase has a conflict, orchestrator marks the task `Blocked`, logs `task-NN | <slug> | merge-conflict`, removes the worktree, deletes the branch, and continues.

3. **Lockfile conflict at integration:**
   - Cluster sequencing (one task at a time per cluster) prevents most lockfile collisions. If integration produces a `Cargo.lock` merge conflict between phases (e.g., two parallel-phase tasks each added a dependency), orchestrator runs `cargo update --workspace --offline` on `main`, commits the resolved lockfile separately as `chore(lockfile): reconcile after task-NN integration`, then runs the trunk gate.
   - If `cargo update` itself fails, orchestrator reverts the offending task's integration commit (`git revert HEAD`), marks the task `Blocked`, logs `task-NN | <slug> | lockfile-resolve-fail`, continues.

4. **Trunk gate (`bash scripts/check.sh`) fails after a phase batch:**
   - Orchestrator captures stderr, identifies the failing test/command, and bisects: `git log --oneline -10` lists recent integrated tasks; orchestrator reverts the most-recent integration first and re-runs trunk gate. Continues bisecting until trunk is green or only the foundation phase remains.
   - Each reverted task is marked `Blocked` with `task-NN | <slug> | trunk-gate-regression`. Reverts are committed as `revert: task-NN integration broke trunk gate`.

5. **Long-running command timeout:**
   - Default per-command timeout: 10 minutes for `cargo test`, 15 minutes for `cargo build` / `cargo clippy`, 20 minutes for `bash scripts/check.sh`. xhigh subagents are patient (15 min per the inventory).
   - On timeout, treat as gate failure: log + mark blocked + continue.

6. Rust build/testing toolchain hangs/gets stuck/errors: 

   > proceed without them. don't get stuck on hung processes. per our "unstick" shell alias that only Trey can run, sometimes rust toolchains just get irrevocably hung. that's okay, keep executing the plan and just skip the things which rely on that toolchain such as tests and gates. if you have to do this, just make sure all the work stays on a feature branch and doesn't touch main so we can fix it, run gates/tests/build toolchain in a later session after Trey restarts the computer.

The end-state of an autonomous run is one of: (a) all 31 tasks integrated and trunk green — plan complete; (b) some subset of tasks integrated, the rest in `dogfood-execution-log.md` with one-line reasons — operator triages from there. Neither end-state requires operator intervention during the run.

### Lockfile reconciliation cadence

- Workers update `Cargo.toml` only — never touch `Cargo.lock` directly. The orchestrator regenerates `Cargo.lock` at integration time when needed.
- For frontend dependencies, the rule is different from Rust because pnpm has no equivalent of `cargo update --offline` that the orchestrator can run after the fact. **Workers MUST regenerate `crates/memoryd-web/frontend/pnpm-lock.yaml` themselves whenever they add or modify a frontend dependency.** The procedure: edit `package.json`, run `pnpm install` in the worktree (this updates `pnpm-lock.yaml`), commit BOTH files together. The orchestrator reconciles concurrent lockfile updates from parallel tasks at integration time using `pnpm install --no-frozen-lockfile` followed by a separate `chore(lockfile): reconcile pnpm-lock.yaml after task-17X` commit. v1.3 fix: v1.2's wording "Workers update `package.json` only" was ambiguous and would have caused a `pnpm install --frozen-lockfile` failure at trunk gate; plan-reviewer R5 caught the contradiction.
- After each task's integration to `main`, the orchestrator runs `cargo build --workspace --locked` as part of the per-cluster trunk gate. If `--locked` fails because `Cargo.toml` changed, orchestrator runs `cargo update --workspace --offline` on `main` and commits the regenerated lockfile as a separate commit before the trunk gate.
- After Task 17A lands (frontend toolchain bootstrap), the trunk gate also runs `pnpm install --frozen-lockfile` inside `crates/memoryd-web/frontend/`. If `--frozen-lockfile` fails because `package.json` changed without a matching `pnpm-lock.yaml` update at integration, orchestrator runs `pnpm install --no-frozen-lockfile` on `main` and commits the regenerated lockfile as `chore(lockfile): reconcile pnpm-lock.yaml after task-17X integration` before the trunk gate. If `pnpm install` itself fails, orchestrator reverts the offending task's integration commit and marks the task `Blocked` with `task-17X | <slug> | pnpm-lockfile-resolve-fail`.

### Helper scripts — DO NOT use as-is

`scripts/spawn-task-worktree.sh` and `scripts/integrate-task-worktree.sh` were authored for the Stream A buildout. They hardcode the `stream-a/` branch prefix and the integrate helper's "narrow" gate runs `cargo test --workspace`, which CLAUDE.md forbids inside task worktrees (stub modules from unstarted tasks will fail for the wrong reason). The orchestrator drives worktrees directly:

- **Spawn:** `git worktree add -b dogfood/task-NN-<slug> ../agent-memory-wt/task-NN main`
- **Per-task gate:** run the exact command from the task's `Per-task gate:` line **inside the task worktree** (the worker does this, not the orchestrator)
- **Integrate:** on `main`: `git merge --ff-only dogfood/task-NN-<slug>` → `git worktree remove ../agent-memory-wt/task-NN` → `git branch -d dogfood/task-NN-<slug>`
- **Trunk gate:** `bash scripts/check.sh` on `main` once per phase batch (not per task), in the policy described under "Failure-mode policy"

---

## Inter-task coordination

This is a single-stream plan, so there is no cross-stream sequencing. Within the plan, file-level coordination matters in two clusters:

**Cluster A — `crates/memoryd/src/handlers.rs` (post-Task-9: `crates/memoryd/src/handlers/mod.rs`)** is touched by Tasks 2, 9, 11A, 19, 20, 21, 22, 23, 24, 25, and **28** (v0.8: Task 28's `ReviewQueueOverThreshold` emit moved here from `memory-governance` to keep governance pure). Task 9 performs the structural conversion `handlers.rs` → `handlers/mod.rs` and Task 11A then adds the new TUI-facing protocol handlers; both must land before Phase 6 starts. Sequential: **2 → 9 → 11A → 19 → 20 → 21 → 22 → 23 → 24 → 25 → 28**, each on its own worktree, integrated to `main` before the next task spawns. The orchestrator MUST NOT fan these out in parallel.

**Cluster B — `crates/memoryd/src/main.rs`** is touched by Tasks 1, 3, 4, 5, and 14. Task 1 (added in v0.5) calls `memory_privacy::install_runtime_enforcement(...)` once at serve startup so the runtime privacy flag is actually wired into the classifier call sites; this is a five-line addition that does not collide with Tasks 3-14's main.rs work but sequences first per foundation order. Sequential: **1 → 3 → 4 → 5 → 14**, integrated between each.

**Cluster C — `crates/memoryd-tui/src/app.rs`** is touched by Tasks 11, 11B, 12, 13, and 14B. Sequential **11 → 11B → 12 → 13 → 14B**. **Task 10A precedes Cluster C as a non-collision blocker** — it lives entirely inside the new `crates/memorum-theme/` crate and does not touch any existing memoryd-tui files; Task 11 is the seam that wires the theme crate into the TUI shell, so 10A must land before 11 spawns. Task 10A may run in parallel with any non-Cluster-C task once Phase 3 closes.

**Cluster D-backend — `crates/memoryd-web/src/`** Tasks 15 and 16 touch sibling files under `routes/` and share `crates/memoryd-web/src/server.rs` for route registration: Task 16 ships first (registers its three routes), Task 15 rebases (registers entity routes on top). Tasks 15 and 16 do not touch the new `frontend/` subtree.

**Cluster D-frontend — `crates/memoryd-web/frontend/`** Tasks 17A through 17K live entirely inside the new `crates/memoryd-web/frontend/` subdirectory (plus `crates/memoryd-web/build.rs` and `Cargo.toml` rust-embed retarget in 17A only). Sequencing:
- **17A first** (foundation: pnpm + Vite + React + TypeScript scaffold, build.rs, rust-embed retarget, removes the old `static/`).
- **17B + 17C + 17D parallel-safe** after 17A (different files: tokens/styles, shell+primitives+icons, Inspector composition).
- **17E → 17F → 17G → 17H sequential** through `frontend/src/views.ts` (each view port adds its registration to the same view-router file; sequential to avoid trivial merge conflicts on a tiny coordination file). Each view's body lives in its own `frontend/src/views/<view>.tsx` and may be authored in parallel; the integration step is the views.ts registration, which sequences.
- **17I → 17J → 17K sequential** (real-data wiring → settings/keyboard/palette → integration validation).

Cluster D-backend and Cluster D-frontend are **independent** (different file subtrees); their tasks may interleave freely. **Task 18 (source-capture URL redaction) is in `crates/memory-source/` and parallels both subclusters freely.**

Everything else is parallel-safe within its phase.

---

## Phase 0 — Foundation: privacy flag, governance defaults, safe durability

These three tasks are sequential blockers for everything else. They change the runtime contract that every other task assumes.

---

### Task 1: Runtime privacy enforcement switches

**Parallel:** no
**Blocked by:** none
**Owned files:** `crates/memory-substrate/src/config/privacy.rs`, `crates/memory-substrate/src/config/mod.rs`, `crates/memory-privacy/src/policy.rs`, `crates/memory-privacy/src/classifier.rs`, `crates/memory-privacy/src/secret_only_scan.rs`, `crates/memory-privacy/src/regex.rs`, `crates/memory-privacy/src/lib.rs`, `crates/memory-privacy/tests/runtime_switches.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/tests/privacy_runtime_install_classifier_off.rs`, `crates/memoryd/tests/privacy_runtime_install_full.rs`, `crates/memoryd/tests/privacy_runtime_install_double.rs`, `docs/api/stream-d-privacy-api.md`
**Subagent:** `backend_arch`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memory-privacy --tests && cargo test -p memory-substrate --test config_privacy && cargo test -p memoryd --test privacy_runtime_install_classifier_off && cargo test -p memoryd --test privacy_runtime_install_full && cargo test -p memoryd --test privacy_runtime_install_double && cargo clippy -p memory-privacy -p memory-substrate -p memoryd --tests -- -D warnings && cargo fmt -p memory-privacy -p memory-substrate -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-01/` on `dogfood/task-01-privacy-runtime-flag`

**Stream A surface touch authorized:** Adds `PrivacyEnforcement` config struct under `crates/memory-substrate/src/config/`. Spec amendment: the privacy *classifier* is switchable at runtime; the secret-refusal *invariant* is preserved by an always-on `SecretOnlyScan` layer that runs even when the full classifier is disabled.

**Architecture — two-layer classification:** Today `DeterministicPrivacyClassifier::classify()` runs `regex_spans` + entropy + label-mapping in a single pass, and `secret` is one of the labels it can emit. Task 1 splits this:

1. **`SecretOnlyScan`** (always on) — runs the regex subset that maps to `PrivacyLabel::Secret`: AWS keys (`AKIA...`), GitHub tokens (`gh[pousr]_...`), Stripe tokens (`sk_live`/`sk_test`), PEM private-key headers, JWT triple-dot, SSN pattern (`\b\d{3}-\d{2}-\d{4}\b`); **plus** the `credit_card_spans` Luhn-validating path currently in `regex.rs` (separate from `RULES`); **plus** the entropy-based `looks_credential_like` check from `entropy.rs`. The Luhn path moves into `SecretOnlyScan` — it does not stay in `FullClassifier` only — because a Luhn-valid 16-digit sequence with `enforcement.classifier: false` would otherwise bypass the secret invariant. On hit, emits `ClassificationOutcome::Secret -> PrivacyStorageAction::Refuse`. This preserves spec invariant #1 regardless of `enforcement.classifier`.
2. **`FullClassifier`** (gated by `enforcement.classifier`) — runs the rest of the regex set plus entropy plus label policy. When disabled, returns `ClassificationOutcome::Trusted` with `PrivacyLabel::None` and `PrivacyStorageAction::Plaintext`.

`DeterministicPrivacyClassifier::classify()` becomes: `SecretOnlyScan` first; on hit, return Secret; else dispatch to `FullClassifier` if enabled, or short-circuit to Trusted/Plaintext if disabled. The substrate write gate at `crates/memory-substrate/src/api.rs:486-494` and the handler refusal checks at `handlers.rs:1593` and `handlers.rs:1639` continue to read `outcome.storage_action` — they don't need to know which layer produced the decision.

**Wiring the runtime flag — fix from v0.5 review.** Three call sites in `crates/memoryd/src/handlers.rs` (`1828`, `2413`, `3470`) construct the classifier inline with `DeterministicPrivacyClassifier::new()` — fresh per-call, no AppState handle. v0.4 added `with_enforcement` but never threaded it. v0.5 parked enforcement in a process-wide `OnceLock<PrivacyEnforcement>` inside `crates/memory-privacy/src/policy.rs` (`static RUNTIME_ENFORCEMENT: OnceLock<PrivacyEnforcement>`). `DeterministicPrivacyClassifier::new()` reads from `RUNTIME_ENFORCEMENT.get().copied().unwrap_or_else(PrivacyEnforcement::paranoid)` so callers that never installed an override get the safe default. `memoryd::main` calls `memory_privacy::install_runtime_enforcement(...)` exactly once at serve startup before any handler can run; double-install is rejected (returned error) so tests can detect misuse. All three handler call sites stay as `::new()` and inherit the runtime flag without a structural refactor.

**Config-shape fix (v0.6).** v0.5 said "add `pub privacy: PrivacyEnforcement` to root `Config`". The substrate has no root `Config` struct — actual shapes are `SyncedConfig` (synced via git, line 18 of `config/mod.rs`), `LocalDeviceConfig` (per-device, never synced, line 47), and `LoadedConfig` (resolved precedence, line 70). Privacy enforcement is per-device runtime (Trey's "off for dogfood" must not propagate to peers via git), so it belongs in `LocalDeviceConfig` — same justification as the existing per-device device-id field, which spec invariant #4 forbids syncing. v0.6 fix: add `pub privacy: PrivacyEnforcement` (with `#[serde(default)]` so existing local configs deserialize cleanly) to `LocalDeviceConfig`, and expose `LoadedConfig::privacy_enforcement()` returning `self.local.as_ref().map(|l| l.privacy).unwrap_or_default()` (paranoid fallback if local config absent). main.rs reads `loaded.privacy_enforcement()` and installs.

**OnceLock test isolation (v0.6).** v0.5 used a single `tests/privacy_runtime_install.rs` with `#[serial_test::serial(...)]`. That doesn't work — `#[serial]` orders tests within a binary, but `OnceLock` is process-global, so once the first test installs, the second test's `install_runtime_enforcement(...)` always errors. The "double-install rejection" test would always pass for the wrong reason and the install-success tests after it would always observe the first install's enforcement. Fix: split into **three separate test binaries** (`tests/privacy_runtime_install_*.rs` — Cargo runs each `tests/<name>.rs` as its own process, so each gets a fresh `OnceLock`). Test `with_enforcement` (which bypasses the `OnceLock`) stays in the `runtime_switches.rs` binary alongside the rest of the matrix.

**Files:**
- Create: `crates/memory-substrate/src/config/privacy.rs` — defines `PrivacyEnforcement { classifier: bool, encryption: bool, masking: bool }` with `Default = { classifier: false, encryption: false, masking: false }` for dogfood profile; `from_yaml`, `from_env` (`MEMORUM_PRIVACY_CLASSIFIER=on|off` etc.), `validate`. The `paranoid()` constructor returns all-true for the eventual ship default.
- Modify: `crates/memory-substrate/src/config/mod.rs` — add `pub privacy: PrivacyEnforcement` field to `LocalDeviceConfig` (per-device, never synced; spec invariant #4 alignment), with `#[serde(default)]` so existing local config files deserialize without the field. Add `impl LoadedConfig { pub fn privacy_enforcement(&self) -> PrivacyEnforcement { self.local.as_ref().map(|l| l.privacy).unwrap_or_default() } }`. Do NOT add the field to `SyncedConfig` — that would cross-pollute peers.
- Create: `crates/memory-privacy/src/secret_only_scan.rs` — extracted secret-pattern subset of `regex_spans` plus the `looks_credential_like` entropy check. Returns `Option<SecretFinding>` for the smallest-possible always-on layer. Tested independently.
- Modify: `crates/memory-privacy/src/regex.rs` — refactor: split the regex set into `SECRET_PATTERNS` (SSN, Luhn-valid card, credential-entropy) and `LABEL_PATTERNS` (everything else: email, phone, address, person, account, URL, date). `SecretOnlyScan` consumes the former; `FullClassifier` consumes both.
- Modify: `crates/memory-privacy/src/policy.rs` — add `static RUNTIME_ENFORCEMENT: OnceLock<PrivacyEnforcement>` plus `pub fn install_runtime_enforcement(enforcement: PrivacyEnforcement) -> Result<(), AlreadyInstalled>` and `pub(crate) fn current_enforcement() -> PrivacyEnforcement` (paranoid fallback when not installed). `DeterministicPrivacyClassifier::classify()` runs the two-layer flow described above and reads `current_enforcement()` instead of taking an explicit parameter, so existing call sites (`::new().classify(...)`) continue to compile and inherit the runtime flag.
- Modify: `crates/memory-privacy/src/classifier.rs` — keep `Classifier::new()` returning a value that consults `current_enforcement()`; add `Classifier::with_enforcement(...)` for tests/fixtures that want to bypass the OnceLock without poisoning it.
- Modify: `crates/memory-privacy/src/lib.rs` — re-export `PrivacyEnforcement`, `SecretOnlyScan`, `install_runtime_enforcement`.
- Modify: `crates/memoryd/src/main.rs` — in the `Command::Serve(args)` branch (around line 458 where `memory_substrate::config::load_config(&args.repo, &args.runtime, None)` is already called), after the `LoadedConfig` is obtained but before any worker is spawned, call `memory_privacy::install_runtime_enforcement(loaded.privacy_enforcement())`. Log the active flag set at `tracing::info!` level so dogfood operators see "privacy enforcement: classifier=off encryption=off masking=off" in the daemon stderr at startup. The install call returns `Err(AlreadyInstalled)` on second install — main logs a `tracing::warn!` and continues (does not panic; allows in-process restart paths and tests-that-fork to behave gracefully).
- Test: `crates/memory-privacy/tests/runtime_switches.rs` — eight tests: classifier-off lets emails through plaintext; classifier-off lets Luhn-passing benign sequences (docker digests etc.) through plaintext IF they don't match the secret-only scan; **classifier-off but valid SSN content still refuses** (spec invariant #1); **classifier-off but Luhn-passing 16-digit pattern with card-context still refuses** (spec invariant #1); classifier-on still refuses SSN; classifier-on email gets `EncryptAtRest`; encryption-off routes `EncryptAtRest` labels to plaintext (encryption is the action, not the detection); masking-off bypasses `MaskingSession`. All tests use `Classifier::with_enforcement(...)` to bypass the OnceLock so they can run in parallel without ordering coupling.
- Test: `crates/memoryd/tests/privacy_runtime_install_classifier_off.rs` — single-binary test: load a `LoadedConfig` whose `LocalDeviceConfig.privacy.classifier = false`, call `install_runtime_enforcement(loaded.privacy_enforcement())`, then construct a fresh `DeterministicPrivacyClassifier::new()` and classify a plaintext email — expected `Trusted/Plaintext` (passes through). Each `tests/<name>.rs` is its own test binary with its own process and fresh `OnceLock`.
- Test: `crates/memoryd/tests/privacy_runtime_install_full.rs` — single-binary test: install with `classifier: true, encryption: true`, classify an email — expected `EncryptAtRest`; classify a literal SSN — expected `Refuse`.
- Test: `crates/memoryd/tests/privacy_runtime_install_double.rs` — single-binary test: install once (success), install again with the same enforcement — expected `Err(AlreadyInstalled)`; install a third time with different enforcement — still `Err(AlreadyInstalled)`; verify the first install's enforcement is what `Classifier::new()` observes.
- Modify: `docs/api/stream-d-privacy-api.md` — append "Runtime enforcement switches" subsection documenting the three flags, env vars, defaults, the `install_runtime_enforcement` lifecycle (install once at serve start, paranoid fallback if uninstalled), and the invariant note.

**Step 1: Write the failing tests** (eleven tests across both files; expect compile failure on `PrivacyEnforcement` and `install_runtime_enforcement` unknown).

**Step 2: Run per-task gate to verify fail.**
Expected: tests fail compile.

**Step 3: Implement `PrivacyEnforcement` struct + `RUNTIME_ENFORCEMENT` OnceLock + `install_runtime_enforcement` + main.rs install call + two-layer classifier.**

**Step 4: Re-run per-task gate.**
Expected: all green; no regressions in existing `memory-privacy` test suite; `cargo clippy --tests -- -D warnings` clean; `cargo fmt -- --check` clean.

**Step 5: Commit.**
```bash
git add crates/memory-substrate/src/config/privacy.rs crates/memory-substrate/src/config/mod.rs crates/memory-privacy/src/{policy,classifier,secret_only_scan,regex,lib}.rs crates/memory-privacy/tests/runtime_switches.rs crates/memoryd/src/main.rs crates/memoryd/tests/privacy_runtime_install_classifier_off.rs crates/memoryd/tests/privacy_runtime_install_full.rs crates/memoryd/tests/privacy_runtime_install_double.rs docs/api/stream-d-privacy-api.md
git commit -m "feat(privacy): runtime enforcement switches wired through OnceLock to call sites"
```

**Step 6: Hand back to orchestrator.**

---

### Task 2: Governance defaults flip for human-driven writes

**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memoryd/src/handlers.rs` (governance-default sections at lines 2879-3020 — the private `GovernanceMeta` struct, its `Default` impl, and the `GovernanceWriteInput::parse` call site), `crates/memory-governance/src/policy.rs`, `crates/memory-governance/tests/dogfood_defaults.rs`, `crates/memoryd/tests/handler_contract.rs` (governance-defaults section)
**Subagent:** `backend_arch`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memory-governance --tests && cargo test -p memoryd --test handler_contract && cargo clippy -p memory-governance -p memoryd --tests -- -D warnings && cargo fmt -p memory-governance -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-02/` on `dogfood/task-02-governance-defaults`

**Live-code check (v0.5 fix).** The v0.4 plan referenced a helper `write_governance_meta()` that does not exist in the repo. The actual surface is the private `GovernanceMeta` struct in `crates/memoryd/src/handlers.rs:2879` with a `Default` impl at `2965` and a single call to `GovernanceMeta::default()` at `3020` inside `GovernanceWriteInput::parse`. Dream/observe paths construct `GovernanceMeta` directly (not via `parse`), so changing `Default` would leak relaxed defaults into programmatic paths. v0.5 fix: introduce a path-specific constructor `GovernanceMeta::for_mcp_human_write()` and call it from `GovernanceWriteInput::parse` only when the parse is invoked from `memory_write` or `memory_note` MCP dispatch (passed as a `MetaSource::McpHumanWrite` enum argument). `Default` stays strict; dream and observe call sites are unchanged.

**Files:**
- Modify: `crates/memoryd/src/handlers.rs:2879-3020` — add `pub(super) enum MetaSource { Default, McpHumanWrite }` (or equivalent) and `impl GovernanceMeta { pub(super) fn for_mcp_human_write() -> Self }` returning `{ explicit_user_context: true, confidence: 0.9, ..Self::default() }`. Modify `GovernanceWriteInput::parse(body, title, tags, meta, source: MetaSource)` to pick the right constructor when `meta.is_null()`. Update the two `parse` call sites (in `memory_write` and `memory_note` dispatch — locate via `rg -n 'GovernanceWriteInput::parse' crates/memoryd/src/handlers.rs`) to pass `MetaSource::McpHumanWrite`; any other call sites pass `MetaSource::Default`. Programmatic dream/observe paths that construct `GovernanceMeta` directly are not touched.
- Modify: `crates/memory-governance/src/policy.rs` — built-in `me-strict` policy: lower `confidence_floor` from 0.9 → 0.85 to match the new MCP-human default. Document the change in the policy file docstring with a `// 2026-05-07: lowered for dogfood profile, see Task 2` comment.
- Test: `crates/memory-governance/tests/dogfood_defaults.rs` — **policy-engine scope only**: verify the lowered `me-strict.confidence_floor: 0.85` accepts a `(confidence: 0.85, grounding: satisfied)` write as `Active`; verify it still refuses `confidence: 0.80`; verify the strict-path policy unchanged for non-`me-strict` policies. **No MCP/handler tests in this crate** — `memory-governance` does not depend on `memoryd` and must not test MCP behavior.
- Test: `crates/memoryd/tests/handler_contract.rs` — **MCP-write behavior tests live here** (the daemon owns MCP). Verify that `RequestPayload::WriteMemory { meta: null }` from a MCP-human source dispatches with `MetaSource::McpHumanWrite` and the resulting `GovernanceMeta` has `explicit_user_context: true, confidence: 0.9`; verify `MetaSource::Default` (programmatic / dream / observe paths) produces strict defaults; verify a MCP `memory_write` to `me/knowledge/` with no explicit confidence and no sources lands `Active` end-to-end; verify dream-scope writes still hit the strict path; verify SSN-bearing content still refuses regardless of relaxed defaults (Task 1's `SecretOnlyScan` runs before governance).

**Steps 1-6:** same TDD-then-impl-then-commit pattern.

```bash
git commit -m "feat(governance): relaxed defaults for human-driven MCP writes"
```

---

### Task 3: `serve --init` safe durability default

**Parallel:** no
**Blocked by:** Task 2
**Owned files:** `crates/memoryd/src/main.rs` (init-flag handling), `crates/memoryd/src/cli.rs` (init flag schema), `crates/memoryd/tests/serve_durability.rs`
**Subagent:** `cli_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test serve_durability && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-03/` on `dogfood/task-03-safe-durability`

**Files:**
- Modify: `crates/memoryd/src/main.rs:33-38` — `serve --init` no longer hardcodes `force_unsafe_durability: true`. Default is `false` (safe). Add explicit `--force-unsafe-durability` CLI flag for opt-in (CI/test only); the flag emits a `tracing::warn!` at startup naming the operator and reason.
- Modify: `crates/memoryd/src/cli.rs` — add `force_unsafe_durability: bool` to the `Serve` subcommand args, default false.
- Test: `crates/memoryd/tests/serve_durability.rs` — verify `serve --init` without the flag opens the substrate with safe durability; verify `serve --init --force-unsafe-durability` opens with unsafe and emits the warn.

```bash
git commit -m "fix(daemon): default to safe durability on serve --init"
```

---

## Phase 1 — MCP / socket / lifecycle (foundation 2)

These three are sequential within Cluster B (`main.rs`); Task 6 may parallel any of them since it lives in `crates/memory-substrate/src/api.rs` + `git/adopt.rs`.

---

### Task 4: MCP bridge owns daemon lifecycle

**Parallel:** no
**Blocked by:** Task 3
**Owned files:** `crates/memoryd/src/cli.rs`, `crates/memoryd/src/main.rs`, `crates/memoryd/src/mcp_stdio.rs`, `crates/memoryd/src/mcp.rs`, `crates/memoryd/src/socket.rs`, `crates/memoryd/tests/mcp_lifecycle.rs`
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test mcp_lifecycle -- --skip auto_start_with_live_probe && cargo test -p memoryd --test mcp_stdio && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check` *(probe-dependent test paths land in Task 5 once `socket::probe_live_socket` returns real status; Task 4 gates only the structural-error and stub-signature tests)*
**Worktree:** `../agent-memory-wt/task-04/` on `dogfood/task-04-mcp-owns-daemon`

**System spec note:** `system-v0.2.md` §14.1 freezes the v1 MCP contract at the currently-shipped tool count. Adding a new MCP tool would require a v2 spec bump, which is out of scope. Therefore `memory_status` is a **daemon-protocol command exposed via the socket**, not an MCP-manifest tool. Agents that need health visibility get it via the auto-start path's structured error or via the daemon's existing socket-level `RequestPayload::Status`. Human-facing health is `memoryd doctor` (Task 9) and the TUI overview / web dashboard (Tasks 11, 17).

**Socket module ownership (v0.5 fix).** v0.4 said Task 4 "stubs the function signature, Task 5 lands the impl" but did not list `crates/memoryd/src/socket.rs` in Task 4's Owned files. v0.5 fix: Task 4 **creates** the module with stub returning `Absent`/computing a runtime path; Task 5 then **modifies** the same file with the real probe (JSON-RPC ping with 1 s timeout) plus owner-only chmod. The stub keeps Task 4 compilable and gates the auto-start logic correctly (an `Absent` probe always triggers spawn).

**McpArgs ownership (v0.6 fix).** v0.5 had Task 4 modifying `main.rs:51-52` to add `--auto-start` and to spawn the daemon with `--repo`/`--runtime`, but the args struct lives in `cli.rs` (`Mcp(SocketArgs)` at line 76 takes only `socket: PathBuf`). The `mcp` subcommand needs `repo`, `runtime`, and `auto_start` to spawn `memoryd serve` correctly. v0.6 fix: Task 4 owns `cli.rs` and replaces `Mcp(SocketArgs)` with a new `Mcp(McpArgs)` struct.

**Files:**
- Create: `crates/memoryd/src/socket.rs` — `pub enum SocketProbe { Live, Stale, Absent }`, `pub fn probe_live_socket(_path: &Path) -> SocketProbe { SocketProbe::Absent }` (stub; Task 5 fills in the JSON-RPC ping), `pub fn resolve_socket_path(runtime: &Path) -> PathBuf` returning `runtime.join("memoryd.sock")`. Module wired into `lib.rs` so other binaries can consume it.
- Modify: `crates/memoryd/src/cli.rs` — replace `Mcp(SocketArgs)` (line 76) with `Mcp(McpArgs)`. New struct: `pub struct McpArgs { pub socket: Option<PathBuf>, pub repo: PathBuf, pub runtime: PathBuf, pub auto_start: bool }`. `socket` is optional — main.rs resolves via `socket::resolve_socket_path(&runtime)` when not set (Task 5 will broaden this resolution to other Args structs that share the same socket field). `repo`/`runtime` mirror `ServeArgs`'s defaults so a single `claude mcp add` line works without extra flags. `auto_start` defaults `true`.
- Modify: `crates/memoryd/src/main.rs` — in the `Command::Mcp(args)` branch, resolve `socket = args.socket.clone().unwrap_or_else(|| memoryd::socket::resolve_socket_path(&args.runtime))`. Bridge then either (a) probes the socket via `socket::probe_live_socket`, starts the daemon if absent, or (b) fails fast with a structured `daemon_not_running` JSON-RPC error if `args.auto_start: false`. On `auto_start: true` with `Absent` probe: spawn `memoryd serve --repo {args.repo} --runtime {args.runtime}` as a child, wait for readiness (10 s timeout), then proceed to stdio bridge.
- Modify: `crates/memoryd/src/mcp_stdio.rs` — on first incoming `tools/call`, ensure the socket is live (re-probe); if not, return a JSON-RPC error pointing the operator at `memoryd doctor`. Do **not** add a `memory_status` MCP tool. The existing 10-tool manifest stays as-is.
- Modify: `crates/memoryd/src/mcp.rs` — no manifest changes (10 tools); only sanity-check that `ToolName::all()` returns the expected count via a debug assertion in tests, no production change.
- Test: `crates/memoryd/tests/mcp_lifecycle.rs` — three tests: bridge auto-starts daemon when socket absent (uses Task 4 stub returning `Absent`, so spawn fires); bridge fails fast on `--auto-start false` with no daemon (returns `daemon_not_running`); `tools/call` after a simulated socket loss returns a structured error not a panic.

```bash
git commit -m "feat(mcp): bridge owns daemon lifecycle + auto-start + fail-fast on dead daemon"
```

---

### Task 5: Per-runtime private socket + canonical resolver

**Parallel:** no
**Blocked by:** Task 4
**Owned files:** `crates/memoryd/src/main.rs`, `crates/memoryd/src/cli.rs`, `crates/memoryd/src/server.rs`, `crates/memoryd/src/socket.rs`, `crates/memoryd/tests/socket_resolver.rs`
**Subagent:** `cli_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test socket_resolver && cargo test -p memoryd --test mcp_lifecycle && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-05/` on `dogfood/task-05-private-socket`

**Socket-defaults breadth (v0.6 fix).** v0.5 said "modifies `cli.rs:63-67` (default socket)" — but `crates/memoryd/src/cli.rs` actually contains **12+ separate `default_value = "/tmp/memoryd.sock"` annotations** across `SocketArgs`, `UiArgs`, and the various subcommand Args structs (search, get, status, recall hooks, supersede, forget, write, write-note, peer admin, reality-check, etc. — `rg -n 'default_value = "/tmp/memoryd.sock"' crates/memoryd/src/cli.rs` enumerates them). Updating only one is the bug Codex caught. v0.6 fix: replace the static default everywhere — make `socket: Option<PathBuf>` (no `default_value`) on every Args struct, then resolve in main.rs at dispatch via `args.socket.clone().unwrap_or_else(|| memoryd::socket::resolve_socket_path(&runtime_root))`. The `runtime_root` for connect-only subcommands (search/get/status/etc. that don't take `--runtime`) is read from `MEMORUM_RUNTIME` env or falls back to `~/.local/share/memorum/runtime/`.

**Files:**
- Modify: `crates/memoryd/src/socket.rs` (Task 4 created the stub; this task replaces the stub body) — `resolve_socket_path(runtime: &Path) -> PathBuf` returns `<runtime>/memoryd.sock` (was `/tmp/memoryd.sock`). `probe_live_socket(path: &Path) -> SocketProbe { Live, Stale, Absent }` does an actual JSON-RPC `status` ping with 1s timeout to distinguish live from stale. Owner-only chmod (700 on parent, 600 on socket) at bind time. Add `pub fn default_runtime_root() -> PathBuf` reading `MEMORUM_RUNTIME` env or falling back to `~/.local/share/memorum/runtime/` (used by connect-only subcommands that have no explicit runtime arg).
- Modify: `crates/memoryd/src/cli.rs` — replace **every** `#[arg(long, default_value = "/tmp/memoryd.sock")] pub socket: PathBuf` with `#[arg(long)] pub socket: Option<PathBuf>`. Worker enumerates exhaustively via `rg -n 'default_value = "/tmp/memoryd.sock"' crates/memoryd/src/cli.rs` (12+ sites at the time of v0.5 verification across `SocketArgs`, `UiArgs`, and the various subcommand Args structs); the rg output is the source of truth, not any partial list in this prose. **No new `default_value` is added** — main.rs is the single resolution point.
- Modify: `crates/memoryd/src/main.rs` — at every dispatch site that previously read `args.socket: PathBuf`, replace with `let socket = args.socket.clone().unwrap_or_else(|| memoryd::socket::resolve_socket_path(&runtime_root));` where `runtime_root` is `args.runtime` for subcommands that have one (Serve, Mcp, recall hooks) and `memoryd::socket::default_runtime_root()` otherwise. Recall-hook fallback at line ~1073-1075 also uses the new resolver.
- Modify: `crates/memoryd/src/server.rs:354-359` — replace unconditional `remove_stale_socket` with `probe_live_socket`; if `Live`, error out with `socket_in_use` and the PID of the live process; if `Stale` or `Absent`, proceed. Owner-only chmod after bind (700 on parent, 600 on socket).
- Test: `crates/memoryd/tests/socket_resolver.rs` — verify private socket path resolves to `<runtime>/memoryd.sock`; verify probe distinguishes live/stale/absent; verify two daemons cannot bind same socket; verify every Args struct that previously had `default_value = "/tmp/memoryd.sock"` now resolves the same way (parameterized test that loops through all 12+ subcommand dispatch paths and asserts they hit `resolve_socket_path`); verify `MEMORUM_RUNTIME` env override works for connect-only subcommands.

```bash
git commit -m "fix(socket): per-runtime private socket + canonical resolver"
```

---

### Task 6: Open path validates before mutating

**Parallel:** no (sequenced after Task 5 for determinism in autonomous runs)
**Blocked by:** Task 5
**Owned files:** `crates/memory-substrate/src/api.rs` (open + adopt entry points), `crates/memory-substrate/src/git/adopt.rs:85-86`, `crates/memory-substrate/tests/open_validation.rs`
**Subagent:** `backend_arch`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memory-substrate --test open_validation && cargo clippy -p memory-substrate --tests -- -D warnings && cargo fmt -p memory-substrate -- --check`
**Worktree:** `../agent-memory-wt/task-06/` on `dogfood/task-06-open-validates`

**Stream A surface touch authorized.** Adds defensive validation to `Substrate::open` and `git::adopt_clone`. Behavior change for callers: passing a directory that is not a Memorum substrate now errors with `NotAMemorumSubstrate` instead of bootstrapping it (init/adopt is now explicit-only).

**Files:**
- Modify: `crates/memory-substrate/src/api.rs` — `Substrate::open` now requires either `.memorum/` marker file or explicit `InitOptions::init_if_missing: true`. New error variant `OpenError::NotAMemorumSubstrate { path }`.
- Modify: `crates/memory-substrate/src/git/adopt.rs:85-86` — replace `current_exe()` placeholder with `merge_driver_path: PathBuf` parameter on `adopt_clone`. Callers (currently just `serve`) pass the resolved path explicitly.
- Test: `crates/memory-substrate/tests/open_validation.rs` — open on non-substrate directory errors; open on valid substrate succeeds; adopt_clone with explicit driver path succeeds; adopt_clone with missing driver errors clearly.

```bash
git commit -m "fix(substrate): open validates before mutating; adopt requires explicit driver path"
```

---

## Phase 2 — Install + lifecycle scripts

Tasks 7, 8 are independent (different scripts) and **may parallel**.

---

### Task 7: install-memorum.sh hardening

**Parallel:** yes
**Blocked by:** Task 5 (uses new socket resolver)
**Owned files:** `scripts/install-memorum.sh`, `scripts/install-memorum.test.sh`
**Subagent:** `cli_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`
**Per-task gate:** `bash scripts/install-memorum.test.sh` *(this task creates the gate script — the gate runs only after the script lands; on the failing-test step, the worker hand-runs the install script in a temp dir to confirm reds, then writes the script + impl together)*
**Worktree:** `../agent-memory-wt/task-07/` on `dogfood/task-07-installer-hardening`

**Files:**
- Modify: `scripts/install-memorum.sh:77` — `memoryd_expected_version()` resolves `crates/memoryd/Cargo.toml` via `$(dirname "$0")/../crates/...` not relative path. Hard-error if Cargo.toml not found.
- Modify: `scripts/install-memorum.sh:140-144` — install all day-one binaries: `memoryd`, `memoryd-tui`, `memoryd-web`, `memoryd-merge-driver`. Verify each exists post-install or fail.
- Modify: `scripts/install-memorum.sh:203` — readiness loop extends to 30s on first-run (when `--init` is involved), 10s otherwise. Detect first-run by absence of substrate marker.
- Modify: `scripts/install-memorum.sh:181` — escalate to SIGKILL after 5s of unresponsive SIGTERM; warn loudly.
- Modify: `scripts/install-memorum.sh:266-274` — pass through PATH including `~/.cargo/bin` to harness CLI detection so daemon's inherited env can find `claude`/`codex`.
- Modify: `scripts/install-memorum.sh:248-253` — fix the launchd auto-restart messaging: separate daemon and dream-scheduler descriptions; only claim auto-restart for the binary that actually has a `KeepAlive` plist.
- Modify: `scripts/install-memorum.sh` — emit MCP wiring snippet using **new private socket path** (`<runtime>/memoryd.sock`), not `/tmp/memoryd.sock`. Include both `claude mcp add` one-liner AND raw JSON block.
- Modify: PID write moves to BEFORE readiness check (kill orphan risk).
- Create: `scripts/install-memorum.test.sh` — end-to-end smoke: install in temp `--repo` and `--runtime` dirs, verify daemon responds, verify all binaries present, verify socket path is private, clean up.

```bash
git commit -m "fix(install): CWD bug, all binaries, readiness, PID race, socket path, PATH"
```

---

### Task 8: launchd plists — daemon LaunchAgent + dream PATH

**Parallel:** yes
**Blocked by:** Task 5 (uses new socket resolver)
**Owned files:** `scripts/install-launchd.sh`, `scripts/templates/com.memorum.daemon.plist.template` (new), `scripts/templates/com.memorum.dream-scheduled.plist.template`, `scripts/install-launchd.test.sh`
**Subagent:** `cli_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `clean-code`
**Per-task gate:** `bash scripts/install-launchd.test.sh`
**Worktree:** `../agent-memory-wt/task-08/` on `dogfood/task-08-launchd-daemon`

**Files:**
- Create: `scripts/templates/com.memorum.daemon.plist.template` — `RunAtLoad: true`, `KeepAlive: { SuccessfulExit: false, NetworkState: false }`, `EnvironmentVariables.PATH` includes `~/.cargo/bin`. `ProgramArguments` invokes `memoryd serve --repo {{REPO_PATH}} --runtime {{RUNTIME_PATH}} --socket {{RUNTIME_PATH}}/memoryd.sock` — **the `--repo` arg is required** because `memoryd serve` cannot locate the substrate without it, and the existing dream-scheduled plist already passes `--repo {{REPO_PATH}}` (see `scripts/templates/com.memorum.dream-scheduled.plist.template`); the daemon plist must match. `{{REPO_PATH}}`/`{{RUNTIME_PATH}}` are placeholders that `scripts/install-launchd.sh` substitutes at install time. Stdout/stderr to `{{RUNTIME_PATH}}/daemon.{out,err}.log` (parallel to the dream plist's logging convention; do not hardcode `~/Library/Logs/`).
- Modify: `scripts/templates/com.memorum.dream-scheduled.plist.template` — add `EnvironmentVariables.PATH` block that includes `~/.cargo/bin`. Drop deprecated load/unload patterns where present.
- Modify: `scripts/install-launchd.sh` — install BOTH plists (daemon + dream scheduler) and `launchctl bootstrap` them in normal use. **Sandbox-safe parameterization (v0.9 add):**
  - Honor env var `MEMORUM_LAUNCHAGENTS_DIR` — when set, write plist files under that directory instead of `~/Library/LaunchAgents/`. Default unchanged.
  - Honor env var `MEMORUM_LAUNCHD_INSTALL_ONLY=1` — when set, skip the `launchctl bootstrap` calls entirely (write the plist files, then exit). Used by the test harness; never set in normal install paths.
  - Use `bootstrap`/`bootout` (not deprecated `load`/`unload`).
  - Document in `--help` that the daemon LaunchAgent restarts automatically and the dream LaunchAgent fires per its `StartCalendarInterval`.
- Create: `scripts/install-launchd.test.sh` — runs the installer with `MEMORUM_LAUNCHAGENTS_DIR=$(mktemp -d)` and `MEMORUM_LAUNCHD_INSTALL_ONLY=1` so the test stays inside the workspace-write sandbox: zero writes to `~/Library/LaunchAgents/`, zero `launchctl` invocations. The test asserts plist file presence, valid XML structure (`plutil -lint`), and the exact `ProgramArguments` array — for the daemon: `["memoryd", "serve", "--repo", "<repo>", "--runtime", "<runtime>", "--socket", "<runtime>/memoryd.sock"]`; for the dream-scheduled plist: `["memoryd", "dream", "scheduled", "--repo", "<repo>", "--runtime", "<runtime>", "--scope", "me"]`. Any drift (missing `--repo`, mis-ordered flags, hardcoded paths instead of substituted placeholders) fails the test loudly. **Crucially the test never calls `launchctl bootstrap`** — production behavior is verified by code review of `install-launchd.sh`'s real bootstrap path, not by the test.

```bash
git commit -m "fix(launchd): daemon LaunchAgent + dream PATH + bootstrap/bootout"
```

---

## Phase 3 — Doctor + docs honesty

Both parallel-safe.

---

### Task 9: Doctor `--reindex` real implementation + drop phantom flags from runbook

**Parallel:** yes
**Blocked by:** Task 2 (Cluster A — doctor extraction touches `handlers.rs`), Task 5 (socket resolver)
**Owned files:** `crates/memoryd/src/cli.rs:177-185` (Doctor subcommand), `crates/memoryd/src/handlers.rs:1440-1495` (doctor logic — reserves Cluster A slot ahead of Tasks 19-25), `crates/memoryd/src/handlers/doctor.rs` (new, extracted), `docs/runbooks/dogfooding-day-one.md`, `crates/memoryd/tests/doctor_reindex.rs`
**Subagent:** `cli_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test doctor_reindex && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-09/` on `dogfood/task-09-doctor-real`

**Sequencing note (v0.7/v0.8 update).** Cluster A is `2 → 9 → 11A → 19 → 20 → 21 → 22 → 23 → 24 → 25 → 28` per the inter-task coordination block at the top of this plan. Task 28 lands in Cluster A because v0.8 moved the `ReviewQueueOverThreshold` notification emit out of `memory-governance` (which would have inverted the dep — governance must not import memoryd's `NotificationEvent`) into memoryd's review-queue handler call site in `crates/memoryd/src/handlers/mod.rs`. Task 9's structural conversion (`handlers.rs` → `handlers/mod.rs`) MUST land before any Cluster A task numbered 11A or higher. The `Parallel: yes` field above means "parallel with non-Cluster-A tasks in this phase" — within Cluster A, Task 9 is strictly serial after Task 2 and strictly before Task 11A.

**Structural note — `handlers.rs` is a flat 3,800-line file today.** Extracting `doctor` into its own file requires converting the flat file into a module directory. The exact steps:

1. `git mv crates/memoryd/src/handlers.rs crates/memoryd/src/handlers/mod.rs` — preserves history.
2. In `mod.rs`, move the doctor block (`handle_doctor`, `doctor_is_healthy`, `doctor_check_*` helpers, ~50 lines around `mod.rs:1440-1495`) into the new file `handlers/doctor.rs`.
3. In `mod.rs`, add `pub mod doctor;` near the top and `pub use doctor::{handle_doctor, doctor_is_healthy, doctor_check_*};` near the existing public re-exports so all callers (`server.rs`, `mcp_stdio.rs`, etc.) keep their import paths working.
4. Verify `lib.rs` / `main.rs` `mod handlers;` declaration still resolves (it does — Rust treats `handlers/mod.rs` and `handlers.rs` identically for the `mod handlers;` line).
5. `cargo build -p memoryd` compiles green before any other change in this task.

This module conversion is **non-negotiable**: creating `handlers/doctor.rs` alongside the existing flat `handlers.rs` is invalid Rust (the compiler will reject the dual-form). Subsequent Cluster A tasks (19–25) edit `handlers/mod.rs` rather than `handlers.rs`; their owned-files paths need the same trailing `/mod.rs` once Task 9 lands.

**Files:**
- Move: `crates/memoryd/src/handlers.rs` → `crates/memoryd/src/handlers/mod.rs` (Step 1 above).
- Create: `crates/memoryd/src/handlers/doctor.rs` — extracted `handle_doctor`, `doctor_is_healthy`, `doctor_check_*` helpers.
- Modify: `crates/memoryd/src/handlers/mod.rs` — `pub mod doctor;` + re-exports; remove the moved code.
- Modify: `crates/memoryd/src/cli.rs:177-185` — Doctor subcommand adds `--reindex` and `--socket <path>` flags.
- Modify: `crates/memoryd/src/handlers/doctor.rs` — `--reindex` runs `Substrate::rebuild_indexes()`; reports rebuilt counts; TODO-comment the future `EventKind::IndexesRebuilt` event hookup.
- Modify: `docs/runbooks/dogfooding-day-one.md` — fix all `cargo run -p memoryd --` → `cargo run --bin memoryd --`; correct `memoryd doctor --socket` example (now honored); document `--reindex`.
- Test: `crates/memoryd/tests/doctor_reindex.rs` — verify `--reindex` actually rebuilds and reports; verify `--socket <custom>` resolves; verify the diagnostic hint suggesting `--reindex` matches the implemented flag.

**Cluster A path note for Tasks 19–25:** after Task 9 integrates, the Cluster A owned-files for subsequent MCP tool surface tasks become `crates/memoryd/src/handlers/mod.rs` (not `handlers.rs`). Each Cluster A task description should read its `handlers.rs` reference as `handlers/mod.rs` post-Task-9. Worker briefs include this rebase note.

```bash
git commit -m "fix(doctor): --reindex impl, --socket flag, drop phantom doc commands"
```

---

### Task 10: Docs sweep — `cargo run --bin memoryd` everywhere

**Parallel:** yes
**Blocked by:** none
**Owned files:** `docs/runbooks/*.md` (excluding `dogfooding-day-one.md` owned by Task 9), `docs/api/*.md`, `docs/dev/*.md`, `README.md`
**Subagent:** `worker` *(v0.7 fix: was `docs_editor`, but the Codex agent inventory has `docs_editor` as a read-only research role; this task does workspace writes (find-and-replace + scripts/docs-command-validity.sh creation) so `worker` is the right fit — write-capable, no domain specialization needed for mechanical edits)*
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** *(none beyond globally active — `write-human` already loaded)*
**Per-task gate:** `bash scripts/docs-command-validity.sh` (new — written as part of this task; greps for `cargo run -p memoryd --` patterns and fails)
**Worktree:** `../agent-memory-wt/task-10/` on `dogfood/task-10-docs-sweep`

**Files:**
- Modify: every `.md` file under `docs/` (excluding Task 9's owned file) — `cargo run -p memoryd --` → `cargo run --bin memoryd --`.
- Create: `scripts/docs-command-validity.sh` — greps for the dead pattern + a few other known-broken doc invocations (e.g., `memoryd doctor --reindex` would have failed pre-Task-9 — script can be reused as a doc-validity gate going forward).

```bash
git commit -m "docs: cargo run --bin memoryd everywhere; doc-validity script"
```

---

## Phase 4 — TUI redesign: themed inbox + inspector + focus modes

The dogfood-readiness closeout left the TUI honest but bureaucratic — 9 typed panels behind a tab bar, with 6 of them rendering sample fixtures because the protocol surface to feed them did not yet exist. v1.0 of this plan replaces the 9-panel shell with a **unified inbox + inspector + filter pills + command palette**, and bakes in a **theming foundation** as a Day-1 invariant — all colors/glyphs/borders/density/keymap configurable, six preset themes shipped, hot-reload from `~/.config/memorum/theme.toml`, terminal-capability-aware color resolution (true-color → 256 → 16-color floor), and a charset fallback for terminals/fonts that mangle the default glyph set.

The redesign preserves Stream G v0.1's data contract (the daemon protocol surface and notification dispatcher are unchanged); it changes presentation only. The reference design is captured in `/tmp/memorum-tui-mockup.html` (warm-dark amber-tinted default, `Spacing::Overlap(1)` shared border between inbox and inspector, OKLCH color tokens, JetBrains-Mono-friendly glyphs `● ▸ ⚠ ▣ ◇ ○`).

**Cluster discipline.** Task 10A creates the new `crates/memorum-theme/` crate and does not touch any memoryd-tui files — it precedes Cluster C as a non-collision blocker. Cluster C (sequential) is **11 → 11B → 12 → 13 → 14B**, all touching `crates/memoryd-tui/src/app.rs` at minimum. Task 11A remains in Cluster A (handlers/mod.rs); its protocol payloads back the new inbox stream and the typed inspector views, not the old panels. Task 14 stays in Cluster B (main.rs) and now lives behind the inspector's policy block instead of the deprecated trust-artifact panel.

**File ownership in `crates/memoryd-tui/src/`.** The redesign deletes most of the old `panels/` directory and re-homes its render code under new `inbox/`, `inspector/`, `focus/`, `palette/`, and `status/` modules. The deletion list and replacement-test mapping are explicit in Task 11's "Files" block so no test coverage is silently lost.

---

### Task 10A: memorum-theme foundation crate (Cluster C precondition)

**Parallel:** yes (with any non-Cluster-C task once Phase 3 closes; standalone new crate)
**Blocked by:** none structurally; sequence after Phase 3 to avoid colliding with the dogfood-readiness baseline. Hard blocker for **all of Cluster C** (Task 11 onward).
**Owned files:** `crates/memorum-theme/Cargo.toml`, `crates/memorum-theme/src/lib.rs`, `crates/memorum-theme/src/theme.rs`, `crates/memorum-theme/src/tokens.rs`, `crates/memorum-theme/src/glyphs.rs`, `crates/memorum-theme/src/border.rs`, `crates/memorum-theme/src/density.rs`, `crates/memorum-theme/src/motion.rs`, `crates/memorum-theme/src/keymap.rs`, `crates/memorum-theme/src/oklch.rs`, `crates/memorum-theme/src/resolver.rs`, `crates/memorum-theme/src/charset.rs`, `crates/memorum-theme/src/loader.rs`, `crates/memorum-theme/src/hot_reload.rs`, `crates/memorum-theme/src/presets/mod.rs`, `crates/memorum-theme/src/presets/default_warm_dark.toml`, `crates/memorum-theme/src/presets/default_light.toml`, `crates/memorum-theme/src/presets/kanagawa.toml`, `crates/memorum-theme/src/presets/gruvbox_dark.toml`, `crates/memorum-theme/src/presets/catppuccin_mocha.toml`, `crates/memorum-theme/src/presets/tokyo_night.toml`, `crates/memorum-theme/tests/preset_coverage.rs`, `crates/memorum-theme/tests/oklch_resolution.rs`, `crates/memorum-theme/tests/loader_validation.rs`, `crates/memorum-theme/tests/hot_reload.rs`, `crates/memorum-theme/tests/charset_detection.rs`, `Cargo.toml` (workspace member registration), `docs/api/memorum-theme-api.md`
**Subagent:** `backend_arch`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memorum-theme --tests && cargo clippy -p memorum-theme --tests -- -D warnings && cargo fmt -p memorum-theme -- --check && cargo build --workspace --locked`
**Worktree:** `../agent-memory-wt/task-10A/` on `dogfood/task-10A-theme-foundation`

**Why this task exists (v1.0).** v0.9 Task 11 wired the existing 9 panels to live daemon data without addressing the larger UX problem: scattered panels do not match how the data actually feels (one stream of memorial events, not 9 typed buckets), and every panel hardcodes `Color::*` and box-drawing characters inline, foreclosing user customization. v1.0 inverts this: theming is a Day-1 invariant, every render path takes `&Theme`, and the panel-replacement work in Task 11 is not done until no rendering code mentions a literal color or glyph. Building the theme crate first — and only the theme crate — keeps Task 10A behavior-preserving (no TUI changes) and makes Task 11's seam explicit.

**Architecture — what `memorum-theme` exposes.**

1. **`Theme` struct** with five composable sections, all serde-derived:
   - `colors: ColorTokens` — semantic tokens, OKLCH stored, `Color::Rgb`/`Indexed`/`Named` resolved per terminal capability.
   - `glyphs: Glyphs` — single-char or short-string glyphs for item-kind icons, separators, cursor, progress fill, etc.
   - `borders: BorderStyle` — `Plain | Rounded | Double | Thick | Dashed | DoubleDashed` mapped to ratatui's `border::Set` constants.
   - `density: Density` — `Comfortable | Compact` (controls list-row vertical padding, header height, gutter width).
   - `motion: MotionConfig` — `enabled: bool`, `slide_in_ms: u16` (default 350), `undo_window_ms: u16` (default 3000), `tick_ms: u16` (default 16).
   - `keymap: Keymap` — `HashMap<KeyChord, Action>`; vim+arrows hybrid by default, fully overridable.

2. **`ColorTokens`** — exhaustive set, every field non-`Option`, every preset declares every token (compile-time validation in `tests/preset_coverage.rs`). Token list: `bg`, `surface`, `surface_2`, `border`, `border_soft`, `fg`, `fg_muted`, `fg_dim`, `accent`, `accent_soft`, `status_ok`, `status_warn`, `status_bad`, `status_info`, `glyph_review`, `glyph_recall`, `glyph_conflict`, `glyph_dream`, `glyph_due`, `glyph_memory`, `selection_gutter`, `palette_bg`, `palette_match`. (23 tokens total.) Names are presentation-semantic, not color-named — users remap tokens to colors, never the inverse.

3. **`oklch::OklchColor`** — parses `oklch(L C H)` literals (e.g. `oklch(0.16 0.006 70)`) and `#rrggbb` hex; stores as `(l, c, h)` floats; converts to sRGB on demand.

4. **`Resolver`** — at TUI startup, queries the terminal once via the existing `crossterm` backend's reported capabilities plus `COLORTERM`/`TERM` env-var inspection. Returns a `ColorCapability` enum: `TrueColor | Indexed256 | Indexed16 | Monochrome`. `Theme::resolve(&Resolver)` lowers every `OklchColor` to the best `ratatui::style::Color` for that capability — `Color::Rgb` for true-color, `Color::Indexed(n)` for 256-color (nearest-OKLCH match against the xterm-256 palette, computed once at startup and cached in a `[u8; 4096]` lookup), `Color::Named(_)` for 16-color (snapped to ANSI 16). `Monochrome` collapses everything to `White`/`Black` and disables the accent.

5. **`Charset`** — `Full | Extended | Minimal`. Detected by inspecting `LC_ALL`/`LANG` for UTF-8 and `TERM` for terminals known to render Braille/box-drawing well (xterm-256color, alacritty, ghostty, kitty, wezterm, foot, iTerm.app). `Minimal` substitutes ASCII fallback glyphs (`* > ! ? ~ o` for `● ▸ ⚠ ▣ ◇ ○`) and forces `borders: Plain` (no Unicode box-drawing). `Extended` keeps default Unicode but skips Nerd Font glyph variants. `Full` permits Nerd Font when present.

6. **`Loader`** — three-step resolution: (1) compile-time embedded preset selected by name (the six shipped presets are `include_str!`'d from `src/presets/*.toml`); (2) optional override from `~/.config/memorum/theme.toml` (top-level) or `~/.config/memorum/themes/<name>.toml` (named); (3) CLI flag `--theme <name>` overrides config. Missing tokens are a hard error with the missing token name; no silent defaults. The default-preset name is `default-warm-dark`.

7. **`HotReload`** — `notify` crate watches the config file path (when present) on a debounced 200ms window. On file change: re-parse, re-validate via the same loader path, return a fresh `Theme` to the consumer through a `tokio::sync::watch::Receiver<Theme>`. On parse/validation error, the receiver does not advance; the error string is exposed via `HotReload::last_error()` so the TUI can render a footer banner without crashing.

8. **`Action` enum** for keymap — `MoveUp | MoveDown | MoveLeft | MoveRight | Enter | OpenPalette | OpenSearch | OpenHelp | NextFilter | PrevFilter | AcceptItem | RejectItem | EditItem | ForgetItem | Confirm | Correct | Skip | PauseSession | Quit`. The crate exposes the action enum and the default `Keymap::vim_arrows()` constructor; the actual dispatch lives in memoryd-tui (Task 11), so `memorum-theme` has zero dependency on `crossterm` event types — `KeyChord` is its own data type (`{ key: KeyCode, mods: KeyModifiers }`) with explicit conversions added in Task 11's `app.rs` glue.

**Stream G v0.1 spec preservation.** The data exposed by Stream G (Status, Doctor, RealityCheck, RecallHits, ReviewQueue, Notifications, TrustArtifact) is unchanged. The protocol contract (`memoryd` socket payloads + MCP tools) is unchanged. The TUI's *presentation* of that data is the only thing this task reshapes; the spec contract is preservation, not amendment.

**Files:**
- Create: `crates/memorum-theme/Cargo.toml` — package metadata; deps: `serde = { version = "1", features = ["derive"] }`, `serde_yaml`, `toml = "0.8"`, `notify.workspace = true` (the workspace pins `notify = "8.0"` at root `Cargo.toml:35`; the new crate inherits — do NOT pin a different major), `unicode-width`, `tokio = { version = "1", features = ["sync"] }`, `thiserror`. Dev-deps: `tempfile`, `pretty_assertions`. **Note: no `ratatui` dependency** — the crate exposes its own `OklchColor` type and conversion functions; consumers (memoryd-tui) do the `ratatui::style::Color` translation at the seam in Task 11. This keeps memorum-theme reusable for the web dashboard (Phase 5 may export tokens as CSS custom properties). **notify 8.x API note:** the worker uses the `RecommendedWatcher` builder + `Config` API (notify 8 changed the watcher constructor surface from notify 6); `HotReload::start` should be implemented against the 8.x docs at first read, not v6 muscle memory.
- Create: `crates/memorum-theme/src/lib.rs` — re-exports: `Theme`, `ResolvedTheme`, `ColorTokens`, `ResolvedColor`, `Glyphs`, `BorderStyle`, `BorderGlyphs`, `Density`, `MotionConfig`, `Keymap`, `Action`, `KeyChord`, `OklchColor`, `Resolver`, `ColorCapability`, `Charset`, `Loader`, `HotReload`, `LoaderError`. Crate-level docs explain the resolution pipeline.
- Create: `crates/memorum-theme/src/theme.rs` — `Theme` struct, `Theme::default_warm_dark()`, `Theme::from_loader(...)`, `Theme::resolve(&Resolver) -> ResolvedTheme` (the post-resolution variant carrying RGB tuples ready for ratatui mapping in the consumer).
- Create: `crates/memorum-theme/src/tokens.rs` — `ColorTokens` struct with all 23 fields; `#[serde(deny_unknown_fields)]` so typos in user themes fail loudly.
- Create: `crates/memorum-theme/src/glyphs.rs` — `Glyphs` struct: `review`, `recall`, `conflict`, `dream`, `due`, `memory`, `cursor`, `progress_filled`, `progress_empty`, `pill_separator`, `palette_prompt`. ASCII-fallback variants behind `#[serde(default = "ascii_fallback_*")]` per token.
- Create: `crates/memorum-theme/src/border.rs` — `BorderStyle` enum (`Plain | Rounded | Double | Thick | Dashed | DoubleDashed`) and a stable `BorderGlyphs { top: char, bottom: char, left: char, right: char, top_left: char, top_right: char, bottom_left: char, bottom_right: char, vertical_left: char, vertical_right: char, horizontal_top: char, horizontal_bottom: char, cross: char }` struct that maps each `BorderStyle` to its glyph set. **No ratatui dependency** — the crate stays ratatui-free; the consumer in Task 11 (`theme_glue.rs`) imports `BorderGlyphs` and constructs the `ratatui::symbols::border::Set` from it. `BorderStyle::glyphs(&self) -> BorderGlyphs` is the public accessor; raw glyph fields are `pub` so `theme_glue` can read them directly.
- Create: `crates/memorum-theme/src/density.rs` — `Density` enum + `pad_top`/`pad_bottom`/`row_height`/`gutter_width` accessor methods.
- Create: `crates/memorum-theme/src/motion.rs` — `MotionConfig` struct + `MotionConfig::reduced()` for `prefers-reduced-motion`-equivalent (terminals don't surface that signal; TUI exposes a CLI flag `--no-motion`).
- Create: `crates/memorum-theme/src/keymap.rs` — `Action` enum, `KeyChord` struct, `Keymap` (HashMap<KeyChord, Action>), `Keymap::vim_arrows()`, `Keymap::emacs()`, `Keymap::merge_user_overrides(&mut self, overrides: HashMap<KeyChord, Action>)`.
- Create: `crates/memorum-theme/src/oklch.rs` — `OklchColor { l: f32, c: f32, h: f32 }`, `parse_oklch(&str) -> Result<Self, ParseError>` (accepts `oklch(0.16 0.006 70)` and `oklch(0.16 0.006 70 / 0.8)` for alpha — alpha is parsed but unused by the current resolver), `parse_hex(&str)`, `to_srgb(&self) -> (u8, u8, u8)` (Oklab → linear sRGB → gamma-corrected sRGB).
- Create: `crates/memorum-theme/src/resolver.rs` — `ColorCapability` enum (`TrueColor | Indexed256 | Indexed16 | Monochrome`), `Resolver::detect() -> ColorCapability` (env-var inspection: `COLORTERM=truecolor`/`24bit` → TrueColor; `TERM=xterm-256color`/`screen-256color`/`tmux-256color` → Indexed256; `TERM=dumb` or empty → Monochrome; else Indexed16). `Resolver::override_from_env() -> Option<ColorCapability>` reads `MEMORUM_FORCE_COLOR` (accepted values `truecolor`/`256`/`16`/`mono`); when set, takes precedence over `detect()`. `Resolver::with_capability(cap: ColorCapability)` constructor for explicit programmatic override (used by the CLI flag wired in Task 11 — `--color-capability <truecolor|256|16|mono>` — which has the highest precedence). Resolution order: CLI flag > env override > auto-detect. `Resolver::resolve_oklch(&OklchColor) -> ResolvedColor` (with `[u8; 4096]` 16-cube lookup for 256-color matching, computed once at first call via `OnceCell`).
- Create: `crates/memorum-theme/src/charset.rs` — `Charset` enum, `Charset::detect()` (inspect `LC_ALL`/`LANG`, return `Minimal` if not UTF-8, else `Extended`/`Full` based on `TERM`).
- Create: `crates/memorum-theme/src/loader.rs` — `Loader::resolve(name: Option<&str>, config_path: Option<&Path>) -> Result<Theme, LoaderError>`. `LoaderError` variants: `MissingToken(String)`, `ParseFailed(String)`, `UnknownPreset(String)`, `Io(io::Error)`. Compile-time embedded presets via `include_str!` indexed by name in a `&'static [(&str, &str)]` table.
- Create: `crates/memorum-theme/src/hot_reload.rs` — `HotReload::start(path: PathBuf, initial: Theme) -> (HotReload, watch::Receiver<Theme>)`; a tokio task watches via `notify`, debounces 200ms, attempts re-load, advances the receiver on success, captures `last_error` on failure.
- Create: `crates/memorum-theme/src/presets/mod.rs` — `pub static PRESETS: &[(&str, &str)] = &[...]` listing all six presets via `include_str!`.
- Create: `crates/memorum-theme/src/presets/default_warm_dark.toml` — the warm-dark amber-tinted preset matching `/tmp/memorum-tui-mockup.html` (background `oklch(0.16 0.006 70)`, accent `oklch(0.80 0.13 72)`, all 23 tokens declared).
- Create: `crates/memorum-theme/src/presets/default_light.toml` — daytime/outdoor variant; same hue family, inverted lightness scale.
- Create: `crates/memorum-theme/src/presets/kanagawa.toml`, `gruvbox_dark.toml`, `catppuccin_mocha.toml`, `tokyo_night.toml` — community presets, each with provenance comment at the top citing the upstream palette source.
- Create: `crates/memorum-theme/tests/preset_coverage.rs` — for each of the six presets: deserialize from embedded TOML; assert every `ColorTokens` field is present and resolves to a valid `OklchColor`; assert every `Glyphs` field is present; assert no preset declares an unknown token (caught by `deny_unknown_fields`).
- Create: `crates/memorum-theme/tests/oklch_resolution.rs` — `OklchColor::parse_oklch` accepts and rejects malformed input; round-trip OKLCH → sRGB stable to ±1 LSB; `Resolver` produces stable output for fixed input under each `ColorCapability`; 256-color lookup picks the documented nearest cell for known anchor colors.
- Create: `crates/memorum-theme/tests/loader_validation.rs` — `Loader::resolve("default-warm-dark", None)` succeeds; `Loader::resolve("nonexistent", None)` returns `UnknownPreset`; a TOML with a missing token returns `MissingToken("accent")`; a TOML with an unknown extra token returns `ParseFailed`; CLI override of preset name takes precedence over config-file preset.
- Create: `crates/memorum-theme/tests/hot_reload.rs` — write a TOML to a tempdir, start HotReload, modify the file, **poll the receiver every 50ms for up to 2s** (using `tokio::time::timeout` wrapping a `loop { interval.tick().await; if rx.has_changed().unwrap_or(false) { break; } }`) and assert it advances; write malformed TOML and confirm via the same poll-with-timeout that the receiver does NOT advance for the full 2s and `last_error()` returns `Some(...)`; concurrent reload safe (multiple writes coalesced by the 200ms debounce). The poll-with-backoff is critical for CI robustness — macOS FSEvents debounce can lag under load and a single 1s sleep would flake.
- Create: `crates/memorum-theme/tests/charset_detection.rs` — `Charset::detect()` with `LANG=en_US.UTF-8 TERM=xterm-256color` returns `Extended` (or `Full`); `LANG=POSIX` returns `Minimal`; `LANG=` returns `Minimal`. Tests use `temp_env::with_var(...)` to scope env-var changes.
- Modify: `Cargo.toml` (workspace) — add `crates/memorum-theme` to `[workspace] members` array; the orchestrator's lockfile-merge cadence (per v0.9 §Fire-and-forget operating manual) handles `Cargo.lock`.
- Create: `docs/api/memorum-theme-api.md` — public API reference: token list, preset list, loader resolution rules, hot-reload contract, terminal-capability fallback rules. Cross-references to the Stream G v0.1 spec note that this is presentation tooling, not a spec amendment.

**Step 1: Write all five test files**, expecting compile failure on the unknown crate types.

**Step 2: Run per-task gate, expect fail.**

**Step 3: Implement** the module set top-down: `tokens.rs` → `glyphs.rs` → `border.rs` → `density.rs` → `motion.rs` → `keymap.rs` → `oklch.rs` → `charset.rs` → `resolver.rs` → `loader.rs` → `hot_reload.rs` → `theme.rs` → `lib.rs`. Authoring order follows dep topology (lower modules have no upward deps).

**Step 4: Author all six preset TOMLs** by hand using OKLCH literals; cross-reference the mockup for `default-warm-dark`. Run `cargo test -p memorum-theme --test preset_coverage` until green.

**Step 5: Re-run per-task gate.** Expect: all five test binaries pass, clippy clean, fmt clean, workspace builds with `--locked`.

**Step 6: Commit.**
```bash
git add crates/memorum-theme/ Cargo.toml docs/api/memorum-theme-api.md
git commit -m "feat(theme): memorum-theme foundation crate (tokens, presets, oklch, hot-reload)"
```

**Step 7: Hand back to orchestrator.**

---

### Task 11A: Daemon protocol read endpoints for inbox + inspector (precondition for Task 11)

**Parallel:** no (Cluster A — slot between Task 9 and Task 19)
**Blocked by:** Task 9 (handlers.rs → handlers/mod.rs conversion must land first; Task 11A's owned files use the post-Task-9 module path)
**Owned files:** `crates/memoryd/src/protocol.rs` (adds 5 RequestPayload + matching ResponsePayload variants), `crates/memoryd/src/handlers/mod.rs` (post-Task-9 path; adds 5 handler functions), `crates/memoryd/src/mcp.rs` (no manifest change; only enforces the new payloads are NOT exposed via MCP), `crates/memoryd/tests/protocol_contract.rs` (extends contract tests for the 5 new payloads)
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test protocol_contract && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-11A/` on `dogfood/task-11A-tui-protocol-payloads`

**Why this task exists (v0.5; framing updated v1.0).** v0.4 Task 11 said panels would call `memoryd review pending`, `memoryd inspect entities`, `memoryd namespace tree`, `memoryd governance policy` — but the TUI does not shell out, it dispatches `protocol::Request` payloads via `client.dispatch_daemon_call(...)`. Five of the six panels in the original Task 11 scope (conflicts, entities, timeline, namespace, policy) had **no corresponding `RequestPayload` variant in `crates/memoryd/src/protocol.rs`**. v0.5 added Task 11A to fix that. v1.0 keeps the same five protocol payloads, but reframes their consumer: in v1.0 the TUI does not have those panels at all. Instead, `ConflictsList` feeds the inbox stream as conflict items; `EventsLogPage` feeds the timeline-flavored filter and the inspector's "recent activity" block; `InspectEntities` and `NamespaceTree` feed the command palette's jump-to actions and the inspector's relationship section; `GovernancePolicyDump` feeds the inspector's policy block. The data exposed and the handler implementations are unchanged from the v0.5 design — only the consumer mapping changed.

**System spec note.** These five payloads are **daemon-protocol-only**, not MCP tools — they do not appear in `tools/list` and `mcp.rs` rejects them at the forwarder. System spec §14.1 v1 surface stays at 10 MCP tools (Task 22 ratifies that count). Daemon-side admin/observability protocol expansion is unrestricted.

**Files:**
- Modify: `crates/memoryd/src/protocol.rs` — add to `RequestPayload`: `InspectEntities { limit: Option<usize>, prefix: Option<String> }`, `EventsLogPage { since: Option<EventId>, limit: usize, kind_filter: Option<Vec<EventKind>> }`, `NamespaceTree { root: Option<String>, depth: Option<usize> }`, `GovernancePolicyDump`, `ConflictsList { limit: Option<usize> }`. Add matching `ResponsePayload` variants with stable shapes (`EntitySummary`, `EventLogEntry`, `NamespaceNode`, `GovernancePolicySnapshot`, `ConflictSummary` — names final, doc-commented).
- Modify: `crates/memoryd/src/handlers/mod.rs` (post-Task-9 path) — add 5 handler functions: `handle_inspect_entities` (queries Stream A index `entity_mentions` table), `handle_events_log_page` (reads `events_log` JSONL with optional kind filter and `since` cursor), `handle_namespace_tree` (walks substrate tree under `<root>/`), `handle_governance_policy_dump` (returns the loaded policy YAML and the active confidence_floor / gates), `handle_conflicts_list` (reads `quarantine/` entries from substrate). All five paths are read-only — no Stream A mutation, no Stream D classifier touch, no privacy refusal possible.
- Modify: `crates/memoryd/src/mcp.rs` — extend the MCP forwarder rejection list to explicitly refuse routing the new payloads through MCP (defense-in-depth; the manifest already does not list them, but the forwarder match arm should error if it ever sees one).
- Test: `crates/memoryd/tests/protocol_contract.rs` — for each new payload: roundtrip serialize/deserialize; handler returns the documented shape on a populated substrate; handler returns empty-but-shape-honoring response on an empty substrate; all five payloads are rejected by the MCP forwarder.

```bash
git commit -m "feat(protocol): add 5 read-only daemon endpoints for TUI inbox + inspector"
```

---

### Task 11: TUI shell rewrite — themed inbox + inspector + filter pills + status line

**Parallel:** no (Cluster C, after Task 10A and Task 11A)
**Blocked by:** Task 4 (daemon auto-start path), Task 5 (socket resolver), Task 10A (memorum-theme crate must exist; the TUI shell is built around `&Theme` from line one), Task 11A (protocol payloads must exist before the TUI client calls them)
**Owned files:** `crates/memoryd-tui/Cargo.toml` (adds `memorum-theme` dep), `crates/memoryd-tui/clippy.toml` (new — see clippy enforcement note below), `crates/memoryd-tui/src/main.rs` (adds `--theme`, `--charset`, `--no-motion`, `--color-capability`, `--theme-config` CLI flags + theme bootstrap), `crates/memoryd-tui/src/lib.rs` (re-exports updated), `crates/memoryd-tui/src/app.rs` (full rewrite of state/event/render), `crates/memoryd-tui/src/client.rs` (typed wrappers for the 5 Task-11A payloads + unified inbox-stream merge function), `crates/memoryd-tui/src/config.rs` (theme name, density override, hot-reload toggle), `crates/memoryd-tui/src/state.rs` (new — central per-feature state; carries `RealityCheckState` extracted from the deleted `panels/reality_check.rs` so Task 12 can extend it without a `Create:` collision), `crates/memoryd-tui/src/theme_glue.rs` (new — translates `memorum-theme::ResolvedColor` to `ratatui::style::Color`, builds `border::Set` from `BorderGlyphs`, converts `crossterm::event::KeyEvent` to `KeyChord`), `crates/memoryd-tui/src/focus/mod.rs` (new STUB — defines `FocusKind` enum (`None | RealityCheck { session: SessionId } | CorrectEditor { item_id: MemoryId }`) and a `render(frame, area, kind, app, theme)` dispatch that returns immediately for `RealityCheck`/`CorrectEditor` (Task 12 and Task 13 fill those arms in via `Modify:`); the stub exists so `app.rs` can reference `crate::focus::FocusKind` at compile time), `crates/memoryd-tui/src/inbox/mod.rs`, `inbox/item.rs`, `inbox/filter.rs`, `inbox/ranking.rs`, `crates/memoryd-tui/src/inspector/mod.rs`, `inspector/memory_view.rs`, `inspector/review_view.rs`, `inspector/conflict_view.rs`, `inspector/recall_view.rs`, `inspector/dream_view.rs`, `inspector/due_view.rs`, `inspector/fields.rs`, `crates/memoryd-tui/src/status/mod.rs`, `crates/memoryd-tui/tests/inbox_render.rs`, `tests/inspector_router.rs`, `tests/filter_pills.rs`, `tests/inbox_polling.rs`, `tests/inbox_ranking.rs`, `tests/theme_application.rs`. **Deletions** (replaced; coverage migrates per the test mapping below): `crates/memoryd-tui/src/panels/mod.rs`, `panels/overview.rs`, `panels/review_queue.rs`, `panels/conflicts.rs`, `panels/entities.rs`, `panels/timeline.rs`, `panels/namespace.rs`, `panels/policy.rs`, `panels/recall.rs`, `panels/reality_check.rs` (its `RealityCheckState` moves to `state.rs`; the v0.9 hardcoded `"0 of 12"` at line 89 of the deleted file is gone with it), `tests/panel_render.rs`, `tests/recall_panel.rs`, `tests/keymap.rs` (rewritten as `tests/keymap_actions.rs` against the new `Action` dispatch).
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-tui --test inbox_render --test inspector_router --test filter_pills --test inbox_polling --test inbox_ranking --test theme_application --test keymap_actions --test resize --test socket_unreachable --test trust_artifact --test panic_restore && cargo clippy -p memoryd-tui --tests -- -D warnings && cargo fmt -p memoryd-tui -- --check`
**Worktree:** `../agent-memory-wt/task-11/` on `dogfood/task-11-tui-inbox-inspector`

**Architecture — what the new shell looks like.** Reference: `/tmp/memorum-tui-mockup.html`. The TUI is a single primary view with five regions:

1. **Header** (1-row): brand sigil + filter pills (`all`, `review·N`, `conflicts·N`, `recall`, `dreams·N`, `due·N`) + right-aligned hotkey hints (`/` search, `:` palette, `?` help). Pills are a custom widget rendered through `&Theme.colors`.

2. **Inbox** (left pane, `Constraint::Percentage(36)` by default, configurable): a `List<HighlightSpacing::Always>` of `Item` rows, two visual rows per item (icon + title; sub-line with scope, source, and counters). The selected item renders with `accent`-colored gutter via `Block::new().border_style(...)` plus `Spacing::Overlap(1)` against the inspector pane (no doubled separator). Filter changes update the underlying `Vec<Item>` view via `inbox::ranking::merge_and_filter(...)`.

3. **Inspector** (right pane, `Constraint::Fill(1)`): an `InspectorRouter` that picks a typed view (`memory_view`, `review_view`, `conflict_view`, `recall_view`, `dream_view`, `due_view`) based on the focused `Item`'s `Kind`. Every view renders through `&Theme` exclusively — clippy lint `disallowed_methods` blocks any direct `Color::*` literal in the inspector module (configured in `clippy.toml` as part of this task).

4. **Focus-mode overlay slot** (full-pane takeover, when active): renders Reality Check (Task 12) or Correct Editor (Task 13) instead of the inbox+inspector grid. Activated via `Action::EnterFocusMode(FocusKind)` on the `App` event loop. While active, header pills and status line still render; only the middle panes are replaced.

5. **Status line** (1-row footer): daemon health, sync state, peer count, next-dream-time, current filter, contextual hotkeys. All glyphs and labels read from `&Theme`.

**Item kinds and ranking.** `inbox::item::Item` is an enum (`ReviewCandidate { … }`, `Conflict { … }`, `RecallHit { … }`, `RealityCheckDue { … }`, `DreamOutput { … }`, `Memory { … }`). The unified inbox stream is built client-side: `client.fetch_inbox_stream(filter, cap_per_source)` issues parallel daemon calls (`ReviewQueue`, `ConflictsList`, `RecallHits`, `RealityCheck::List`, `EventsLogPage` filtered to `EventKind::DreamPassWritten`/`SubstrateFragmentWritten`), each capped at `cap_per_source` (default 50), then merges and sorts by `recency_score = recency_seconds * urgency_weight(kind)` where `urgency_weight` ranks `Conflict > RealityCheckDue > ReviewCandidate > DreamOutput > RecallHit > Memory`. Rationale: the data is unified in feel; ranking is a presentation concern; keeping it client-side avoids protocol churn when we tune the formula.

**Theme integration discipline.** Every render function takes `&Theme` (or a derived view like `&InspectorContext` carrying theme + focused item). The new `crates/memoryd-tui/clippy.toml` adds the theme enforcement rules **and re-declares the workspace thresholds** because a per-crate `clippy.toml` shadows the workspace root file (it does not merge):
- `too-many-lines-threshold = 60` (mirrors workspace `/Users/treygoff/Code/agent-memory/clippy.toml`)
- `cognitive-complexity-threshold = 15` (mirrors workspace)
- `too-many-arguments-threshold = 4` (mirrors workspace)
- `disallowed-methods = [{ path = "ratatui::style::Style::default", reason = "construct Style via theme tokens, not defaults" }]`
- `disallowed-types = [{ path = "ratatui::style::Color", reason = "use memorum_theme::ResolvedColor and the theme_glue translation seam" }]`

The two new disallowed-* rules apply project-wide *except* the `theme_glue` module, which is exempt via `#[allow(...)]` attributes at the module head. The `theme_glue` module is the *only* place `memorum-theme::ResolvedColor` becomes `ratatui::style::Color` and the only place `BorderGlyphs` becomes `ratatui::symbols::border::Set`. Everywhere else takes `Theme` (or sub-views) and builds Styles via theme-helper methods. Note: per-crate clippy.toml shadowing means the workspace file at `/Users/treygoff/Code/agent-memory/clippy.toml` no longer governs this crate; copying its three thresholds preserves the existing checks.

**Test coverage migration.** Old test → new test mapping (so coverage is preserved, not lost):
- `tests/panel_render.rs` (snapshot of all 9 panels) → `tests/inbox_render.rs` + `tests/inspector_router.rs` (snapshots of inbox in 4 states × inspector in 6 kinds = 24 snapshot cases) and `tests/filter_pills.rs` (pill rendering with counts in 6 filter states).
- `tests/recall_panel.rs` → `tests/inbox_render.rs::recall_filter` and `tests/inspector_router.rs::recall_view` (the recall filter shows recall items; the recall_view inspector renders score/harness/session fields *only when the daemon protocol exposes them* — same Stream A protocol-truthfulness invariant as v0.9).
- `tests/keymap.rs` → `tests/keymap_actions.rs` (every `Action` from the keymap has a dispatch test against the new App state; vim+arrows default verified; user override scenario verified).
- Kept unchanged: `tests/resize.rs`, `tests/socket_unreachable.rs`, `tests/trust_artifact.rs`, `tests/panic_restore.rs`. Each is rewired to the new App constructor signature in this task; the assertions stay the same.

**Step 1: Add memorum-theme dep and clippy enforcement.** Modify `Cargo.toml` (add `memorum-theme = { path = "../memorum-theme" }`). Add `crates/memoryd-tui/clippy.toml` with the five entries listed in the "Theme integration discipline" block above (three thresholds mirrored from workspace + two new disallowed-* rules). Run `cargo clippy -p memoryd-tui --tests -- -D warnings`; expect clean before any rewrite (the existing TUI code uses no `Color` literals or `Style::default` outside what `theme_glue` will eventually wrap, but if the lint trips here, fix the offending site as part of this task — that's the disovery point of the discipline).

**Step 2: Author all 6 new test files (failing).** `tests/inbox_render.rs`, `tests/inspector_router.rs`, `tests/filter_pills.rs`, `tests/inbox_polling.rs`, `tests/inbox_ranking.rs`, `tests/theme_application.rs`. Use `ratatui::backend::TestBackend` for snapshot tests. The `theme_application` test renders the same buffer with `Theme::default_warm_dark()` and `Theme::for_test()` (deterministic ANSI), asserting that token swaps produce different rendered cells in expected positions.

**Step 3: Run per-task gate, expect compile failure.**

**Step 4: Implement `theme_glue.rs`** — the seam between `memorum-theme` (no ratatui dep) and `ratatui::style::*`. This file is the only place permitted to construct `Color::*` directly (exempted from the disallowed-types lint via a module-level `#[allow(...)]`). Includes `KeyChord::from_crossterm(KeyEvent) -> Option<KeyChord>` and `BorderGlyphs::to_border_set(&self) -> ratatui::symbols::border::Set` (also called only from this module).

**Step 4b: Wire CLI flags in `main.rs`.** Add `--theme <name>` (default `default-warm-dark`), `--theme-config <path>` (default `~/.config/memorum/theme.toml`, `None` if absent), `--charset <full|extended|minimal>` (default auto-detect via `Charset::detect()`), `--no-motion` (boolean; sets `Theme.motion.enabled = false`), `--color-capability <truecolor|256|16|mono>` (default auto-detect via `Resolver::detect()` with `MEMORUM_FORCE_COLOR` env override taking precedence over auto). Resolution precedence at startup: CLI flag > `MEMORUM_FORCE_COLOR` env var > auto-detect. The flag plumbing terminates in `App::new(theme: Theme, charset: Charset, capability: ColorCapability, hot_reload: Option<HotReload>)`.

**Step 5: Implement `inbox/`, `inspector/`, `status/`, `app.rs` rewrite, `client.rs` extensions.** Build top-down with TDD: each module's tests turn green as the impl lands. Inspector views share `inspector/fields.rs` for the kv-table renderer (provenance block, policy block).

**Step 6: Delete the old `panels/` directory.** `git rm` the 9 files. Update `lib.rs` re-exports.

**Step 7: Re-wire kept tests** (`resize.rs`, `socket_unreachable.rs`, `trust_artifact.rs`, `panic_restore.rs`) to the new `App::new(...)` signature.

**Step 8: Re-run per-task gate.** All 12 named test binaries pass; clippy clean (including new `disallowed-methods` rule); fmt clean; workspace builds with `--locked`.

**Step 9: Commit.**
```bash
git add -A crates/memoryd-tui/
git commit -m "feat(tui): inbox + inspector + filter pills shell, themed via memorum-theme"
```

**Step 10: Hand back to orchestrator.**

---

### Task 11B: Command palette (`:` key) with fuzzy match

**Parallel:** no (Cluster C, after Task 11)
**Blocked by:** Task 11
**Owned files:** `crates/memoryd-tui/src/palette/mod.rs` (new — `PaletteState`, render, dispatch glue), `crates/memoryd-tui/src/palette/commands.rs` (new — command catalog), `crates/memoryd-tui/src/palette/fuzzy.rs` (new — wraps the fuzzy-matcher dependency), `crates/memoryd-tui/Cargo.toml` (adds `nucleo-matcher = "0.5"` for fuzzy matching — same crate Helix uses; orchestrator verifies the latest published 0.5.x version on crates.io before spawning, since 0.3 is outdated and the API changed between versions), `crates/memoryd-tui/src/app.rs` (palette overlay dispatch — bounded surface), `crates/memoryd-tui/tests/palette_open.rs`, `crates/memoryd-tui/tests/palette_fuzzy.rs`, `crates/memoryd-tui/tests/palette_dispatch.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-tui --test palette_open --test palette_fuzzy --test palette_dispatch && cargo clippy -p memoryd-tui --tests -- -D warnings && cargo fmt -p memoryd-tui -- --check`
**Worktree:** `../agent-memory-wt/task-11B/` on `dogfood/task-11B-command-palette`

**Why this task exists (v1.0).** The redesign collapses 9 panels into 1 inbox + 1 inspector, but keeps multiple navigational verbs (switch filter, jump to namespace, search memories, start reality check session, force sync, show doctor output, switch theme). v1.0 puts those verbs behind a single `:`-keyed command palette instead of scattering them across submenus. Read-only and read-modify operations in v1.0; destructive operations stay behind their existing item-level keys (`a`/`r`/`f`/etc.) so accidental palette dispatch can't forget memories.

**Command catalog (v1.0 initial set).**
- `filter:all`, `filter:review`, `filter:conflicts`, `filter:recall`, `filter:dreams`, `filter:due`
- `search` (opens `/`-style search prompt — the actual search routes through the existing memory_search MCP plumbing on the daemon side)
- `jump:namespace <name>` (uses Task 11A's `NamespaceTree`)
- `jump:entity <name>` (uses Task 11A's `InspectEntities`)
- `reality-check:start` (transitions the App into focus mode — dispatches to Task 12's machinery)
- `theme:switch <preset>` (live theme swap; the swap happens in-memory only — session-scoped — unless followed by `theme:save-as`)
- `theme:save-as <name>` (persists the currently active theme to `~/.config/memorum/themes/<name>.toml`; if `<name>` is omitted, writes to `~/.config/memorum/theme.toml` so the next startup picks it up by default)
- `theme:reload` (force re-read of the config file)
- `device:status`, `peer:list` (read-only diagnostic)
- `dream:next-run` (read-only — shows next scheduled cleanup time)
- `help` (shows the help overlay)

Commands are declared as `Command { id, label, action: PaletteAction }` in `palette/commands.rs`. `PaletteAction` is a thin enum that the App dispatcher translates into the same `Action` enum from `memorum-theme::keymap` plus a few palette-only variants (`SwitchTheme(String)`, `ReloadTheme`, etc.).

**Files:**
- Modify: `crates/memoryd-tui/Cargo.toml` — add `nucleo-matcher = "0.3"` (lightweight fuzzy matcher; same crate Helix uses).
- Create: `crates/memoryd-tui/src/palette/mod.rs` — `PaletteState { input: String, cursor: usize, candidates: Vec<&Command>, selected: usize, open: bool }`, `render(frame, area, &Theme, &PaletteState)`, key handling (text input + Up/Down navigation + Enter/Esc).
- Create: `crates/memoryd-tui/src/palette/commands.rs` — full command catalog above.
- Create: `crates/memoryd-tui/src/palette/fuzzy.rs` — wraps `nucleo-matcher::Matcher` with a stable `score(query, label) -> Option<u32>` API.
- Modify: `crates/memoryd-tui/src/app.rs` — handle `Action::OpenPalette`, dispatch palette commands back into `Action`, render palette overlay using `Clear` widget + centered popup `Rect`.
- Create: `crates/memoryd-tui/tests/palette_open.rs` — `:` opens, `Esc` closes; cursor moves with arrow keys; rendering uses theme tokens.
- Create: `crates/memoryd-tui/tests/palette_fuzzy.rs` — `"rev"` matches `filter:review`; `"rc"` matches `reality-check:start`; ranked output stable.
- Create: `crates/memoryd-tui/tests/palette_dispatch.rs` — running `filter:review` switches the inbox filter; running `theme:switch kanagawa` swaps active theme via the hot-reload receiver; running an unknown command stays open and shows no-match state.

```bash
git commit -m "feat(tui): command palette (:) with fuzzy match for read-only and theme verbs"
```

---

### Task 12: Reality Check focus mode (replaces panel render with takeover view)

**Parallel:** no (Cluster C, after Task 11B)
**Blocked by:** Task 11B
**Owned files:** `crates/memoryd-tui/src/focus/mod.rs` (modify — Task 11 created the stub with the `FocusKind` enum and an empty render dispatch; Task 12 fills the `RealityCheck` arm), `crates/memoryd-tui/src/focus/reality_check.rs` (new — replaces the deleted `panels/reality_check.rs`; takeover view per `/tmp/memorum-tui-mockup.html` view 2), `crates/memoryd-tui/src/state.rs` (modify — Task 11 created this file with `RealityCheckState` extracted from the deleted panel; Task 12 extends it with `items_total: usize`, `items_reviewed: usize`, `session_id: Option<SessionId>`), `crates/memoryd-tui/src/client.rs` (adds `reality_check_session_progress` typed wrapper using existing `RealityCheck::List`), `crates/memoryd-tui/src/app.rs` (focus-mode overlay slot wiring + tick-counter increment for transition timing — see "Transition timing" note below), `crates/memoryd-tui/tests/focus_mode_render.rs`, `crates/memoryd-tui/tests/focus_mode_progress.rs`, `crates/memoryd-tui/tests/focus_mode_transition.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-tui --test focus_mode_render --test focus_mode_progress --test focus_mode_transition && cargo clippy -p memoryd-tui --tests -- -D warnings && cargo fmt -p memoryd-tui -- --check`
**Worktree:** `../agent-memory-wt/task-12/` on `dogfood/task-12-rc-focus-mode`

**Architecture.** Reality Check is the one ritual where small ceremony is appropriate. The focus-mode view (mockup view 2) renders the active memory in large text with four answer cards (`y` confirm / `k` correct / `f` forget / `s` skip), a thin `LineGauge` progress at the top reading `N of M` (kills the v0.9 hardcoded `"0 of 12"` that lived at `panels/reality_check.rs:89` — that file is deleted in Task 11; the historical line reference is cited only to anchor what was being replaced), and a side rail of session items with done/now/upcoming markers. Transitions between successive items use a 350ms slide-in driven by an App-tracked tick counter (see "Transition timing" below); `--no-motion` skips the transition entirely.

**Transition timing.** ratatui 0.29's `Frame` does not expose `frame.count()` — the public counter is `Terminal::frame_count()` on the terminal handle, which is not in scope inside the render closure. Implementation: Task 12 adds a `tick_counter: u64` field to `App` (incremented inside the existing `on_tick()` path at the top of the event loop). The focus-mode render reads `app.tick_counter` and `focus_state.transition_start_tick: Option<u64>` to compute progress; `progress = ((tick_counter - start) * tick_ms) / slide_in_ms` clamped to `[0, 1]`. When `Theme.motion.enabled == false` (or `--no-motion` is set), `progress` is forced to `1.0` immediately on entry so the slide is skipped entirely. This avoids the wrong assumption that `Frame` carries a counter and centralizes the timing source on App state.

**Files:**
- Modify: `crates/memoryd-tui/src/focus/mod.rs` — Task 11 stubbed this file with the `FocusKind` enum (variants: `None`, `RealityCheck { session: SessionId }`, `CorrectEditor { item_id: MemoryId }`) and an empty `render(...)` dispatch. Task 12 extends the `FocusMode` struct with `transition_start_tick: Option<u64>` and fills the `RealityCheck` arm of the dispatch to call into `focus::reality_check::render(...)`.
- Create: `crates/memoryd-tui/src/focus/reality_check.rs` — render + key handling. Reads `Theme.glyphs.progress_filled`/`progress_empty` for the LineGauge, `Theme.colors.accent` for the active answer card border, `Theme.motion.slide_in_ms` plus `Theme.motion.tick_ms` for transition timing (against the App tick counter, not `frame.count()`).
- Modify: `crates/memoryd-tui/src/state.rs` — Task 11 created this file with the bare `RealityCheckState` extracted from the deleted panel. Task 12 extends it with `items_total: usize`, `items_reviewed: usize`, `session_id: Option<SessionId>`; populated from `RealityCheck::List` response (which already returns total session items + per-item review state — Stream G shipped this surface).
- Modify: `crates/memoryd-tui/src/client.rs` — add `reality_check_session_progress(session: SessionId) -> Result<RealityCheckProgress>` that wraps the existing protocol. No new daemon-side work.
- Modify: `crates/memoryd-tui/src/app.rs` — when `Action::EnterFocusMode(FocusKind::RealityCheck { … })` fires, set `focus_mode.transition_start_tick = Some(app.tick_counter)`, swap render path to `focus::render(...)` until `Action::ExitFocusMode` fires (Esc or session complete). Increment `app.tick_counter` once per `on_tick()` call (existing tick site).
- Create: `crates/memoryd-tui/tests/focus_mode_render.rs` — snapshot of focus mode at 0%, 25%, 100% progress; assert no `"0 of 12"` substring anywhere in the rendered buffer.
- Create: `crates/memoryd-tui/tests/focus_mode_progress.rs` — `RealityCheckState { items_total: 0, items_reviewed: 0 }` renders `"0 of 0"`; `{ 5, 12 }` renders `"5 of 12"`; LineGauge fill matches ratio.
- Create: `crates/memoryd-tui/tests/focus_mode_transition.rs` — with `Theme.motion.enabled = false`, frame N+1 after entry shows item fully positioned; with `Theme.motion.enabled = true`, frame N+1 after entry shows partial slide-in; `--no-motion` flag in test fixture forces the disabled path.

```bash
git commit -m "feat(tui): Reality Check focus mode with live progress counter and motion-respecting transition"
```

---

### Task 13: Correct action — inline editor in focus mode

**Parallel:** no (Cluster C, after Task 12)
**Blocked by:** Task 12
**Owned files:** `crates/memoryd-tui/src/focus/correct_editor.rs` (new — multiline text input modal), `crates/memoryd-tui/src/client.rs` (replaces the v0.9 error stub at the previous `client.rs:178` with real dispatch), `crates/memoryd-tui/src/app.rs` (focus transition `RealityCheck → CorrectEditor → RealityCheck` on submit/cancel), `crates/memoryd-tui/tests/correct_editor.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-tui --test correct_editor && cargo clippy -p memoryd-tui --tests -- -D warnings && cargo fmt -p memoryd-tui -- --check`
**Worktree:** `../agent-memory-wt/task-13/` on `dogfood/task-13-correct-editor`

**Architecture.** Pressing `k` during a Reality Check item transitions focus from `FocusKind::RealityCheck` to `FocusKind::CorrectEditor`. The editor is rendered in-pane (not a separate full-screen modal) — the side rail and progress gauge stay visible so the session context is preserved. The editor is a multiline text input collecting the replacement body **only** (Esc cancels and returns to the same RC item; Ctrl-S submits). On submit, the TUI dispatches `RequestPayload::RealityCheck(RealityCheckRequest::Respond { session_id, memory_id, action: RealityCheckAction::Correct { new_body } })` — this is the actual protocol shape per `crates/memoryd/src/protocol.rs:198,206-208`; there is no `RealityCheckRequest::Correct` direct variant and `RealityCheckAction::Correct` carries only `new_body: String` (no `reason` field). On daemon ack (a `RealityCheckResponse::ResponseAccepted` or equivalent), focus transitions back to `FocusKind::RealityCheck` advancing to the next item.

**Protocol-shape note (v1.0 fix).** The plan-reviewer round caught that v0.9 prose described a `RealityCheckRequest::Correct { id, replacement, reason }` variant that does not exist. v1.0 corrects to the real shape and drops the `reason` collection from the editor. If a future task wants reason capture, that is an additive protocol change (`RealityCheckAction::Correct { new_body, reason: Option<String> }`) and lives in a separate task — out of scope for dogfood. For Forget-with-reason, the existing `RealityCheckAction::Forget { reason: String }` variant is dispatched from the `f` keybind with a default reason of `"user-forgot-via-tui"` until a richer prompt is added.

**Files:**
- Create: `crates/memoryd-tui/src/focus/correct_editor.rs` — `CorrectEditorState { body: String, cursor: (usize, usize) }` (single-field; no `reason` field, no `active_field` enum). Render uses `Theme.glyphs.cursor` and `Theme.colors.accent` for the cursor and footer hint; reuses the focus-mode side rail and progress gauge from Task 12 by composition (it does not re-render those — they stay drawn from `app.focus_mode`'s outer chrome). Esc returns `Action::ExitFocusMode`; Ctrl-S dispatches `RealityCheckRequest::Respond { session_id, memory_id, action: RealityCheckAction::Correct { new_body: body } }` via the daemon client.
- Modify: `crates/memoryd-tui/src/focus/mod.rs` — Task 11 stubbed the `CorrectEditor` arm of the render dispatch; Task 13 fills it to call `focus::correct_editor::render(...)`.
- Modify: `crates/memoryd-tui/src/client.rs` — the v0.9 stub returning `Err(anyhow!("Reality Check correct requires replacement text and is not supported by the TUI yet"))` is removed. The new `correct(session_id: String, memory_id: MemoryId, new_body: String) -> Result<()>` dispatches `RequestPayload::RealityCheck(RealityCheckRequest::Respond { session_id, memory_id, action: RealityCheckAction::Correct { new_body } })` and awaits the ack.
- Modify: `crates/memoryd-tui/src/app.rs` — wire `Action::Correct` (the `k` keybind from `Theme.keymap`) to `Action::EnterFocusMode(FocusKind::CorrectEditor { item_id: focused_rc_item.memory_id })`. The session_id required by the protocol comes from `app.state.reality_check.session_id` (populated by Task 12's session-progress wiring).
- Create: `crates/memoryd-tui/tests/correct_editor.rs` — pressing `k` during RC focus opens the editor; Esc returns to RC focus without dispatching; Ctrl-S with non-empty body dispatches `RealityCheckRequest::Respond { action: RealityCheckAction::Correct { new_body: ... } }` (asserted on the fixture daemon's received envelope); Ctrl-S with empty body shows an inline "body required" hint and does NOT dispatch; daemon ack advances RC focus to the next item.

```bash
git commit -m "feat(tui): inline correct editor for Reality Check, dispatches RealityCheckRequest::Correct"
```

---

### Task 14: STREAM_I_PLACEHOLDER trust-artifact lie fix (now lives in inspector's policy block)

**Parallel:** no (Cluster B — touches `crates/memoryd/src/main.rs` for ClaimLockRegistry wiring; sequence after Task 5)
**Blocked by:** Task 5
**Owned files:** `crates/memoryd/src/trust_artifact.rs:223`, `crates/memoryd/src/main.rs` (Cluster B — sequence after Task 5), `crates/memoryd/tests/trust_artifact_claim_lock.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test trust_artifact_claim_lock && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-14/` on `dogfood/task-14-trust-artifact`

**Note (v1.0).** The trust-artifact rendering surface in v1.0 lives in the inspector's policy block (`crates/memoryd-tui/src/inspector/fields.rs` reads `claim_lock_status` off the trust artifact). The fix in `crates/memoryd/src/trust_artifact.rs:223` is identical to v0.9; only its consumer changed. The existing daemon-level test (`crates/memoryd/tests/trust_artifact_claim_lock.rs`) is unchanged. The v0.9 widget-level test (`crates/memoryd-tui/tests/trust_artifact.rs`) is preserved by Task 11's "kept unchanged" set with constructor signature rewiring only.

**Files:**
- Modify: `crates/memoryd/src/trust_artifact.rs:223` — replace `STREAM_I_PLACEHOLDER` with real `claim_lock_status` lookup via `crates/memorum-coordination::ClaimLockRegistry::status_for(memory_id)`. When no lock exists, return `None`, NOT the misleading "Stream I not active" string.
- Modify: `crates/memoryd/src/main.rs` — wire ClaimLockRegistry handle into trust-artifact handler.
- Test: trust artifact with active lock shows lock holder + TTL; trust artifact with no lock shows `None`/blank, never the placeholder string.

```bash
git commit -m "fix(trust-artifact): real claim lock status, no Stream I lie"
```

---

### Task 14B: Theme presets, charset fallback, terminal-capability floor — TUI-side validation

**Parallel:** no (Cluster C, after Task 13)
**Blocked by:** Task 13
**Owned files:** `crates/memoryd-tui/tests/preset_smoke.rs` (new), `crates/memoryd-tui/tests/charset_fallback.rs` (new), `crates/memoryd-tui/tests/terminal_capability_floor.rs` (new), `crates/memoryd-tui/tests/theme_hot_reload_e2e.rs` (new), `docs/runbooks/tui-theming.md` (new — user-facing runbook covering `--theme`, `--charset`, `--no-motion`, the config file format, the six shipped presets, recommended fonts, hot-reload semantics, error-banner behavior on bad TOML)
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-tui --test preset_smoke --test charset_fallback --test terminal_capability_floor --test theme_hot_reload_e2e && cargo clippy -p memoryd-tui --tests -- -D warnings && cargo fmt -p memoryd-tui -- --check`
**Worktree:** `../agent-memory-wt/task-14B/` on `dogfood/task-14B-theme-validation`

**Why this task exists.** Task 10A unit-tests the theme crate in isolation; Task 11 verifies the inbox/inspector renders against `Theme::default_warm_dark()` and `Theme::for_test()`. Task 14B closes the loop: it renders the actual TUI shell against all six shipped presets and against the three terminal-capability tiers, verifying that no preset crashes the render path, no token resolves to an unsupported color in the 16-color floor, and the charset fallback substitutes glyphs without breaking row width.

**Files:**
- Create: `crates/memoryd-tui/tests/preset_smoke.rs` — for each preset name in `memorum_theme::presets::PRESETS`: load it, build an `App` with that theme + a populated fixture daemon, render with `TestBackend(140, 38)`, assert no panic and that all 6 inbox-item kinds render with their preset-specific glyph and accent token.
- Create: `crates/memoryd-tui/tests/charset_fallback.rs` — render with `Charset::Minimal`; assert ASCII-only output (no Unicode codepoint > 0x7F in the buffer except for explicitly allowed strings like memory titles); assert `BorderStyle::Plain` is forced regardless of theme setting.
- Create: `crates/memoryd-tui/tests/terminal_capability_floor.rs` — render with `ColorCapability::Indexed16`; assert no `Color::Rgb(_, _, _)` cells in the buffer (every cell is `Color::Named(_)` or `Color::Reset`); assert the layout is unchanged from the true-color render (capability degrades color, not structure).
- Create: `crates/memoryd-tui/tests/theme_hot_reload_e2e.rs` — write a TOML to a tempdir, start the App with `--theme-config <tempdir>/theme.toml`, render snapshot 1; modify the file (change `accent` token); **poll for theme advance every 50ms for up to 2s** (same pattern as `memorum-theme/tests/hot_reload.rs`), then render snapshot 2; assert the accent-colored cells differ between snapshots; write malformed TOML; poll for 2s confirming the receiver does NOT advance; render snapshot 3 and assert identical to snapshot 2 (hot-reload rejected the bad file) and the status line shows the error banner.
- Create: `docs/runbooks/tui-theming.md` — public runbook. Sections: `--theme`, `--theme-config`, `--charset`, `--no-motion`, `--color-capability` flag matrix (with the resolution precedence: CLI > `MEMORUM_FORCE_COLOR` env > auto-detect); config file path and TOML format with full token list; six shipped preset names with one-line descriptions and small swatches (described textually since the runbook is markdown); recommended fonts (JetBrains Mono, Berkeley Mono, MonoLisa, Iosevka, Cascadia Code) with notes on which glyphs render best; hot-reload semantics; error-banner behavior on bad TOML; instructions for authoring a custom preset; **a "common misdetection fixes" subsection** covering: tmux panes that report `TERM=screen-256color` when the host is true-color (use `--color-capability truecolor` or `MEMORUM_FORCE_COLOR=truecolor`); dumb-terminal SSH sessions where glyphs render as `?` (use `--charset minimal`); accent invisible because terminal palette is monochrome (use `--theme default-light` or set background-aware accent in a custom preset).

```bash
git commit -m "test(tui): preset smoke + charset fallback + 16-color floor + hot-reload e2e"
```

---

## Phase 5 — Web dashboard build-out

**Authoritative design contract:** `docs/design/dashboard-handoff/README.md` plus the React prototype in `docs/design/dashboard-handoff/src/`, `styles/`, and `tweaks-panel.jsx`. The handoff ships **7 primary views** (Inbox, Reality Check, Recall, Dreams, Peers, Governance, Entities) plus shared infrastructure (Shell, Inspector composition with 10 kinds, ~13 UI primitives, icon set, Tweaks panel). The CSS uses 6 themes (`warm-dark` default plus `warm-light`, `cool-dark`, `cool-light`, `monochrome`, `high-contrast`). A 25 KB design contract README documents tokens / components / surface states / keyboard / animations; ~32 KB of realistic mock data fixtures live in `docs/design/dashboard-handoff/src/data.js`. Every Task 17X acceptance criterion includes "matches the handoff" — visual, behavioral, and contract-level fidelity to the prototype is non-negotiable. The handoff README explicitly says "do not copy the inline-Babel `<script type='text/babel'>` setup, the global `Object.assign(window, …)` exports, or the `useStateXyz` hook aliases — those are artifacts of running React without a build step. Lift the markup, layout, design tokens, copy, and interaction patterns; rebuild the wiring idiomatically." Task 17X workers MUST honor that.

**Cluster D-backend (Tasks 15, 16):** sequential 16 → 15 (server.rs route registration order). Both stay as Rust handler-body work; no frontend touch.

**Cluster D-frontend (Tasks 17A through 17K):** the React + Vite + TypeScript build-out. 17A first (foundation); 17B / 17C / 17D parallel-safe after 17A; 17E / 17F / 17G / 17H sequential through `frontend/src/views.ts`; 17I / 17J / 17K final integration. See "Inter-task coordination" above for the full sequence rationale.

**Task 18 (capture redaction):** independent of both subclusters; relocated to `crates/memory-source/` in v0.5.

---

### Task 15: Web entity graph + entity detail endpoints

**Parallel:** no (Cluster D — sequenced after Task 16, which also modifies `server.rs`)
**Blocked by:** Task 5, Task 16, Task 11A (uses the new `InspectEntities` daemon payload)
**Owned files:** `crates/memoryd-web/src/routes/entity_graph.rs:117,127`, `crates/memoryd-web/src/routes/entity_detail.rs` (split from existing if combined), `crates/memoryd-web/tests/entity_endpoints.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-web --test entity_endpoints && cargo clippy -p memoryd-web --tests -- -D warnings && cargo fmt -p memoryd-web -- --check`
**Worktree:** `../agent-memory-wt/task-15/` on `dogfood/task-15-web-entities`

**Framing note (v0.5).** The route paths `/api/entity-graph` and `/api/entity-graph/{entity_id}` already exist as registered routes in `crates/memoryd-web/src/server.rs:185-186` calling `entity_graph` / `entity_detail` handlers in `routes/entity_graph.rs:117, 127`. Today those handlers return fixture data via the deferred-response pattern. This task **replaces the fixture handler bodies with real daemon-backed implementations** — it does not register new routes, and `server.rs` is touched only in case of follow-up coordination with Task 16, which already plans to share that file.

**Files:**
- Modify: `crates/memoryd-web/src/routes/entity_graph.rs:117` — replace `deferred_response("entity_graph")` with real handler: query daemon for entity graph (JSON: `{ nodes: [{id, label, kind, count}], edges: [{from, to, kind, weight}] }`), return CSP-safe JSON.
- Modify: `crates/memoryd-web/src/routes/entity_graph.rs:127` (or split) — `entity_detail` returns `{ entity_id, mentions: [...], related_memories: [...], first_seen, last_seen }`.
- Test: empty graph returns `{ nodes: [], edges: [] }` (200, not 501); populated graph returns honest data.

```bash
git commit -m "feat(web): entity graph + entity detail endpoints"
```

---

### Task 16: Web policy editor + sync dashboard + reality-check history endpoints

**Parallel:** no (Cluster D — sequenced before Task 15; both touch `crates/memoryd-web/src/server.rs`. v0.7 fix: prior wording said "Parallel: yes (with Task 15)" while Task 15 listed Task 16 as a `Blocked by`, contradicting itself. Cluster D order is **16 → 15 → 17**, with Task 18 outside the cluster.)
**Blocked by:** Task 5
**Owned files:** `crates/memoryd-web/src/routes/policy_editor.rs` (new file split from `server.rs:196-197`), `crates/memoryd-web/src/routes/sync_dashboard.rs` (new), `crates/memoryd-web/src/routes/reality_check.rs:120` (history fall-through), `crates/memoryd-web/src/server.rs:196-197` (route registration only — non-conflicting with Task 15), `crates/memoryd-web/tests/dashboard_endpoints.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd-web --test dashboard_endpoints && cargo clippy -p memoryd-web --tests -- -D warnings && cargo fmt -p memoryd-web -- --check`
**Worktree:** `../agent-memory-wt/task-16/` on `dogfood/task-16-web-dashboard-routes`

**Coordination with Task 15:** `server.rs` route registration is touched by both. Both tasks add `Router::route()` calls; conflicts are resolvable by accepting both sets of additions. Task 16 ships first per dependency ordering, Task 15 rebases.

**CSRF wiring (v0.6 fix).** v0.5 said "POST validates + writes (CSRF-protected, localhost-only enforcement already in place)" but did not specify where the POST route registers in the existing CSRF middleware merge. `crates/memoryd-web/src/server.rs` builds two routers and merges them: a public-GET router and a `protected_post_routes` block wrapped in `axum::middleware::from_fn_with_state(state.clone(), require_csrf)` (line ~177) which is then `.merge(protected_post_routes)` into the main router (line 198). Existing CSRF-protected POSTs are `/api/reality-check/respond` and `/api/review/action`. v0.6 fix: Task 16 explicitly registers `/api/policy-editor` POST (the YAML-write handler) inside `protected_post_routes` so it inherits the CSRF middleware automatically. The GET stays in the public router.

**Files:**
- Create: `crates/memoryd-web/src/routes/policy_editor.rs` — exports two handlers: `pub async fn policy_editor_get(...) -> ...` returning current governance policy YAML; `pub async fn policy_editor_post(...) -> ...` validating the submitted YAML against the governance policy schema and writing to disk (atomic rename). Defense-in-depth: the existing `require_csrf` middleware enforces the same-origin token check (mitigating CSRF from a foreign origin), and the daemon's existing 127.0.0.1 bind in `crates/memoryd-web/src/server.rs` (combined with the operator-controlled `web enable` lifecycle) keeps the dashboard off the network. **The two protections do different things** — CSRF is about request provenance, the localhost bind is about reachability — and the prior wording "localhost-only by virtue of CSRF" was sloppy; both layers must hold.
- Create: `crates/memoryd-web/src/routes/sync_dashboard.rs` — GET returns sync state (last commit, ahead/behind, peer presence summary, claim lock summary).
- Modify: `crates/memoryd-web/src/routes/reality_check.rs:120` — `reality_check_history` proxies to daemon when socket live; falls back gracefully when not.
- Modify: `crates/memoryd-web/src/server.rs` — (a) inside `protected_post_routes` (around line 175-178, where `/api/reality-check/respond` and `/api/review/action` already live), add `.route("/api/policy-editor", post(policy_editor_post))`; (b) inside the public-GET router (around line 196-197, where the deferred-response stubs currently live), replace `.route("/api/policy-editor", get(|| async { deferred_response("policy_editor"... }))` with `.route("/api/policy-editor", get(policy_editor_get))`, and similarly replace the `/api/sync-dashboard` deferred stub with the real `sync_dashboard` handler. Remove the deferred 501 stubs. The CSRF protection is then automatic for the POST and absent for the GET, matching the existing pattern.

```bash
git commit -m "feat(web): policy editor, sync dashboard, reality-check history"
```

---

### Task 17A: Frontend toolchain bootstrap + e2e harness scaffolding

**Parallel:** no (foundation for all 17B–17K)
**Blocked by:** Task 16 (Cluster D-backend lands first so the routes 17I will hit are real before frontend wiring begins)
**Owned files:** `crates/memoryd-web/frontend/` (entire new directory tree — `package.json`, `pnpm-lock.yaml`, `tsconfig.json`, `vite.config.ts`, `eslint.config.js`, `.prettierrc`, `playwright.config.ts`, `vitest.config.ts`, `index.html`, `src/main.tsx`, `src/App.tsx`, `src/views.ts` stub, `tests/e2e/smoke.spec.ts`, `tests/setup.ts`), `crates/memoryd-web/build.rs` (new), `crates/memoryd-web/Cargo.toml` (build-dependencies + build = "build.rs"), `crates/memoryd-web/src/server.rs` (rust-embed retarget at lines 33-35; deletion of `app_js()` and `style_css()` named handlers at ~252-258; deletion of `.route("/assets/app.js", get(app_js))` and `.route("/assets/style.css", get(style_css))` from the router at ~181-182 — verified against the actual file by plan-reviewer in v1.3), `crates/memoryd-web/static/` (DELETE — old vanilla shell removed in this task), `crates/memoryd-web/tests/frontend_smoke.rs` (CREATE — does not exist today; v1.2 said "Modify" which was wrong, plan-reviewer caught this in v1.3), `.gitignore` (append `crates/memoryd-web/frontend/node_modules/`, `crates/memoryd-web/frontend/dist/`, `crates/memoryd-web/frontend/playwright-report/`, `crates/memoryd-web/frontend/test-results/`, `crates/memoryd-web/frontend/.vite/`)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm install --frozen-lockfile && pnpm run check:all && cd ../../.. && cargo build -p memoryd-web --locked && cargo test -p memoryd-web --test frontend_smoke && cargo clippy -p memoryd-web --tests -- -D warnings && cargo fmt -p memoryd-web -- --check`

The `pnpm run check:all` script defined in `package.json` runs `pnpm run lint && pnpm run typecheck && pnpm run test --run && pnpm exec playwright install --with-deps chromium && pnpm run test:e2e -- --reporter=list` so the gate stays a single shell command.

**Worktree:** `../agent-memory-wt/task-17A/` on `dogfood/task-17A-frontend-bootstrap`

**Architecture.** The shipped `crates/memoryd-web/static/` tree is 3 vanilla files (`index.html` 38 lines, `style.css` 35 lines, `app.js` 7 lines — verified via `wc -l crates/memoryd-web/static/*`). The Claude Design handoff at `docs/design/dashboard-handoff/` is a 9-component React prototype. Task 17A creates a real React + Vite + TypeScript pipeline that builds production assets into `frontend/dist/`, retargets `rust-embed` from `static/` to `frontend/dist/`, and integrates the build into cargo via a `build.rs` that invokes `pnpm install --frozen-lockfile && pnpm run build` before Rust compilation. The deployed binary is still a single Rust binary (rust-embed reads at compile time); the only new toolchain dependency is pnpm + Node at *build* time, not at runtime. Static asset URL surface (`/assets/app.js`, `/assets/style.css` singular) MUST be preserved unless the rust-embed handler is updated correspondingly — Vite's content-hashed bundles emit names like `assets/index-a1b2c3.js` so the static asset routes in `crates/memoryd-web/src/server.rs` need a glob-style match against the build manifest. The handler change is in this task's owned files.

**Files:**
- Create: `crates/memoryd-web/frontend/package.json` — name `memorum-dashboard`, private, type `module`. Dependencies: `react@^18.3`, `react-dom@^18.3`, `@tanstack/react-virtual@^3.10` (mandated up-front so Task 17G's heavy-data Recall ledger uses it without re-adding deps in a later task — orchestrator-merged-lockfile risk minimized by landing all runtime deps in 17A). Dev dependencies: `typescript@^5.6`, `vite@^5.4`, `@vitejs/plugin-react@^4.3`, `eslint@^9`, `@typescript-eslint/eslint-plugin`, `@typescript-eslint/parser`, `eslint-plugin-react`, `eslint-plugin-react-hooks`, `prettier`, `vitest@^2`, `@testing-library/react@^16`, `@testing-library/jest-dom`, `jsdom`, `@playwright/test@^1.48`, `@axe-core/playwright`, `msw@^2.4`. Scripts: `dev`, `build`, `preview`, `lint`, `lint:fix`, `format`, `format:check`, `typecheck`, `test`, `test:visual`, `test:e2e`, `test:a11y`, `check:all` (composes lint + typecheck + unit + e2e). Pin pnpm version in `packageManager` field.
- Create: `crates/memoryd-web/frontend/pnpm-lock.yaml` — generated by `pnpm install` during task; committed.
- Create: `crates/memoryd-web/frontend/tsconfig.json` — strict TS, `target: ES2022`, `module: ESNext`, `moduleResolution: bundler`, `jsx: react-jsx`, `noUnusedLocals`, `noUnusedParameters`, `noImplicitOverride`, `exactOptionalPropertyTypes`.
- Create: `crates/memoryd-web/frontend/vite.config.ts` — React plugin, `build.outDir: "dist"`, `build.assetsDir: "assets"`, `server.proxy` for `/api` → `http://127.0.0.1:7137` (dev daemon), `build.rollupOptions.output.manualChunks` to keep CSS in a single bundle and JS within budget. Bundle manifest emitted for the rust-embed glob match.
- Create: `crates/memoryd-web/frontend/eslint.config.js` — flat-config, React 18 rules, Hooks rules, TypeScript rules. No `any`, no `unknown` without narrowing. `@typescript-eslint/no-explicit-any: error`.
- Create: `crates/memoryd-web/frontend/.prettierrc` — 2-space indent, single quotes, no semicolons-OFF (use semicolons), trailing commas all, print width 100.
- Create: `crates/memoryd-web/frontend/playwright.config.ts` — chromium project only (Linux/macOS CI parity), `webServer: { command: "pnpm run dev", port: 5173, reuseExistingServer: !process.env.CI }`, `use.baseURL: "http://localhost:5173"`, screenshot/trace on failure, retries 0 locally / 1 in CI. **Cross-platform snapshot path:** set `snapshotPathTemplate: "{testDir}/__snapshots__/{platform}/{testFilePath}/{arg}{ext}"` so visual baselines are scoped per OS (macOS dev baselines never collide with Linux CI baselines). The CI workflow regenerates Linux baselines on first run via `pnpm run test:visual --update-snapshots` and commits them; subsequent runs diff. Document this in `frontend/README.md` and the v1.3 revision-history entry. **Tolerance:** `expect.toHaveScreenshot.maxDiffPixelRatio: 0.01` (1%) — bumped from v1.2's 0.005 per plan-reviewer R1, because OKLCH gamut-mapping and font hinting differ enough between Chrome on macOS (P3-aware) and Chrome on Linux (sRGB) that 0.5% absorbs noise but fails on real cross-platform drift.
- Create: `crates/memoryd-web/frontend/vitest.config.ts` — jsdom environment, setup file `tests/setup.ts`, `globals: true`, coverage thresholds (lines 80% / functions 80% / branches 75% / statements 80%). Visual regression via `vitest`-driven `toMatchSnapshot()` plus a separate `test:visual` script that runs Playwright's `toHaveScreenshot()` for full-page captures.
- Create: `crates/memoryd-web/frontend/index.html` — entry HTML with `<meta name="csrf-token" content="__MEMORUM_CSRF_TOKEN__">` so the existing CSRF token-rewrite path in `crates/memoryd-web/src/server.rs` continues to work. CSP-strict: no inline scripts, no inline styles, only Vite-built bundles loaded from `/assets/`. `<title>Memorum Dashboard</title>`. `data-theme="warm-dark"` on `<html>` as initial-state default (overridden by ThemeProvider in 17B).
- Create: `crates/memoryd-web/frontend/src/main.tsx` — React root mount. Reads `data-theme` initial state. Imports `./App.tsx`.
- Create: `crates/memoryd-web/frontend/src/App.tsx` — minimal "Hello Memorum" shell that renders the brand sigil + wordmark. Real shell ports in 17C.
- Create: `crates/memoryd-web/frontend/src/views.ts` — empty view registry stub; tasks 17E–17H register their views here.
- Create: `crates/memoryd-web/frontend/tests/setup.ts` — `@testing-library/jest-dom` extension, jsdom matchMedia polyfill (for OS-pref detection in 17B).
- Create: `crates/memoryd-web/frontend/tests/e2e/smoke.spec.ts` — Playwright smoke: visit `/`, assert `<title>` reads `Memorum Dashboard`, assert `<meta name="csrf-token">` present, assert `data-theme="warm-dark"` on `<html>`.
- Create: `crates/memoryd-web/build.rs` — invokes `pnpm install --frozen-lockfile` and `pnpm run build` inside `crates/memoryd-web/frontend/` before Rust compilation. Use `std::process::Command` with explicit `current_dir`. On macOS/Linux, prefer `corepack pnpm` if available; fall back to `pnpm`. On failure, emit `cargo:warning=` and exit non-zero. Add `cargo:rerun-if-changed=frontend/src`, `cargo:rerun-if-changed=frontend/styles`, `cargo:rerun-if-changed=frontend/index.html`, `cargo:rerun-if-changed=frontend/package.json`, `cargo:rerun-if-changed=frontend/vite.config.ts` so the frontend rebuilds on real source changes (NOT on every cargo invocation).
- Modify: `crates/memoryd-web/Cargo.toml` — add `[build-dependencies]` section if absent (no actual deps needed beyond stdlib for the build.rs); ensure `rust-embed` is at the existing version. Add `build = "build.rs"` to `[package]` if not already implied.
- Modify: `crates/memoryd-web/src/server.rs:33-35` — change the existing `#[derive(RustEmbed)] #[folder = "static/"] struct Assets;` annotation to `#[folder = "frontend/dist/"]`. The `Assets` struct keeps the same name; only the folder pointer changes. Verified locations: line 33 declares `#[derive(RustEmbed)]`, line 34 declares `#[folder = "static/"]`, line 35 declares `struct Assets;`.
- Modify: `crates/memoryd-web/src/server.rs` (router around lines 181-182) — DELETE the two hardcoded routes `.route("/assets/app.js", get(app_js))` and `.route("/assets/style.css", get(style_css))`. The wildcard route `.route("/assets/{*path}", get(asset))` at line 183 already serves any `/assets/<filename>` path through `embedded_response(&path, content_type_for(&path))`, which works correctly for Vite's content-hashed filenames (e.g., `index-a1b2c3.js`, `index-d4e5f6.css`).
- Modify: `crates/memoryd-web/src/server.rs` (around lines 252-258) — DELETE the now-unused named handler functions `async fn app_js()` and `async fn style_css()`. Failing to delete them would trip `cargo clippy -- -D warnings` on dead code. The `asset()` and `embedded_response()` helpers stay; they handle all `/assets/*` traffic post-retarget. The `index()` handler at line ~245 still serves `/` and rewrites `__MEMORUM_CSRF_TOKEN__`; that path is unchanged because Vite emits an `index.html` at the dist root and the rewrite contract is preserved.
- Modify: `crates/memoryd-web/src/server.rs` — also delete the now-unreferenced `APP_JS` and `STYLE_CSS` constants if they exist near the top of the file (run `rg "APP_JS|STYLE_CSS" crates/memoryd-web/src/server.rs` first to locate; if they're inlined into the handlers being deleted, no separate constant cleanup needed).
- Delete: `crates/memoryd-web/static/index.html`, `crates/memoryd-web/static/style.css`, `crates/memoryd-web/static/app.js`, `crates/memoryd-web/static/` (entire directory). Verify with `git status` no `static/` files remain tracked.
- Create: `crates/memoryd-web/tests/frontend_smoke.rs` — NEW file (does not exist today). The existing test directory contains only `api_contract.rs`, `concurrent_access.rs`, `csrf.rs`. This new test file boots a fixture daemon via `memoryd_web::fixture_router()`, fetches `/`, and asserts: (a) the embedded bundle includes a content-hashed `index-*.js` asset (via `Assets::iter()` enumeration), (b) the embedded bundle includes a content-hashed `index-*.css` asset, (c) the served `/` includes `<meta name="csrf-token" content="<token>">` with the placeholder rewritten to a real token, (d) no inline `<script>` blocks (CSP-strict — regex match), (e) no inline `<style>` blocks (CSP-strict — regex match), (f) the served HTML's `<title>` reads `Memorum Dashboard`. Uses the same `tower::ServiceExt` test pattern as the existing `api_contract.rs`. Task 17K extends this same file with bundle-budget assertions (gzipped CSS ≤ 80 KB, gzipped JS ≤ 250 KB).
- Modify: `.gitignore` — append the five frontend ignore lines.
- Create: `crates/memoryd-web/frontend/README.md` — frontend dev quickstart (`pnpm install`, `pnpm run dev`, `pnpm run check:all`, where the production build lands, how rust-embed picks it up).

**Step 1: Author every file above with stub content.** The `App.tsx` renders `<div>memorum dashboard — bootstrap</div>`. The Playwright smoke test passes against that minimal app.

**Step 2: Run per-task gate.** Expected: green. Both `cd crates/memoryd-web/frontend && pnpm run check:all` (Vitest unit / typecheck / lint / Playwright smoke) and the Rust gate (`cargo build --locked` triggers build.rs which triggers pnpm build, then `cargo test --test frontend_smoke` exercises the embedded bundle).

**Step 3: Commit.**
```bash
git add crates/memoryd-web/frontend crates/memoryd-web/build.rs crates/memoryd-web/Cargo.toml crates/memoryd-web/src/main.rs crates/memoryd-web/tests/frontend_smoke.rs .gitignore
git rm -r crates/memoryd-web/static
git commit -m "feat(web): React + Vite + TypeScript frontend toolchain bootstrap with e2e harness"
```

---

### Task 17B: Port design tokens + theme infrastructure + visual regression baseline

**Parallel:** yes (with 17C, 17D — disjoint files)
**Blocked by:** Task 17A
**Owned files:** `crates/memoryd-web/frontend/src/styles/tokens.css` (new — verbatim copy from handoff), `crates/memoryd-web/frontend/src/styles/app.css` (new — verbatim copy from handoff), `crates/memoryd-web/frontend/src/theme/ThemeProvider.tsx` (new), `crates/memoryd-web/frontend/src/theme/types.ts` (new), `crates/memoryd-web/frontend/src/theme/storage.ts` (new — localStorage + matchMedia OS-pref reader), `crates/memoryd-web/frontend/src/theme/index.ts` (new — barrel exports), `crates/memoryd-web/frontend/tests/theme/ThemeProvider.test.tsx` (new — Vitest unit), `crates/memoryd-web/frontend/tests/visual/themes.spec.ts` (new — Playwright visual baseline, 6 themes × 2 densities = 12 snapshots), `crates/memoryd-web/frontend/tests/visual/__snapshots__/themes/` (12 baseline PNGs)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run theme && pnpm run test:visual --run themes`
**Worktree:** `../agent-memory-wt/task-17B/` on `dogfood/task-17B-tokens-themes`

**Token discipline.** The handoff's `tokens.css` is the canonical token contract — copy it verbatim, do not "improve" hue/lightness/chroma values. The 6 themes (`warm-dark` default, `warm-light`, `cool-dark`, `cool-light`, `monochrome`, `high-contrast`) attach to `<html data-theme="…">`. Density (`comfortable` default, `compact`) attaches to `<html data-density="…">`. Reduced motion is 3-way (`os` default, `on`, `off`) and attaches to `<html data-reduced-motion="…">` (the `os` value resolves at runtime via `matchMedia('(prefers-reduced-motion: reduce)')`).

**Files:**
- Create: `crates/memoryd-web/frontend/src/styles/tokens.css` — verbatim copy of `docs/design/dashboard-handoff/styles/tokens.css`. Single-line diff allowed only if the handoff's font references need URL rewrites for the bundled font files (which 17A imports).
- Create: `crates/memoryd-web/frontend/src/styles/app.css` — verbatim copy of `docs/design/dashboard-handoff/styles/app.css`. Imports `tokens.css` at the top.
- Create: `crates/memoryd-web/frontend/src/theme/types.ts` — `type Theme = 'warm-dark' | 'warm-light' | 'cool-dark' | 'cool-light' | 'monochrome' | 'high-contrast'`; `type Density = 'comfortable' | 'compact'`; `type ReducedMotion = 'os' | 'on' | 'off'`; `interface ThemePreferences { theme: Theme; density: Density; reducedMotion: ReducedMotion }`.
- Create: `crates/memoryd-web/frontend/src/theme/storage.ts` — `loadPreferences(): ThemePreferences` reads `localStorage.getItem('memorum.theme')`, `'memorum.density'`, `'memorum.reducedMotion'` with defaults; `savePreferences(p: ThemePreferences): void` writes; `resolveReducedMotion(setting: ReducedMotion): boolean` reads `matchMedia` when setting is `'os'`.
- Create: `crates/memoryd-web/frontend/src/theme/ThemeProvider.tsx` — React context exposing `{ preferences, setTheme, setDensity, setReducedMotion }`. Effect: writes `data-theme`, `data-density`, `data-reduced-motion` attributes on `<html>` whenever preferences change. Persists via `storage.ts`. Listens to `matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', ...)` to react to OS-pref changes when setting is `'os'`.
- Create: `crates/memoryd-web/frontend/src/theme/index.ts` — barrel.
- Modify: `crates/memoryd-web/frontend/src/App.tsx` — wrap render tree in `<ThemeProvider>`. Show a placeholder with body copy + a heading + a sample inline-code span, enough to give the visual baseline real content to snapshot.
- Test: `crates/memoryd-web/frontend/tests/theme/ThemeProvider.test.tsx` — 8 tests covering: initial-state from defaults; localStorage round-trip per token; `data-theme` attribute write-through; OS-pref reduced-motion resolves correctly when setting is `'os'`; OS-pref change event triggers re-resolve; setting `'on'`/`'off'` overrides OS pref; invalid stored value falls back to default; `useTheme()` outside provider throws.
- Test: `crates/memoryd-web/frontend/tests/visual/themes.spec.ts` — Playwright. Iterates `themes × densities` (6 × 2 = 12), navigates to `/?theme=<t>&density=<d>` (App.tsx parses query params for visual-regression scaffolding), asserts `<html data-theme="<t>" data-density="<d>">`, calls `await expect(page).toHaveScreenshot(`{theme}-{density}.png`, { fullPage: true, animations: 'disabled' })`. First run produces baseline; subsequent runs diff. Tolerance: `maxDiffPixelRatio: 0.005` to absorb font-rendering subpixel jitter.

**Step 1: Copy tokens.css and app.css verbatim. Author ThemeProvider + storage + types. Wire in App.tsx.**

**Step 2: Write the 8 ThemeProvider unit tests + visual baseline spec.**

**Step 3: Run gate twice.** **Gate run 1:** visual baseline doesn't exist yet — Playwright auto-generates baselines on first run and the assertion vacuously passes. The worker MUST run the gate a second time. **Gate run 2:** all 12 snapshots must diff-match the just-generated baselines (zero diff against self). Both runs must pass before commit. If the second run shows non-zero diff, investigate determinism (check for animation, time-of-day, randomness in mock data) before regenerating. Document the regenerate-baseline command in `frontend/README.md`: `pnpm run test:visual --update-snapshots`. The two-pass requirement is explicit because v1.2's wording was implicit and plan-reviewer N2 caught it.

```bash
git add crates/memoryd-web/frontend/src/styles crates/memoryd-web/frontend/src/theme crates/memoryd-web/frontend/src/App.tsx crates/memoryd-web/frontend/tests/theme crates/memoryd-web/frontend/tests/visual
git commit -m "feat(web): design tokens + 6-theme infrastructure + visual regression baseline"
```

---

### Task 17C: Port Shell + UI primitives + icons

**Parallel:** yes (with 17B, 17D — disjoint files)
**Blocked by:** Task 17A
**Owned files:** `crates/memoryd-web/frontend/src/shell/Shell.tsx` (new — port of handoff's `Shell.jsx`), `crates/memoryd-web/frontend/src/shell/Sidebar.tsx`, `crates/memoryd-web/frontend/src/shell/TopBar.tsx`, `crates/memoryd-web/frontend/src/shell/Footer.tsx`, `crates/memoryd-web/frontend/src/ui/Pill.tsx`, `crates/memoryd-web/frontend/src/ui/Badge.tsx`, `crates/memoryd-web/frontend/src/ui/Card.tsx`, `crates/memoryd-web/frontend/src/ui/EmptyState.tsx`, `crates/memoryd-web/frontend/src/ui/StatusDot.tsx`, `crates/memoryd-web/frontend/src/ui/Toast.tsx`, `crates/memoryd-web/frontend/src/ui/Banner.tsx`, `crates/memoryd-web/frontend/src/ui/Modal.tsx`, `crates/memoryd-web/frontend/src/ui/ListRow.tsx`, `crates/memoryd-web/frontend/src/ui/index.ts` (barrel), `crates/memoryd-web/frontend/src/icons/index.tsx` (port of handoff's `icons.jsx`), `crates/memoryd-web/frontend/tests/ui/*.test.tsx` (one test file per primitive — 11 files), `crates/memoryd-web/frontend/tests/visual/primitives.spec.ts` (Playwright visual: each primitive × 6 themes), plus `__snapshots__/primitives/` baseline tree
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run ui shell icons && pnpm run test:visual --run primitives`
**Worktree:** `../agent-memory-wt/task-17C/` on `dogfood/task-17C-shell-primitives`

**Translation discipline.** Read handoff's `Shell.jsx`, `UI.jsx`, `icons.jsx` end to end. Port each component to TSX with: (a) explicit prop types (no `any`); (b) `React.FC<Props>` or function-component returning `JSX.Element`; (c) tokens referenced as CSS variables (`var(--accent)`) inside `className`-driven styles, never inline literal colors; (d) zero `Object.assign(window, …)`; (e) no `useStateXyz` aliases — use real `useState`. Keyboard handlers from the handoff (`useKeyDown`, etc.) get re-implemented as proper React hooks. ARIA roles preserved. The handoff's CSS classes survive verbatim because `app.css` was copied verbatim in 17B.

**Files:**
- Create: `crates/memoryd-web/frontend/src/icons/index.tsx` — Phosphor-style stroke-1.5 SVG components for nav (`InboxIcon`, `RealityCheckIcon`, `RecallIcon`, `DreamsIcon`, `PeersIcon`, `GovernanceIcon`, `EntitiesIcon`, `SettingsIcon`) plus shell affordances (`SearchIcon`, `BellIcon`, `CommandIcon`). All take `{ size?: number; className?: string }`.
- Create: `crates/memoryd-web/frontend/src/shell/Sidebar.tsx` — 220px sidebar, brand block, nav list, active-item 2px accent inset. Selectable nav items take `{ id, label, icon, count?, shortcut, active, onSelect }`.
- Create: `crates/memoryd-web/frontend/src/shell/TopBar.tsx` — 44px top bar, brand sigil block, global search input (forwards `/` keypress), command palette trigger, notification bell with unread dot, daemon status pill.
- Create: `crates/memoryd-web/frontend/src/shell/Footer.tsx` — 28px footer, view-name + selected-meta + keystroke hints. Hints rotate per active view via `keymap` prop.
- Create: `crates/memoryd-web/frontend/src/shell/Shell.tsx` — composes Sidebar + TopBar + content slot + Footer.
- Create: `crates/memoryd-web/frontend/src/ui/Pill.tsx` — filter chip with label + count. Active state via `--surface-2` bg, count colored `--accent` only when active.
- Create: `crates/memoryd-web/frontend/src/ui/Badge.tsx` — inline status (`candidate`, `plaintext`, etc.). Variant prop maps to semantic colors.
- Create: `crates/memoryd-web/frontend/src/ui/ListRow.tsx` — three-column grid (glyph / title+sub / meta). `selected` prop renders 2px accent inset; `focused` prop renders inset 1px ring; both can stack (selected+focused). `onClick` for selection, `tabIndex` for focus.
- Create: `crates/memoryd-web/frontend/src/ui/Card.tsx` — surface wrapper with hairline border. Header slot, body slot.
- Create: `crates/memoryd-web/frontend/src/ui/EmptyState.tsx` — centered icon + heading + body + optional action. No emojis, no decoration.
- Create: `crates/memoryd-web/frontend/src/ui/StatusDot.tsx` — 8px circle, `kind` prop (`ok`/`warn`/`bad`/`info`/`muted`), always paired with a label via `children`.
- Create: `crates/memoryd-web/frontend/src/ui/Toast.tsx` — bottom-center, auto-dismiss 4s default, manual close. Singleton via context (`ToastProvider`).
- Create: `crates/memoryd-web/frontend/src/ui/Banner.tsx` — top-of-page persistent banner. Used by daemon-down, sync-conflict states.
- Create: `crates/memoryd-web/frontend/src/ui/Modal.tsx` — single-shadow dialog, focus-trap on open, Esc-to-close, click-outside-to-close opt-in.
- Create: `crates/memoryd-web/frontend/src/ui/index.ts` — barrel.
- Test: 11 component test files. Each covers default render, all states (default/hover/focus/selected/disabled where applicable), keyboard interaction (Esc/Enter/Tab where applicable), token resolution (asserts `getComputedStyle` returns the expected CSS variable). Total ~80 unit tests minimum.
- Test: `crates/memoryd-web/frontend/tests/visual/primitives.spec.ts` — Playwright visual. For each primitive, render at default state in each of 6 themes (66 snapshots: 11 primitives × 6 themes). Snapshot tolerance same as 17B.

**Step 1-3:** TDD — write tests first for one primitive, port the primitive, repeat per-primitive. Visual baseline regenerates at end.

```bash
git add crates/memoryd-web/frontend/src/shell crates/memoryd-web/frontend/src/ui crates/memoryd-web/frontend/src/icons crates/memoryd-web/frontend/tests/ui crates/memoryd-web/frontend/tests/visual/primitives.spec.ts
git commit -m "feat(web): port shell, UI primitives, and icon set with full visual regression"
```

---

### Task 17D: Port Inspector composition

**Parallel:** yes (with 17B, 17C — disjoint files)
**Blocked by:** Task 17A
**Owned files:** `crates/memoryd-web/frontend/src/inspector/Inspector.tsx` (new — port of handoff's `Inspector.jsx`), `crates/memoryd-web/frontend/src/inspector/cards/` (new directory: `ProvenanceCard.tsx`, `PolicyCard.tsx`, `PrivacyScanCard.tsx`, `PolicyDecisionTraceCard.tsx`, `EvidenceCard.tsx`, `DreamPassCard.tsx`, `EvidenceSummaryCard.tsx`, `SessionsCard.tsx`, `ClaimLocksCard.tsx`, `ConnectionCard.tsx`, `TrafficCard.tsx`, `DisagreementCard.tsx`, `EntityCard.tsx`, `CoOccurringCard.tsx`, `RecentMemoriesCard.tsx`), `crates/memoryd-web/frontend/src/inspector/kinds/` (new directory: one file per inspector kind — `inboxReview.tsx`, `inboxRecall.tsx`, `inboxConflict.tsx`, `inboxDue.tsx`, `inboxDream.tsx`, `recallEvent.tsx`, `dreamOutput.tsx`, `peerDetail.tsx`, `governanceDecision.tsx`, `entityDetail.tsx`), `crates/memoryd-web/frontend/src/inspector/index.ts` (barrel), `crates/memoryd-web/frontend/src/inspector/types.ts` (kind discriminated-union), `crates/memoryd-web/frontend/tests/inspector/*.test.tsx` (one per kind), `crates/memoryd-web/frontend/tests/visual/inspector.spec.ts` (10 kinds × 6 themes = 60 snapshots)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run inspector && pnpm run test:visual --run inspector`
**Worktree:** `../agent-memory-wt/task-17D/` on `dogfood/task-17D-inspector`

**Composition contract.** The Inspector is the dashboard's primary detail surface. It MUST adapt to item kind via a discriminated union: `type InspectorItem = { kind: 'inbox-review'; … } | { kind: 'recall-event'; … } | …` (10 kinds total, enumerated in `types.ts`). The Inspector dispatches to the appropriate `kinds/*.tsx` renderer based on `item.kind`. Each kind renders a header (title + scope + badges + memory id), a body section, kind-specific action cards, and stacked metadata cards from `cards/*.tsx`.

The handoff's `Inspector.jsx` already encodes the full composition matrix; port it verbatim. Where the handoff uses inline JSX for cards, extract to dedicated card components in `inspector/cards/` so other views (Reality Check, Recall, Dreams, Peers, Governance, Entities) can compose the same cards.

**Files:** (15 card components + 10 kind dispatchers + Inspector + types + barrel) — every card takes typed props, returns `JSX.Element`, renders via `app.css` classes from 17B, no inline literal colors. Component tests assert: default render, all kind dispatches, action button keyboard contract (`a`/`r`/`e`/`f` etc. wired), token resolution. Visual snapshot per kind × 6 themes.

**Step 1-3:** TDD per inspector kind — write the kind's test first, port the dispatcher and any new cards, run gate.

```bash
git add crates/memoryd-web/frontend/src/inspector crates/memoryd-web/frontend/tests/inspector crates/memoryd-web/frontend/tests/visual/inspector.spec.ts
git commit -m "feat(web): port Inspector composition with 10 kind variants and 15 metadata cards"
```

---

### Task 17E: Port Inbox view (4 layout variants)

**Parallel:** no (Cluster D-frontend sequential through `views.ts`)
**Blocked by:** Task 17B, Task 17C, Task 17D
**Owned files:** `crates/memoryd-web/frontend/src/views/Inbox.tsx` (new — port of handoff's `Inbox.jsx`, all 4 layout variants), `crates/memoryd-web/frontend/src/views/inbox/FilterPills.tsx`, `crates/memoryd-web/frontend/src/views/inbox/InboxList.tsx`, `crates/memoryd-web/frontend/src/views/inbox/layouts/TwoPane.tsx`, `crates/memoryd-web/frontend/src/views/inbox/layouts/ThreePane.tsx`, `crates/memoryd-web/frontend/src/views/inbox/layouts/Drawer.tsx`, `crates/memoryd-web/frontend/src/views/inbox/layouts/ModalSheet.tsx`, `crates/memoryd-web/frontend/src/views/inbox/index.ts`, `crates/memoryd-web/frontend/src/views.ts` (register `inbox` view), `crates/memoryd-web/frontend/tests/views/Inbox.test.tsx` (component), `crates/memoryd-web/frontend/tests/e2e/inbox.spec.ts` (Playwright e2e), `crates/memoryd-web/frontend/tests/visual/inbox.spec.ts` (visual: 4 layouts × 6 themes = 24 snapshots)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run inbox && pnpm run test:visual --run inbox && pnpm run test:e2e -- --grep inbox`
**Worktree:** `../agent-memory-wt/task-17E/` on `dogfood/task-17E-inbox-view`

**Layout discipline.** Layout variant is a prop (`layout: 'two-pane' | 'three-pane' | 'drawer' | 'modal'`) controlled by Tweaks panel preference. Drawer DEFAULTS OPEN (corrects the handoff prototype's drawer-closed default per pre-handoff design review). Modal exists as the "see why this fails" reference variant — wired but not recommended. Inbox row selection populates Inspector via 17D's dispatch.

**Files:** filter pills (six pills with keyboard shortcuts `1-6`); inbox list with `j`/`k` nav, selection vs focus separation; four layout components composing list + Inspector; `views.ts` registration: `register('inbox', { component: Inbox, keymap: inboxKeymap, defaultLayout: 'two-pane' })`. Component tests cover pill filtering, row selection, keyboard nav, layout switching. E2e walks: filter selection → row selection → action firing (with mocked daemon response). Visual: 24 snapshots.

```bash
git add crates/memoryd-web/frontend/src/views/Inbox.tsx crates/memoryd-web/frontend/src/views/inbox crates/memoryd-web/frontend/src/views.ts crates/memoryd-web/frontend/tests/views/Inbox.test.tsx crates/memoryd-web/frontend/tests/e2e/inbox.spec.ts crates/memoryd-web/frontend/tests/visual/inbox.spec.ts
git commit -m "feat(web): port Inbox view with 4 layout variants and full test coverage"
```

---

### Task 17F: Port Reality Check focus mode

**Parallel:** no (Cluster D-frontend sequential through `views.ts`)
**Blocked by:** Task 17E
**Owned files:** `crates/memoryd-web/frontend/src/views/RealityCheck.tsx` (new — port of handoff's `RealityCheck.jsx`), `crates/memoryd-web/frontend/src/views/realityCheck/QuestionStage.tsx`, `crates/memoryd-web/frontend/src/views/realityCheck/AnswerCards.tsx`, `crates/memoryd-web/frontend/src/views/realityCheck/SessionSidebar.tsx`, `crates/memoryd-web/frontend/src/views/realityCheck/CorrectEditor.tsx`, `crates/memoryd-web/frontend/src/views/realityCheck/ScoreBreakdown.tsx`, `crates/memoryd-web/frontend/src/views/realityCheck/CompletionCard.tsx`, `crates/memoryd-web/frontend/src/views/realityCheck/index.ts`, `crates/memoryd-web/frontend/src/views.ts` (register `reality-check`), `crates/memoryd-web/frontend/tests/views/RealityCheck.test.tsx`, `crates/memoryd-web/frontend/tests/e2e/realityCheck.spec.ts`, `crates/memoryd-web/frontend/tests/visual/realityCheck.spec.ts`
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run realityCheck && pnpm run test:visual --run realityCheck && pnpm run test:e2e -- --grep realityCheck`
**Worktree:** `../agent-memory-wt/task-17F/` on `dogfood/task-17F-reality-check`

**Chrome dissolution contract.** Reality Check is the only view that dissolves the dashboard chrome. Sidebar collapses to nothing; top bar shrinks to a single thin strip rendering `◆ memorum · reality check · <namespace> · <progress gauge> · <X of Y> · esc · pause`. Footer keymap stays (per handoff). Question column is centered horizontally with the SESSION sidebar floating right. Variants to render: happy path, encrypted memory, refused decision, score breakdown expanded, session complete (`X of X`).

**Protocol contract.** Actions wire to `RealityCheckRequest::Respond { session_id, memory_id, action: RealityCheckAction }`. `RealityCheckAction` variants: `Confirm`, `Correct { new_body: String }`, `Forget { reason: String }` (default reason `"user-forgot-via-tui"` if not collected — same default as TUI Task 13), `NotRelevant`, `SkipThisWeek`. Verified against `crates/memoryd/src/protocol.rs:195-212`.

**Files:** question stage (chrome strip + scope line + question heading + memory body + source line); 4-card answer hierarchy with `y`/`k`/`f`/`s` keyboard contract; session sidebar with done (`✓`) / current (`▸`) / upcoming (`·`) / `+ N more` rollup; correct-editor inline single-field textarea (replaces card area when `k` pressed); score breakdown expandable with 5 component bars; completion card. Tests cover all 5 variants + keyboard contract. Visual: 5 variants × 6 themes = 30 snapshots.

```bash
git add crates/memoryd-web/frontend/src/views/RealityCheck.tsx crates/memoryd-web/frontend/src/views/realityCheck crates/memoryd-web/frontend/src/views.ts crates/memoryd-web/frontend/tests/views/RealityCheck.test.tsx crates/memoryd-web/frontend/tests/e2e/realityCheck.spec.ts crates/memoryd-web/frontend/tests/visual/realityCheck.spec.ts
git commit -m "feat(web): port Reality Check focus mode with chrome dissolution and 5-variant coverage"
```

---

### Task 17G: Port Recall ledger + Dreams views

**Parallel:** no (Cluster D-frontend sequential through `views.ts`)
**Blocked by:** Task 17F
**Owned files:** `crates/memoryd-web/frontend/src/views/Recall.tsx` (new), `crates/memoryd-web/frontend/src/views/recall/TimelineStrip.tsx`, `crates/memoryd-web/frontend/src/views/recall/RecallList.tsx`, `crates/memoryd-web/frontend/src/views/recall/index.ts`, `crates/memoryd-web/frontend/src/views/Dreams.tsx`, `crates/memoryd-web/frontend/src/views/dreams/DreamList.tsx`, `crates/memoryd-web/frontend/src/views/dreams/index.ts`, `crates/memoryd-web/frontend/src/views.ts` (register `recall`, `dreams`), `crates/memoryd-web/frontend/tests/views/Recall.test.tsx`, `crates/memoryd-web/frontend/tests/views/Dreams.test.tsx`, `crates/memoryd-web/frontend/tests/e2e/recall.spec.ts`, `crates/memoryd-web/frontend/tests/e2e/dreams.spec.ts`, `crates/memoryd-web/frontend/tests/visual/recall.spec.ts`, `crates/memoryd-web/frontend/tests/visual/dreams.spec.ts`
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run "recall|dreams" && pnpm run test:visual --run "recall|dreams" && pnpm run test:e2e -- --grep "recall|dreams"`
**Worktree:** `../agent-memory-wt/task-17G/` on `dogfood/task-17G-recall-dreams`

**Recall ledger:** timeline strip (sparkline-style 30-day bar chart) + dense column list (TIME/SEQ/DEVICE/AGENT/MEMORY/NAMESPACE/LAT/SCORE). Inspector kind `recall-event` from 17D. Header affordances: agent filter, device filter, free-text search, export CSV. **Heavy-data state must hold:** the test suite includes a 9k-event scroll-perf assertion (60fps target via `requestAnimationFrame` measurement, virtualization required if needed).

**Dreams:** status-pill-filtered list (PROPOSED / QUEUED / ACCEPTED / COMPLETED / DISMISSED / RUNNING). Inspector kind `dream-output` from 17D. Item-kind drives inspector composition (Pattern / Question / Cleanup / Dream-run). Per pre-handoff design review: dream-run "meta-entries" (one per scheduled run, summarizing the pass) get a distinct visual treatment so they don't conflate with the per-output entries.

**Files:** Recall view + Dreams view + their sub-components + `views.ts` registrations + per-view component tests + e2e walk + visual snapshots (Recall: 4 visual states × 6 themes = 24; Dreams: 4 status-states × 6 themes = 24).

```bash
git add crates/memoryd-web/frontend/src/views/Recall.tsx crates/memoryd-web/frontend/src/views/recall crates/memoryd-web/frontend/src/views/Dreams.tsx crates/memoryd-web/frontend/src/views/dreams crates/memoryd-web/frontend/src/views.ts crates/memoryd-web/frontend/tests/views/Recall.test.tsx crates/memoryd-web/frontend/tests/views/Dreams.test.tsx crates/memoryd-web/frontend/tests/e2e/recall.spec.ts crates/memoryd-web/frontend/tests/e2e/dreams.spec.ts crates/memoryd-web/frontend/tests/visual/recall.spec.ts crates/memoryd-web/frontend/tests/visual/dreams.spec.ts
git commit -m "feat(web): port Recall ledger + Dreams views with heavy-data perf assertion"
```

---

### Task 17H: Port Peers + Governance + Entities views

**Parallel:** no (Cluster D-frontend sequential through `views.ts`)
**Blocked by:** Task 17G
**Owned files:** `crates/memoryd-web/frontend/src/views/Peers.tsx` (new), `crates/memoryd-web/frontend/src/views/peers/TrustLedger.tsx`, `crates/memoryd-web/frontend/src/views/peers/index.ts`, `crates/memoryd-web/frontend/src/views/Governance.tsx`, `crates/memoryd-web/frontend/src/views/governance/ReviewQueue.tsx`, `crates/memoryd-web/frontend/src/views/governance/index.ts`, `crates/memoryd-web/frontend/src/views/Entities.tsx`, `crates/memoryd-web/frontend/src/views/entities/EntityTable.tsx`, `crates/memoryd-web/frontend/src/views/entities/index.ts`, `crates/memoryd-web/frontend/src/views.ts` (register `peers`, `governance`, `entities`), `crates/memoryd-web/frontend/tests/views/Peers.test.tsx`, `crates/memoryd-web/frontend/tests/views/Governance.test.tsx`, `crates/memoryd-web/frontend/tests/views/Entities.test.tsx`, plus matching e2e + visual specs (one per view)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run "peers|governance|entities" && pnpm run test:visual --run "peers|governance|entities" && pnpm run test:e2e -- --grep "peers|governance|entities"`
**Worktree:** `../agent-memory-wt/task-17H/` on `dogfood/task-17H-peers-gov-entities`

**Peers:** trust-ledger sortable table (DEVICE / LABEL / TRUST / SYNC / PUBKEY / LAST HANDSHAKE / LOCKS H/P / EVENTS 24H). Trust × sync paired badges (`trusted/in-sync`, `trusted/behind`, `limited/fenced`, `revoked/revoked`). Inspector kind `peer-detail` from 17D. Top-right "+ pair new device" CTA.

**Governance:** review queue surface (NOT policy editor — that stays as the v1.1+ deferral). Kind-filter pills (blocks / warnings / info / consent / redactions). Batch checkbox column + batch action bar (Approve selected / Reject selected). Inspector kind `governance-decision` from 17D — composes Inbox's PROVENANCE / POLICY / PRIVACY SCAN PLUS the new POLICY DECISION TRACE card from 17D's `cards/`.

**Entities:** sortable table (NAME / KIND / MENTIONS / NAMESPACES / LAST SEEN / FIRST SEEN / CONFIDENCE bar). Kind-filter pills (person / org / project / place / tool / language). Inspector kind `entity-detail` from 17D. Per pre-handoff design review: graph view is v1.1+ deferral, NOT in scope here.

```bash
git add crates/memoryd-web/frontend/src/views/Peers.tsx crates/memoryd-web/frontend/src/views/peers crates/memoryd-web/frontend/src/views/Governance.tsx crates/memoryd-web/frontend/src/views/governance crates/memoryd-web/frontend/src/views/Entities.tsx crates/memoryd-web/frontend/src/views/entities crates/memoryd-web/frontend/src/views.ts crates/memoryd-web/frontend/tests/views/Peers.test.tsx crates/memoryd-web/frontend/tests/views/Governance.test.tsx crates/memoryd-web/frontend/tests/views/Entities.test.tsx crates/memoryd-web/frontend/tests/e2e/peers.spec.ts crates/memoryd-web/frontend/tests/e2e/governance.spec.ts crates/memoryd-web/frontend/tests/e2e/entities.spec.ts crates/memoryd-web/frontend/tests/visual/peers.spec.ts crates/memoryd-web/frontend/tests/visual/governance.spec.ts crates/memoryd-web/frontend/tests/visual/entities.spec.ts
git commit -m "feat(web): port Peers, Governance review queue, and Entities views"
```

---

### Task 17I: Wire real data — TanStack Query + CSRF + SSE + MSW fixtures

**Parallel:** no (after all view ports)
**Blocked by:** Task 17H
**Owned files:** `crates/memoryd-web/frontend/src/api/client.ts` (new — fetch wrapper with CSRF), `crates/memoryd-web/frontend/src/api/queries.ts` (TanStack Query hooks), `crates/memoryd-web/frontend/src/api/mutations.ts` (POST mutations with optimistic + rollback), `crates/memoryd-web/frontend/src/api/notifications.ts` (SSE EventSource for `/api/notifications/stream`), `crates/memoryd-web/frontend/src/api/types.ts` (matches `crates/memoryd-web/src/server.rs` route response shapes — generated or hand-authored), `crates/memoryd-web/frontend/src/api/index.ts` (barrel), `crates/memoryd-web/frontend/src/main.tsx` (wrap with `QueryClientProvider`), every `crates/memoryd-web/frontend/src/views/*.tsx` and `views/*/`-internal data-binding swaps (replace mock data with query hooks — files modified, not created), `crates/memoryd-web/frontend/tests/msw/handlers.ts` (MSW request handlers covering every route), `crates/memoryd-web/frontend/tests/msw/server.ts` (MSW node setup for unit tests), every `crates/memoryd-web/frontend/tests/views/*.test.tsx` (extended to use MSW), `crates/memoryd-web/frontend/tests/e2e/realData.spec.ts` (Playwright e2e against fixture daemon)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `xhigh`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run && pnpm run test:e2e && pnpm run test:visual --run`
**Worktree:** `../agent-memory-wt/task-17I/` on `dogfood/task-17I-real-data-wiring`

**Wiring contract.** Replace every `data.js` import (mock data) with a TanStack Query hook hitting the real API route. **Route enumeration approach:** read `crates/memoryd-web/src/server.rs` and locate the `router_with_state` function (search by name, not by line number — Tasks 15 and 16 will have shifted line positions by adding the new `policy_editor`, `sync_dashboard`, and `reality_check_history` handlers). Enumerate every `.route(...)` call inside `router_with_state` and `protected_post_routes`. Verified route set as of v1.3 (after Tasks 15 + 16 land): `/api/status`, `/api/entity-graph`, `/api/entity-graph/{entity_id}`, `/api/roi`, `/api/reality-check`, `/api/reality-check/history`, `/api/reality-check/respond` (POST), `/api/recall-hits`, `/api/audit/{id}`, `/api/audit/{id}/walk`, `/api/audit/{id}/temporal`, `/api/review`, `/api/review/action` (POST), `/api/notifications/stream` (SSE), `/api/policy-editor` (GET + POST), `/api/sync-dashboard`. Every POST attaches `X-Memorum-CSRF` header from `<meta name="csrf-token">`. SSE EventSource subscribes once at App mount, dispatches notification events to a shared store consumed by the bell dropdown.

**Error contract.** 403 (CSRF fail) → toast "Session expired. Refresh the page." with a refresh button. 409 (stale write) → toast "This item changed elsewhere. Refresh to see latest." with a refresh button + optimistic rollback. 503 / network error → page-level Banner "Daemon unreachable" in `--bad`, list rows dim to `--fg-3`, all action buttons disable.

**MSW fixtures.** Every route gets a handler in `tests/msw/handlers.ts` with a default happy-path response and named overrides (empty / heavy / error / 403 / 409 / 503). Component tests inject MSW per scenario.

**Files:** API client (fetch wrapper with CSRF + JSON parse + error normalization); query hooks (one per GET route); mutation hooks (one per POST route, with optimistic + rollback semantics); SSE subscription; type definitions; MSW handlers + setup; mutation test in `tests/api/`. Every existing view test extended to use MSW. New e2e against fixture-daemon spin-up.

```bash
git add crates/memoryd-web/frontend/src/api crates/memoryd-web/frontend/src/main.tsx crates/memoryd-web/frontend/src/views crates/memoryd-web/frontend/tests/msw crates/memoryd-web/frontend/tests/views crates/memoryd-web/frontend/tests/e2e/realData.spec.ts crates/memoryd-web/frontend/tests/api
git commit -m "feat(web): wire real data via TanStack Query + CSRF + SSE with MSW fixtures"
```

---

### Task 17J: Settings page + keyboard handlers + command palette

**Parallel:** no
**Blocked by:** Task 17I
**Owned files:** `crates/memoryd-web/frontend/src/views/Settings.tsx` (new), `crates/memoryd-web/frontend/src/views/settings/AppearanceTab.tsx`, `crates/memoryd-web/frontend/src/views/settings/ThemeEditorTab.tsx`, `crates/memoryd-web/frontend/src/views/settings/KeyboardTab.tsx`, `crates/memoryd-web/frontend/src/views/settings/NotificationsTab.tsx`, `crates/memoryd-web/frontend/src/views/settings/AboutTab.tsx`, `crates/memoryd-web/frontend/src/views/settings/index.ts`, `crates/memoryd-web/frontend/src/keyboard/Keymap.ts` (global + per-view keymap registry), `crates/memoryd-web/frontend/src/keyboard/useKeymap.ts` (hook), `crates/memoryd-web/frontend/src/palette/CommandPalette.tsx` (modal with fuzzy match), `crates/memoryd-web/frontend/src/palette/commands.ts` (command catalog), `crates/memoryd-web/frontend/src/palette/index.ts`, `crates/memoryd-web/frontend/src/help/HelpOverlay.tsx`, `crates/memoryd-web/frontend/src/views.ts` (register `settings`, plus reroute `?tweaks=1` to dev tweaks panel), `crates/memoryd-web/frontend/src/App.tsx` (mount palette + help overlay + global keymap), `crates/memoryd-web/frontend/tests/views/Settings.test.tsx`, `crates/memoryd-web/frontend/tests/keyboard/Keymap.test.tsx`, `crates/memoryd-web/frontend/tests/palette/CommandPalette.test.tsx`, `crates/memoryd-web/frontend/tests/e2e/settings.spec.ts`, `crates/memoryd-web/frontend/tests/e2e/keyboard.spec.ts`, `crates/memoryd-web/frontend/tests/e2e/palette.spec.ts`, `crates/memoryd-web/frontend/tests/visual/settings.spec.ts`, `crates/memoryd-web/frontend/tests/visual/palette.spec.ts`, plus `crates/memoryd-web/frontend/package.json` (add `fuse.js@^7` to `dependencies`) and `crates/memoryd-web/frontend/pnpm-lock.yaml` (regenerated by `pnpm install` in worktree)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run lint && pnpm run typecheck && pnpm run test --run "settings|keyboard|palette" && pnpm run test:visual --run "settings|palette" && pnpm run test:e2e -- --grep "settings|keyboard|palette"`
**Worktree:** `../agent-memory-wt/task-17J/` on `dogfood/task-17J-settings-keyboard-palette`

**Settings page.** Five tabs at `/settings`: Appearance (theme swatch grid + density toggle + reduced-motion 3-way + font-size slider), Theme editor (per-token OKLCH picker + live preview + save-as), Keyboard (full keymap reference table — read-only), Notifications (channel toggles + thresholds), About (versions + commit + docs links). The Tweaks panel from the handoff stays available in dev mode behind `?tweaks=1` query param so designers can iterate.

**Keymap registry.** Global keys: `:` palette, `?` help, `Esc` close-modal, `g` then letter for nav (`gi` Inbox, `gr` Reality Check, etc.). Per-view: filter pills `1-6`, list nav `j`/`k`, view-specific actions (`a`/`r`/`e`/`f` Inbox; `y`/`k`/`f`/`s` Reality Check; etc.). The keymap registry takes per-view keymaps and merges with global. ⚠️ guard against text-input focus (don't dispatch `j` when typing in a textarea).

**Command palette.** Modal centered, ~520 px wide. Fuzzy matcher ranks commands via `fuse.js` (added in this task's `package.json` as a runtime dependency). v1.2 mentioned `nucleo-matcher` as the candidate library; plan-reviewer caught in v1.3 that `nucleo-matcher` is a Rust crate with no published JavaScript package, so v1.3 commits to `fuse.js`. Categories: Navigate, Theme, Action, Help. Each command entry: label, scope, optional keyboard shortcut, dispatch function. View-specific commands appear when matching the active view scope. `Enter` executes; `Esc` closes.

```bash
git add crates/memoryd-web/frontend/src/views/Settings.tsx crates/memoryd-web/frontend/src/views/settings crates/memoryd-web/frontend/src/keyboard crates/memoryd-web/frontend/src/palette crates/memoryd-web/frontend/src/help crates/memoryd-web/frontend/src/views.ts crates/memoryd-web/frontend/src/App.tsx crates/memoryd-web/frontend/package.json crates/memoryd-web/frontend/pnpm-lock.yaml crates/memoryd-web/frontend/tests/views/Settings.test.tsx crates/memoryd-web/frontend/tests/keyboard crates/memoryd-web/frontend/tests/palette crates/memoryd-web/frontend/tests/e2e/settings.spec.ts crates/memoryd-web/frontend/tests/e2e/keyboard.spec.ts crates/memoryd-web/frontend/tests/e2e/palette.spec.ts crates/memoryd-web/frontend/tests/visual/settings.spec.ts crates/memoryd-web/frontend/tests/visual/palette.spec.ts
git commit -m "feat(web): Settings page + global keymap + command palette + help overlay"
```

---

### Task 17K: Surface state coverage + accessibility audit + bundle budgets + integration sweep

**Parallel:** no (final integration gate for Cluster D-frontend)
**Blocked by:** Task 17J
**Owned files:** `crates/memoryd-web/frontend/tests/states/` (new directory — 7 state-coverage spec files: `empty.spec.ts`, `daemonDown.spec.ts`, `csrf.spec.ts`, `staleWrite.spec.ts`, `paletteOpen.spec.ts`, `bellOpen.spec.ts`, `heavyData.spec.ts`), `crates/memoryd-web/frontend/tests/a11y/axe.spec.ts` (Playwright + @axe-core/playwright on every view × every theme), `crates/memoryd-web/frontend/tests/budgets/bundle.test.ts` (Vitest asserting CSS gzip ≤ 80 KB and JS gzip ≤ 250 KB), `crates/memoryd-web/frontend/tests/budgets/csp.test.ts` (Vitest asserting built `dist/index.html` has zero inline scripts and zero inline styles), `crates/memoryd-web/frontend/tests/perf/recallScroll.spec.ts` (Playwright RAF-based 60fps assertion at 9k events), `crates/memoryd-web/tests/frontend_smoke.rs` (extended), `crates/memoryd-web/frontend/README.md` (extended with the full test-suite catalog and runbook for regenerating snapshots / updating budgets / reviewing a11y violations)
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `xhigh`
**Skills to load:** `clean-code`, `tdd`
**Per-task gate:** `cd crates/memoryd-web/frontend && pnpm run check:all && pnpm run test:e2e && pnpm run test:visual --run && pnpm run test:a11y && pnpm run test --run budgets && cd ../../.. && cargo test -p memoryd-web --test frontend_smoke`. The `bash scripts/check.sh` full release gate runs separately as the Phase 5 trunk gate per the existing protocol at the "Trunk gate + integration" section, NOT as part of 17K's per-task gate. v1.2's gate concatenated check.sh on the end (~9-min addition); plan-reviewer R4 caught that this would push 17K past the 20-min default timeout. v1.3 split: per-task gate exercises everything 17K owns; the post-phase trunk gate runs check.sh once for the whole Phase 5 batch.

**Worktree:** `../agent-memory-wt/task-17K/` on `dogfood/task-17K-integration-validation`

**Coverage matrix.** Every view × every state. Specifically:
- 7 views × 6 state variants (empty, daemon-down, CSRF/403, stale-write/409, palette-open, bell-open, heavy-data) = **42 state tests**. Where a state isn't applicable (e.g., Reality Check has no "heavy data" semantic), the spec explicitly skips with a comment.
- 7 views × 6 themes × axe scan = **42 a11y scans**, each must report **zero violations**. Configure axe with `axe.run(document, { rules: { 'color-contrast': { enabled: true } } })`.
- Bundle budgets: gzipped CSS ≤ 80 KB, gzipped JS ≤ 250 KB (per route chunk if code-splitting is used). The test reads `dist/assets/*.css.gz` and `dist/assets/*.js.gz` (gzipping via `zlib` at test time if Vite doesn't pre-gzip), asserts byte counts.
- CSP-strict: `dist/index.html` MUST contain zero `<script>` tags with inline content and zero `<style>` tags with inline content. Asserted via simple regex match.
- Recall ledger heavy-data 60fps: Playwright with `page.evaluate()` measuring `requestAnimationFrame` deltas during a programmatic scroll of 9k entries; assert mean frame time ≤ 16.6ms.
- `frontend_smoke.rs` extended: asserts the served `/` page has no inline scripts/styles, has the CSRF meta, the title reads `Memorum Dashboard`, and the embedded bundle's CSS + JS are within budget (asserts byte counts of the embedded files at the Rust test level too — defense in depth against build-time vs. embed-time drift).

**Documentation deliverable.** `frontend/README.md` extended with: full test-suite catalog (every command + what it covers); regenerate-baselines runbook (`pnpm run test:visual --update`); review-a11y-violations runbook; bump-budget runbook (when do we let budgets grow); CI integration notes (which step runs in which job).

**Final integration commit.**
```bash
git add crates/memoryd-web/frontend/tests/states crates/memoryd-web/frontend/tests/a11y crates/memoryd-web/frontend/tests/budgets crates/memoryd-web/frontend/tests/perf crates/memoryd-web/frontend/README.md crates/memoryd-web/tests/frontend_smoke.rs
git commit -m "feat(web): full surface-state + a11y + budget + perf coverage; integration sweep complete"
```

---

### Task 18: Source-capture URL redaction (relocated to `memory-source`)

**Parallel:** yes (independent of Tasks 15-17 — this task no longer touches `crates/memoryd-web/`)
**Blocked by:** Task 5
**Owned files:** `crates/memory-source/src/url_safety.rs` (URL parsing + hop validation already lives here), `crates/memory-source/src/capture.rs` (capture pipeline that calls into url_safety), `crates/memory-source/tests/source_capture_redaction.rs`
**Subagent:** two-phase — `security_auditor` (read-only, xhigh) then `worker` (workspace-write, medium)
**Sandbox:** `read-only` (phase 1) → `workspace-write` (phase 2)
**Reasoning:** `xhigh` (phase 1) → `medium` (phase 2)
**Skills to load:** `clean-code`, `rust-engineer` (both phases)
**Per-task gate:** `cargo test -p memory-source --test source_capture_redaction && cargo clippy -p memory-source --tests -- -D warnings && cargo fmt -p memory-source -- --check`
**Worktree:** `../agent-memory-wt/task-18/` on `dogfood/task-18-source-capture-redact`

**Crate-location fix (v0.5).** v0.4 placed this task in `crates/memoryd-web/src/source_capture/url.rs` — that path does not exist. URL parsing + hop validation lives in `crates/memory-source/src/url_safety.rs`; the capture pipeline lives in `crates/memory-source/src/capture.rs:14` (which already imports from `url_safety`). v0.5 relocates the task to those files.

**Persisted-URL surface coverage (v0.6 + v0.7 fixes).** v0.5 said "redact before persisting the captured URL into the manifest" — singular. The actual `WebCaptureManifest` (`crates/memory-source/src/model.rs:46-82`) persists **multiple URL-bearing fields**, and `RedirectHop` itself has **two** URL strings on it: `pub url: String` (the URL of the hop request) and `pub location: String` (the raw `Location:` header value the response sent back, which is itself a URL or an absolute path that gets joined into one — `capture.rs:83-89` reads it from `response.headers().get(LOCATION)` and stores it verbatim). v0.6 redacted `manifest.original_url`, `manifest.final_url`, and `redirect_chain[i].url` but **missed `redirect_chain[i].location`**. v0.7 fix: redact every URL persisted to disk **and** every URL returned in the response — `manifest.original_url`, `manifest.final_url`, every `manifest.redirect_chain[i].url`, **every `manifest.redirect_chain[i].location`** (v0.7 add), and `response.final_url`. Single redaction function applied at every persistence/return site. The auditor's enumeration (phase 1) must include the `location` field explicitly.

**Two-phase orchestration (explicit steps):**
1. Spawn `security_auditor` (read-only, xhigh) with brief: "Read `crates/memory-source/src/url_safety.rs`, `crates/memory-source/src/capture.rs`, and `crates/memory-source/src/model.rs` end to end. Enumerate (a) every sensitive query-parameter name that should be redacted, (b) every fragment pattern that should be stripped, (c) every place in the capture pipeline that persists or returns a URL — `manifest.original_url`, `manifest.final_url`, every `manifest.redirect_chain[i].url`, every `manifest.redirect_chain[i].location` (the raw `Location:` header value — read at `capture.rs:83-89`), and `response.final_url`. Output two Markdown bullet lists: redaction targets and persistence sites. No other prose."
2. `wait_agent` for completion. Capture the auditor output verbatim.
3. Spawn `worker` (workspace-write, medium) with brief: "Apply the redaction list to `crates/memory-source/src/url_safety.rs`. Add a `redact_sensitive_url(url: &Url) -> Url` function that strips sensitive query params and matching fragments, plus a `redact_sensitive_location_header(raw: &str) -> String` that parses the raw `Location:` header value (which may be absolute or relative — use `Url::parse` with the hop's base URL as fallback for relative cases) and applies the same redaction. Modify `crates/memory-source/src/capture.rs` so that **every** URL string written to `WebCaptureManifest` is the redacted form: `manifest.original_url`, `manifest.final_url`, every `manifest.redirect_chain[i].url`, and every `manifest.redirect_chain[i].location`. Also redact `CaptureWebSourceResponse.final_url` before returning. Add the test cases in `crates/memory-source/tests/source_capture_redaction.rs`. Per-task gate: `cargo test -p memory-source --test source_capture_redaction && cargo clippy -p memory-source --tests -- -D warnings && cargo fmt -p memory-source -- --check`."
4. `wait_agent` for completion; integrate.

**Files:**
- Modify: `crates/memory-source/src/url_safety.rs` — add `pub fn redact_sensitive_url(url: &Url) -> Url` that strips the auditor-supplied param list (case-insensitive) and the documented fragment patterns (e.g., `#access_token=`, `#id_token=`). Add `pub fn redact_sensitive_location_header(raw: &str, base: &Url) -> String` that handles absolute and relative `Location:` values uniformly. Add a const `SENSITIVE_QUERY_PARAMS: &[&str]` populated from the auditor output.
- Modify: `crates/memory-source/src/capture.rs` — call `redact_sensitive_url` at every URL write site: `manifest.original_url = redact_sensitive_url(&request_url).to_string()`; `manifest.final_url = redact_sensitive_url(&hop.url).to_string()`; for each `RedirectHop` pushed to `redirect_chain`, redact **both** the `url` field (`redact_sensitive_url(&hop.url)`) **and** the `location` field (`redact_sensitive_location_header(&location, &hop.url)`); `response.final_url = redact_sensitive_url(&hop.url).to_string()`. The on-disk manifest contains zero unredacted URLs and zero unredacted `Location:` headers; the response returns zero unredacted URLs.
- Test: `crates/memory-source/tests/source_capture_redaction.rs` — capture URL with `?token=xyz` ⇒ token stripped from `manifest.original_url`; URL with `#access_token=...` ⇒ fragment stripped from manifest; redirect-chain captured with sensitive params on intermediate hops ⇒ every hop's `url` AND `location` is redacted in `manifest.redirect_chain`; a redirect whose `Location:` header carries a sensitive query param (e.g., a `302 → /reset?token=...` flow) has the `location` field redacted in the manifest; response's `final_url` is redacted; URL with no sensitive params unchanged byte-for-byte; URL with mixed case (`?Token=`) ⇒ stripped (case-insensitive match).

```bash
git commit -m "fix(memory-source): redact sensitive query params and fragments before manifest write"
```

---

## Phase 6 — MCP tool surface fixes (Cluster A — strictly sequential)

All `crates/memoryd/src/handlers.rs` work serializes through this phase.

---

### Task 19: `memory_search` `include_body` real implementation

**Parallel:** no (Cluster A)
**Blocked by:** Task 9 (Doctor extracted from handlers; subsequent edits target `handlers/mod.rs`)
**Owned files:** `crates/memoryd/src/handlers/mod.rs` (search section, post-Task-9 path), `crates/memoryd/src/mcp.rs` (output schema for `body` field — declared here for the next Cluster A tasks to coordinate; if Task 22 also needs `mcp.rs`, Task 22 rebases over Task 19), `crates/memoryd/src/protocol.rs` (SearchHit shape), `crates/memoryd/tests/handler_contract.rs` (search tests)
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test handler_contract -- search && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-19/` on `dogfood/task-19-search-include-body`

**Files:**
- Modify: `handlers.rs:1526` — when `include_body: true`, fetch full body via `Substrate::get_memory_content` and return in `SearchHit.body` (new field). When false, keep current 240-char snippet behavior.
- Modify: protocol response shape — `SearchHit.body: Option<String>`.
- Modify: `mcp.rs` schema — declare `body` in output schema.
- Test: `include_body: true` returns full body; `include_body: false` returns `None` for body and snippet only.

```bash
git commit -m "fix(mcp): memory_search include_body honored"
```

---

### Task 20: `memory_get` `include_provenance` real implementation

**Parallel:** no (Cluster A)
**Blocked by:** Task 19
**Owned files:** `crates/memoryd/src/handlers/mod.rs` (get section), `crates/memoryd/src/mcp.rs` (provenance schema), `crates/memoryd/src/protocol.rs` (provenance envelope shape), `crates/memoryd/tests/handler_contract.rs` (get tests)
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test handler_contract -- get && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-20/` on `dogfood/task-20-get-include-provenance`

**Files:**
- Modify: `handlers/mod.rs:1537` (post-Task-9 path) — accept `include_provenance: bool`; when true attach `ProvenanceEnvelope { actor, reason, source_chain, signed_at, harness, session_id }` from substrate.
- Modify: `protocol.rs` — `MemoryGetResponse.provenance: Option<ProvenanceEnvelope>`.
- Modify: `mcp.rs` schema — declare `provenance`.
- Test: `include_provenance: true` returns populated envelope; `false` omits.

```bash
git commit -m "fix(mcp): memory_get include_provenance honored"
```

---

### Task 21: `memory_forget` reason sanitization

**Parallel:** no (Cluster A)
**Blocked by:** Task 20
**Owned files:** `crates/memoryd/src/handlers/mod.rs` (forget section, post-Task-9 path), `crates/memoryd/tests/handler_contract.rs` (forget tests)
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test handler_contract -- forget && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-21/` on `dogfood/task-21-forget-reason-sanitize`

**Files:**
- Modify: `handlers/mod.rs` (forget section, post-Task-9 path) — call `sanitize_forget_reason()` (already at the helper at line ~1189) before passing reason to `tombstone_memory`/`write_tombstone_rule`. Empty reasons rejected with `invalid_request`.
- Test: empty reason rejected; reason with email/phone gets sanitized in tombstone JSONL.

```bash
git commit -m "fix(mcp): memory_forget reason sanitized before tombstone write"
```

---

### Task 22: `tools/list` reconciliation + system spec §14.1 amendment

**Parallel:** no (Cluster A)
**Blocked by:** Task 21
**Owned files:** `crates/memoryd/src/mcp.rs`, `crates/memoryd/tests/mcp_manifest.rs`, `docs/specs/system-v0.2.md`, `docs/api/stream-b-daemon-mcp-api.md` *(verified live at this path; v0.7 fix — v0.6 said `stream-b-mcp-api.md` but the actual filename is `stream-b-daemon-mcp-api.md`)*
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test mcp_manifest && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-22/` on `dogfood/task-22-tools-list-spec-amend`

**Decision:** The MCP surface stays at **10 tools** for v1.x. Task 4 explicitly does **not** add an 11th tool (`memory_status` is daemon-protocol-only, see Task 4's system spec note). `memory_capture_source` was added by the recent web-source-grounding commit (`ab66a34`) without an explicit §14.1 amendment — Task 22 ratifies it in the system spec so the charter and the implementation agree.

**Files:**
- Modify: `mcp.rs:263` — verify `ToolName::all()` returns exactly 10: `memory_search`, `memory_get`, `memory_write`, `memory_supersede`, `memory_forget`, `memory_reveal`, `memory_startup`, `memory_note`, `memory_observe`, `memory_capture_source`. No more, no less.
- Modify: `docs/specs/system-v0.2.md` §14.1 — append the dated amendment block **inside** the existing §14.1 section (immediately after the original tool-count statement, NOT at end-of-file): "**2026-05-07 amendment:** v1 MCP surface ratified at 10 tools (adds `memory_capture_source` shipped 2026-05-06 in `ab66a34`). Surface frozen at 10 for v1.x. Daemon-protocol commands (`Status`, `Doctor`, etc.) are not part of the MCP surface and are exposed via socket only."
- Modify: charter doc (located via `rg -l 'memory_observe' docs/`) — update tool count and add `memory_capture_source` description.
- Test: `tools/list` returns exactly 10; each dispatches without phantom routing; the spec amendment block exists and parses.

```bash
git commit -m "fix(mcp): 10-tool surface ratified in system spec §14.1"
```

---

### Task 23: `memory_supersede` encrypted-memory handling

**Parallel:** no (Cluster A)
**Blocked by:** Task 22
**Owned files:** `crates/memoryd/src/handlers/mod.rs` (supersede paths around lines 1978-2022, post-Task-9 path), `crates/memoryd/tests/handler_contract.rs` (supersede tests)
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test handler_contract -- supersede && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-23/` on `dogfood/task-23-supersede-encrypted`

**Files:**
- Modify: `handlers/mod.rs` (supersede sections, post-Task-9 path) — when `enforcement.encryption: false` (Task 1's flag off — current dogfood default), supersede works on plaintext-stored memories normally. When `enforcement.encryption: true`, the existing refusal stays; the refusal message changes to a clearer "encrypted supersession requires reveal+rewrite cycle in current build" with a runbook pointer.
- Test: with encryption off, supersede a plaintext memory succeeds; with encryption on, the refusal fires with the new message and runbook link.

```bash
git commit -m "fix(mcp): memory_supersede honors privacy.encryption flag"
```

---

### Task 24: `memory_reveal` envelope metadata + reason validation polish

**Parallel:** no (Cluster A)
**Blocked by:** Task 23
**Owned files:** `crates/memoryd/src/handlers/mod.rs` (reveal path around lines 1557-1571, post-Task-9 path), `crates/memory-privacy/src/crypto.rs` (envelope serialization — small surface), `crates/memoryd/tests/handler_contract.rs` (reveal tests)
**Subagent:** `mcp_developer`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test handler_contract -- reveal && cargo test -p memory-privacy --test envelope && cargo clippy -p memoryd -p memory-privacy --tests -- -D warnings && cargo fmt -p memoryd -p memory-privacy -- --check`
**Worktree:** `../agent-memory-wt/task-24/` on `dogfood/task-24-reveal-envelope-metadata`

**Files:**
- Modify: `handlers/mod.rs` (reveal path, post-Task-9 path) — replace `EncryptedPayload { ciphertext: bytes, envelope: serde_json::Value::Null }` with the actual stored envelope from disk; pass to `PrivacyEncryptor::decrypt`.
- Modify: `handlers/mod.rs` (reveal reason validation, post-Task-9 path) — bounded length (max 512 chars), non-empty, but skip the privacy classifier scan on the reason itself (the prior implementation routed reasons through `is_safe_plaintext_for_indexing` which silently refused legitimate reasons mentioning emails/URLs).
- Test: reveal of envelope-bearing ciphertext decrypts; reason mentioning a URL is accepted; reason longer than 512 chars rejected; empty reason rejected.

```bash
git commit -m "fix(mcp): memory_reveal envelope metadata + reason bounded validation"
```

---

### Task 25: Recall `since_event_id` startup delta fallback + pending-attention dedupe

**Parallel:** no (Cluster A — last in queue)
**Blocked by:** Task 24
**Owned files:** `crates/memoryd/src/recall/startup.rs:67-69`, `crates/memoryd/src/recall/startup.rs:117` (pending-attention dedupe), `crates/memoryd/src/handlers/mod.rs` (recall hook wiring, post-Task-9 path), `crates/memoryd/tests/recall_startup.rs`
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test recall_startup && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-25/` on `dogfood/task-25-recall-since-event-fallback`

**Files:**
- Modify: `recall/startup.rs:67-69` — replace `RecallError::not_implemented("event-based startup deltas...")` with a proper fallback: when `since_event_id` is supplied, attempt incremental delta via `events_log.read_since(event_id)`; on ANY failure (event log gap, schema mismatch, stale mirror), emit a `tracing::warn!` and fall back to full startup recall. Never error on this path.
- Modify: `recall/startup.rs:117` — pending-attention dedupe: build a `HashSet<MemoryId>` of all attention-marked rows from `collect_recall_candidates`, then count `Candidate`/`Quarantined` rows from the index excluding any ID already in the set. Single source of truth, no double-count.
- Test: `since_event_id` with valid log returns incremental delta; `since_event_id` with stale mirror returns full recall + warn; `pending_attention_count` never exceeds the union of attention-marked sets.

```bash
git commit -m "fix(recall): since_event_id fallback + pending-attention dedupe"
```

---

## Phase 7 — Dream + notifications + eval honesty

Sequencing within phase: **Task 26 → Task 27 → Task 28**, because Tasks 26 and 27 both touch `crates/memoryd/src/dream/orchestration.rs`, and Task 28 also touches it. Task 29 (eval honesty, separate crate) is independent and may parallel any of 26/27/28.

---

### Task 26: Dream auth-failure stderr no-redact + diagnostic clarity

**Parallel:** yes
**Blocked by:** none (independent of Cluster A)
**Owned files:** `crates/memoryd/src/dream/harness.rs:706-712`, `crates/memoryd/src/dream/orchestration.rs:133-135`, `crates/memoryd/tests/dream_auth_diagnostic.rs`
**Subagent:** `worker`
**Sandbox:** `workspace-write`
**Reasoning:** `medium`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test dream_auth_diagnostic && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-26/` on `dogfood/task-26-dream-auth-stderr`

**Files:**
- Modify: `dream/harness.rs:706-712` — auth-probe failures preserve stderr verbatim (subject to a 4 KiB cap and explicit secret-pattern redaction — strip API keys but keep "not logged in" / "session expired" / "permission denied" messages). The previous sha256-everything redaction is replaced with selective patterns.
- Modify: `dream/orchestration.rs:133-135` — `dream_unavailable` error includes the cleaned stderr in the `diagnostic` field of the response payload so a co-founder sees the real message.
- Test: simulated `claude` auth failure produces a useful diagnostic, not a hash; a stderr containing an `sk-ant-...` token has the token redacted but surrounding message preserved.

```bash
git commit -m "fix(dream): preserve stderr in auth diagnostics, redact only secrets"
```

---

### Task 27: Dream prompt rewrites (Pass 1/2/3) with examples and schemas

**Parallel:** no (sequence after Task 26 — both touch `dream/orchestration.rs`)
**Blocked by:** Task 26
**Owned files:** `prompts/dream-pass-1-v2.md`, `prompts/dream-pass-2-v2.md`, `prompts/dream-pass-3-v2.md`, `crates/memoryd/src/dream/prompts.rs`, `crates/memory-substrate/src/config/mod.rs`, `crates/memoryd/src/dream/run.rs`, `crates/memoryd/src/dream/orchestration.rs`, `crates/memoryd/src/dream/pass1.rs`, `crates/memoryd/src/dream/pass2.rs`, `crates/memoryd/src/dream/pass3.rs`, `crates/memoryd/src/main.rs` *(v0.8 add — `execute_dream_run` at `main.rs:801` constructs `DreamRunBuildRequest` at `main.rs:807` and is the actual loaded-config-to-build-request seam; without owning main.rs the wiring is dead)*
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `clean-code`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test dream_prompt_smoke && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-27/` on `dogfood/task-27-dream-prompts-v2`

**Subagent choice:** `heavy_worker` (workspace-write, high reasoning) authors both the prompt `.md` files and the Rust wiring in a single spawn. The `prompt_engineer` agent is read-only per the Codex inventory and cannot author code; do not split this task into two agents.

**Loaded-config plumbing (v0.7+v0.8 fix).** v0.6 said "Modify `crates/memoryd/src/dream/config.rs` — add `pub prompt_version` field". The `dream/config.rs` in memoryd holds `CleanupConfig`-style local types and is **not** the loaded-config path that flows from `memoryd serve` startup into the dream subsystem. v0.7 corrected the path to `DreamsConfig` → `DreamRunOptions` → `render_prompt`, but missed the actual seam where loaded config becomes a build request. v0.8 grounds the wiring in the real call graph:

- `crates/memoryd/src/main.rs:801` defines `async fn execute_dream_run(invocation: DreamRunInvocation) -> ...`. This is invoked from `Command::Dream` paths at `main.rs:480` and `main.rs:770`.
- Inside `execute_dream_run`, `main.rs:807` constructs `memoryd::dream::orchestration::DreamRunBuildRequest { ... }` populated from `invocation.dreams.{per_pass_timeout_seconds, pass_2_max_candidates, pass_1_window_days, default_cli_priority, ...}`.
- `crates/memoryd/src/dream/orchestration.rs:52` defines `pub struct DreamRunBuildRequest`. This is the actual handoff struct — a request, not the runtime options. Build-then-run produces `DreamRunOptions` internally; the request is the entry point.
- `crates/memoryd/src/handlers.rs:1323` also constructs `DreamRunBuildRequest { ... }` for handler-side dream invocations (post-Task-9 path: `handlers/mod.rs`).

v0.8 fix: thread `PromptVersion` through the real graph.
1. Add `prompt_version: PromptVersion` (with `#[serde(default = "default_prompt_version")]` defaulting to `V2`) to `memory_substrate::config::DreamsConfig` — **Stream A surface touch authorized** for this additive field only.
2. Add `pub prompt_version: PromptVersion` field to `DreamRunBuildRequest` in `crates/memoryd/src/dream/orchestration.rs:52` (the request struct, not just `DreamRunOptions`).
3. Add `pub prompt_version: PromptVersion` field to `DreamRunOptions` in `crates/memoryd/src/dream/run.rs:27` so the runtime carries it through pass dispatch.
4. Populate at every `DreamRunBuildRequest { ... }` construction site:
   - `crates/memoryd/src/main.rs:807` (inside `execute_dream_run`): `prompt_version: invocation.dreams.prompt_version`.
   - `crates/memoryd/src/handlers/mod.rs:1323` (post-Task-9 path): `prompt_version: substrate_config.dreams.prompt_version`. If the local accessor name differs from `substrate_config`, the worker uses `rg -n 'DreamRunBuildRequest' crates/memoryd/src/handlers/mod.rs` to find the construction site and reads the surrounding scope to identify the actual `DreamsConfig` handle name; the field path `.dreams.prompt_version` is invariant.
5. Inside `dream/run.rs` build/run logic, copy `request.prompt_version` into `DreamRunOptions.prompt_version` so passes can read it.
6. Change `render_prompt(pass: DreamPass, input: &DreamPromptInput, version: PromptVersion)` — add the `version` param and have the function dispatch to the right `include_str!` set.
7. Update every call site at `dream/pass1.rs:28`, `dream/pass2.rs:34`, `dream/pass3.rs:39`, plus any in-file calls in `dream/run.rs`, to pass `options.prompt_version`.

**Files:**
- Create: `prompts/dream-pass-1-v2.md` — full prompt with: role framing; substrate snapshot section explanation; few-shot example of input JSON → expected reflection JSON; entity-extraction schema; 3 worked examples covering empty substrate, sparse substrate, rich substrate. Target ~50–80 lines, not 10.
- Create: `prompts/dream-pass-2-v2.md` — same shape; output schema (JSON array of candidate-memory shapes per `MemoryCandidate` struct); refusal-reason enumeration; 3 worked examples.
- Create: `prompts/dream-pass-3-v2.md` — same shape; entity-binding constraint section; 3 worked examples.
- Modify: `crates/memoryd/src/dream/prompts.rs` — define `pub enum PromptVersion { V1, V2 }`; change `render_prompt` signature to take `PromptVersion`; resolve to the right `include_str!` per version. v1 prompt files remain valid `include_str!` targets.
- Modify: `crates/memory-substrate/src/config/mod.rs` — add `#[serde(default = "default_prompt_version")] pub prompt_version: PromptVersion` to `DreamsConfig` (line 81-105 area). **Default = `V2` everywhere** (v0.8 contradiction fix — v0.7 had inconsistent prose claiming both "V2 for new installs / V1 for omitted" and "V2 via serde default"; only the V2-everywhere reading is consistent with the dogfood goal of all installs running the new prompts on upgrade). Define `pub enum PromptVersion { V1, V2 }` in `memory-substrate::config` (lightweight enum, no further deps); re-export from the crate root.
- Modify: `crates/memoryd/src/dream/run.rs:27` — add `pub prompt_version: PromptVersion` field to `DreamRunOptions`. Update every `render_prompt(...)` call inside this file to pass `options.prompt_version` (the worker uses `rg -n 'render_prompt' crates/memoryd/src/dream/run.rs` to enumerate call sites; the local variable holding the options is `options` per the existing struct convention).
- Modify: `crates/memoryd/src/dream/orchestration.rs:78` — at the `DreamRunOptions { ... }` construction site, populate `prompt_version: loaded.synced.dreams.prompt_version`. If the local variable holding `LoadedConfig` is not named `loaded`, worker uses `rg -B5 'DreamRunOptions {' crates/memoryd/src/dream/orchestration.rs` to find the surrounding scope and read the actual binding name; the field path `.synced.dreams.prompt_version` is invariant.
- Modify: `crates/memoryd/src/dream/pass1.rs:28`, `pass2.rs:34`, `pass3.rs:39` — change `render_prompt(DreamPass::PassN, input)?` to `render_prompt(DreamPass::PassN, input, options.prompt_version)?`. Threading the `options` reference into pass functions if not already present is part of this task.
- Test: `crates/memoryd/tests/dream_prompt_smoke.rs` — load `DreamsConfig` from a YAML fixture with `prompt_version: V1` and `V2`; verify a YAML lacking the field deserializes to V2 (serde default); verify each pass's `render_prompt` returns content from the right v1/v2 file (the rendering layer).
- Test: `crates/memoryd/tests/dream_build_prompt_version.rs` *(new — v0.8 falsifiable build-path coverage)* — invoke the `execute_dream_run` build path end-to-end against a fixture: load `DreamsConfig` with `prompt_version: V1`, drive through `execute_dream_run(DreamRunInvocation { dreams: ..., ... })`, intercept the constructed `DreamRunBuildRequest`, and assert it carries `prompt_version: V1`. Then repeat with `V2` and assert `V2`. This catches the failure mode where rendering would work but the actual build/preview path is still hardcoded — v0.7's tests only covered the rendering layer in isolation, not the construction site.

```bash
git commit -m "feat(dream): v2 prompts with examples, schemas, refusal enumeration"
```

---

### Task 28: Notification fan-out — wire 5 unused variants (LeakedSecretDetected deferred)

**Parallel:** no (Cluster A — last in sequence; `handlers/mod.rs` ownership for the `ReviewQueueOverThreshold` emit)
**Blocked by:** Task 5 (socket resolver), Task 25 (last Cluster A predecessor — `handlers/mod.rs` ownership), Task 27 (touches `dream/orchestration.rs`)
**Owned files:** `crates/memoryd/src/notifications/dispatcher.rs`, `crates/memoryd/src/notifications/triggers.rs`, `crates/memory-substrate/src/runtime/reconcile.rs`, `crates/memoryd/src/dream/orchestration.rs`, `crates/memoryd/src/dream/run.rs`, `crates/memory-governance/src/review.rs`, `crates/memoryd/src/handlers/mod.rs` *(post-Task-9 path — this is where the `ReviewQueueOverThreshold` emit lives; v0.8 Cluster A serial dep on Task 25)*, `crates/memoryd/src/reality_check/scheduling.rs`, `crates/memoryd/src/server.rs`, `crates/memoryd/tests/notification_fanout.rs`
**Subagent:** `heavy_worker`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memoryd --test notification_fanout && cargo clippy -p memoryd -p memory-governance -p memory-substrate --tests -- -D warnings && cargo fmt -p memoryd -p memory-governance -p memory-substrate -- --check`
**Worktree:** `../agent-memory-wt/task-28/` on `dogfood/task-28-notification-fanout`

**Live-code path corrections (v0.5).** v0.4 referenced `crates/memory-governance/src/review_queue.rs` (does not exist; actual file is `review.rs`) and `crates/memory-privacy/src/audit.rs` (does not exist). The `LeakedSecretDetected` emit site would require either creating a new privacy-audit module or threading a callback into `memory-privacy` from `memoryd`'s notification dispatcher — both are larger than a notification-wiring task should be. v0.5 narrows scope: this task wires **5 of the 6 unused variants** (BlockingMergeConflict, DreamRunCompleted, DailySynthesisSummaryReady, ReviewQueueOverThreshold, RealityCheckOverdue). `LeakedSecretDetected` is deferred to the post-dogfood privacy refactor (out-of-scope per the plan header), where the new privacy-audit module is the appropriate home.

**Coordination notes:**
- This task does **not** touch `handlers/mod.rs` (post-Task-9) — the `DailySynthesisSummaryReady` emit site is in the dream-completion path (`dream/orchestration.rs` or `dream/run.rs`, NOT handlers). No Cluster A dependency.
- Touches `crates/memory-substrate/src/runtime/reconcile.rs` — Stream A surface authorized for a **return-shape extension only** (no broadcast channel injected into the substrate crate; see BlockingMergeConflict wiring below).
- Touches `dream/orchestration.rs` which Task 27 also modifies — sequence after Task 27 lands to avoid rebase.
- Touches `crates/memoryd/src/server.rs` (the post-reconcile notification emit) — coordinates with Task 5 which also touches `server.rs:354-359` (socket bind). Different code regions; either order works as long as both are integrated before Task 28's tests run.

**BlockingMergeConflict wiring — substrate stays sync, daemon emits.** `reconcile_all_phases` is a synchronous function in the substrate crate with no `tokio::sync::broadcast` dependency. To stay clean, this task adds `blocking_conflicts: Vec<String>` to `ReconcileReport` (Stream A return-shape extension, additive). The daemon's reconcile call site in `crates/memoryd/src/server.rs` (locate via `rg -n 'reconcile_all_phases' crates/memoryd/`) reads the report after the call returns and emits `NotificationEvent::BlockingMergeConflict` for each entry. The substrate crate gains no async dependency; the daemon owns the channel side.

**Files:**
- Modify: `crates/memoryd/src/notifications/dispatcher.rs` — verify all 7 `NotificationEvent` variants have backend dispatch paths (osascript / Slack / SMTP / passive). Already wired per audit; only changes if a dispatch arm is missing.
- Create: `crates/memoryd/src/notifications/triggers.rs` — central registry mapping `EventKind` → `NotificationEvent` so emit sites use a uniform helper (`maybe_notify(EventKind::MergeQuarantined, ctx)`).
- Modify: `crates/memory-substrate/src/runtime/reconcile.rs` — extend `ReconcileReport` with `blocking_conflicts: Vec<String>`; populate it from the existing quarantine path. **Stream A surface touch authorized** for this additive return-shape change only.
- Modify: `crates/memoryd/src/server.rs` — at the existing reconcile call site, after `reconcile_all_phases` returns, iterate `report.blocking_conflicts` and emit `NotificationEvent::BlockingMergeConflict` per entry.
- Modify: `crates/memoryd/src/dream/orchestration.rs` — emit `NotificationEvent::DreamRunCompleted` at the existing dream-run completion path; emit `NotificationEvent::DailySynthesisSummaryReady` after the daily synthesis branch (the worker locates the daily branch via `rg -n 'daily_synthesis' crates/memoryd/src/dream/` — likely `orchestration.rs` or `run.rs`; if `run.rs` is the actual home, this task moves the emit there and that file remains in the Owned files list).
- Modify: `crates/memoryd/src/dream/run.rs` — secondary emit site for `DailySynthesisSummaryReady` if completion lives here rather than in `orchestration.rs`. Worker chooses one site, not both; the unused file may not need to be touched at all.
- Modify: `crates/memory-governance/src/review.rs` — define `pub const REVIEW_QUEUE_DOGFOOD_THRESHOLD: usize = 25` at the top of the file with a code comment marking it as a post-dogfood-configurable. Add `pub fn over_threshold(queue: &ReviewQueue) -> bool { queue.items.len() >= REVIEW_QUEUE_DOGFOOD_THRESHOLD }` (pure helper, no notification dependency). **`memory-governance` does not import `memoryd::NotificationEvent`** — v0.8 fix: v0.7 had governance emit the notification directly, which would require governance to depend on `memoryd`, inverting the crate dep. Governance stays pure: defines the threshold + crossing predicate, returns `bool`, nothing more.
- Modify: `crates/memoryd/src/handlers/mod.rs` (post-Task-9 path) — at the existing `RequestPayload::ReviewQueue` handler call site (locate via `rg -n 'ReviewQueue::from_envelopes\|review_queue' crates/memoryd/src/handlers/mod.rs`), after the queue is materialized, call `memory_governance::over_threshold(&queue)`; on `true`, emit `NotificationEvent::ReviewQueueOverThreshold { current_count: queue.items.len() }` via the dispatcher. **Notification semantics (v0.8 clarify):** `ReviewQueueOverThreshold` is a **passive-only** notification — the dispatcher routes it to the in-process notification log readable by `memoryd doctor` and the TUI overview panel, but does **not** fire the osascript / Slack / SMTP external channels (those are reserved for higher-urgency variants like `LeakedSecretDetected` once the privacy refactor lands). 25+ pending review is informational, not pageable.
- Modify: `crates/memoryd/src/reality_check/scheduling.rs` — emit `NotificationEvent::RealityCheckOverdue` when the scheduled-due check finds overdue items.
- Test: `crates/memoryd/tests/notification_fanout.rs` — trigger each of the 5 variants in turn; verify dispatcher receives them; verify passive-only fallback works when external channels disabled; verify `LeakedSecretDetected` is not emitted by any path in this build (deferred-scope sentinel).

```bash
git commit -m "feat(notifications): wire 5 unused variants to real emit sites; LeakedSecretDetected deferred"
```

---

### Task 29: Eval honesty — mock skip not pass for T13/T15 + T19 marker fix

**Parallel:** yes
**Blocked by:** none
**Owned files:** `crates/memorum-eval/src/harness_runner.rs:300-393`, `crates/memorum-eval/src/orchestrator.rs:225-686` (skip-detection path), `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs:16-18`, `crates/memorum-eval/tests/honesty.rs`, `.github/workflows/stream-h-eval.yml`
**Subagent:** `test_hardener`
**Sandbox:** `workspace-write`
**Reasoning:** `high`
**Skills to load:** `tdd`, `rust-engineer`
**Per-task gate:** `cargo test -p memorum-eval --tests && cargo clippy -p memorum-eval --tests -- -D warnings && cargo fmt -p memorum-eval -- --check`
**Worktree:** `../agent-memory-wt/task-29/` on `dogfood/task-29-eval-honesty`

**Files:**
- Modify: `harness_runner.rs:300-393` — `MockHarness::run_test_13` and `run_test_15` return `TestOutcome::Skipped { reason: "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED" }` instead of `Passed`. Mock mode is not a substitute for real-harness runs.
- Modify: `t19_peer_update_framing.rs:16-18` — `println!("MEMORUM_EVAL_SKIP:STREAM_I_DEPS_DISABLED")` (was `SKIP:` — wrong prefix). Now matched by orchestrator skip detector.
- Modify: `.github/workflows/stream-h-eval.yml` — CI gate fails if `partial: true` AND `harness_mode != "mock"` (allow partial in mock; reject partial in real). For mock mode, gate fails if any test reports `Passed` with `annotation contains "mode: mock"` (regression guard against the silent-mock-pass bug).
- Test: `crates/memorum-eval/tests/honesty.rs` — run mock; assert T13/T15 are `Skipped` not `Passed`; run with feature `stream-i-deps` off and verify T19 stub uses correct prefix.

```bash
git commit -m "fix(eval): mock skip not pass; T19 marker prefix; CI honesty gate"
```

---

## Phase 8 — Trust + atomic ops (final cleanup)

---

### Task 30: Atomic archival + event compaction

**Parallel:** yes
**Blocked by:** Task 28 (touches `runtime/reconcile.rs` — sequence after to be safe)
**Owned files:** `crates/memoryd/src/dream/cleanup.rs`, `crates/memoryd/src/dream/cleanup/atomic.rs` *(Create only if the atomic-write pattern repeats 3+ times in `cleanup.rs`; otherwise inline the helper and leave this file uncreated)*, `crates/memoryd/tests/cleanup_atomic.rs`
**Subagent:** `performance_engineer`
**Sandbox:** `workspace-write`
**Reasoning:** `xhigh`
**Skills to load:** `tdd`, `rust-engineer`, `clean-code`
**Per-task gate:** `cargo test -p memoryd --test cleanup_atomic && cargo clippy -p memoryd --tests -- -D warnings && cargo fmt -p memoryd -- --check`
**Worktree:** `../agent-memory-wt/task-30/` on `dogfood/task-30-atomic-archival`

**Stream A surface touch:** **none required**. Archival/compaction live in `crates/memoryd/src/dream/cleanup.rs`, not `crates/memory-substrate/`. Earlier draft incorrectly placed them in the substrate; corrected per round-1 review.

**Files:**
- Modify: `crates/memoryd/src/dream/cleanup.rs` — replace direct file-write-then-delete pattern in `write_archived_events` and `compact_event_logs` with: write to `<dest>.tmp`, fsync, rename to `<dest>`, fsync parent dir. Only after rename succeeds, prune source segments. Crash between fsync and rename: source intact. Crash between rename and prune: archive present, source still pruneable on next run (idempotent).
- Decision rule for atomic-write helper extraction: if the fsync-rename-fsync pattern appears 3+ times in `cleanup.rs` after the modifications above, extract `fn atomic_write(dest: &Path, contents: &[u8]) -> io::Result<()>` into `crates/memoryd/src/dream/cleanup/atomic.rs` (private to the cleanup module, not crate-level). If <3 occurrences, inline. Worker counts via `rg -c 'fsync\|tempfile' crates/memoryd/src/dream/cleanup.rs` after the implementation to make the call.
- Test: `crates/memoryd/tests/cleanup_atomic.rs` — inject a panic between fsync and rename; verify source segments intact and archive absent. Inject between rename and prune; verify archive intact and source still pruneable on a re-run. Verify final state matches non-crashed run.

```bash
git commit -m "fix(events): atomic archival + compaction with fsync-rename-fsync"
```

---

## Trunk gate + integration

After every task lands on `main`, the orchestrator runs:

```bash
bash scripts/check.sh
```

If any phase's tasks are integrated as a batch (e.g., Phase 2 install scripts integrate together), run trunk gate **once** after the batch — not after each task — to keep gate runtime bounded. The trunk gate must pass green before the next phase's tasks spawn.

After Task 30 lands, run:

```bash
bash scripts/check.sh                           # full release gate
bash scripts/install-memorum.test.sh            # install smoke
cargo run --bin memoryd -- doctor --reindex     # post-fix doctor honesty check
```

If all three pass: **plan complete, dogfooding can begin.** Trey installs, points Claude Code at `<runtime>/memoryd.sock`, and uses the system for real eng work for a week before the privacy refactor + ship pass.

---

## Plan revision history

- **v1.3 (2026-05-07):** `plan-reviewer` adversarial pass on Phase 5 v1.2 (Tasks 15, 16, 17A–17K, 18 + the new Cluster D coordination + the new lockfile cadence). Verdict: **APPROVED-WITH-FIXES**. Four blockers, six risks, three nits — all mechanical. Fixed against verified evidence (read the real `crates/memoryd-web/src/server.rs`, `src/main.rs`, `tests/`, and the actual handoff at `docs/design/dashboard-handoff/`). **Blockers fixed.** B1 — Task 17A said "Modify: `crates/memoryd-web/tests/frontend_smoke.rs`" but the file does NOT exist (verified: `tests/` contains only `api_contract.rs`, `concurrent_access.rs`, `csrf.rs`). v1.3 fix: 17A's annotation changed to "Create"; the explicit content spec landed in 17A's Files block; 17K's "Modify" annotation on the same file is now correct because 17A creates it. B2 — 17A's owned files list said `src/main.rs` for the rust-embed retarget, but `src/main.rs` is 7 lines of module re-exports; the actual `#[derive(RustEmbed)] #[folder = "static/"] struct Assets;` lives at `src/server.rs:33-35` (verified). v1.3 fix: 17A owns `src/server.rs` (not `src/main.rs`); the file modification description names the exact lines (33-35 for the folder retarget, 181-182 for the route deletion, 252-258 for the named-handler deletion). B3 — 17A's server.rs changes were underspecified for autonomous execution. Specifically the plan said "glob-match Vite's content-hashed asset filenames" without naming what happens to the named handlers `app_js()` and `style_css()` and the hardcoded routes `.route("/assets/app.js", get(app_js))` and `.route("/assets/style.css", get(style_css))` (verified at `server.rs:181-182, 252-258`). After the retarget, those routes 404 (Vite emits `index-a1b2c3.js`, not `app.js`) and the named handlers become dead code that fails clippy. v1.3 fix: 17A's Files block explicitly enumerates the three deletions (the two routes, the two named handlers, any leftover `APP_JS`/`STYLE_CSS` constants) plus the surviving wildcard `/assets/{*path}` route which already handles hashed filenames via `embedded_response(&path, content_type_for(&path))`. B4 — 17J added `nucleo-matcher` to `package.json` with a parenthetical "or fuse.js if nucleo lacks a JS port — orchestrator verifies on npm before spawning"; `nucleo-matcher` is a Rust crate on crates.io with no published JavaScript package, the orchestrator has no defined "verify on npm" preflight, and the parenthetical instruction was unexecutable. v1.3 fix: drop `nucleo-matcher` entirely, commit to `fuse.js@^7` as a runtime dependency in 17J's package.json edit; remove the unexecutable parenthetical. **Risks fixed.** R1 — visual snapshot baselines at `maxDiffPixelRatio: 0.005` would fail on Linux CI when generated from macOS dev (OKLCH gamut-mapping + font hinting differ between platforms). v1.3 fix: bump tolerance to `0.01` (1%) AND add `snapshotPathTemplate` scoping baselines per platform (`{testDir}/__snapshots__/{platform}/{testFilePath}/{arg}{ext}`). CI workflow regenerates Linux baselines on first run via `--update-snapshots`; subsequent runs diff. Documented in 17A's playwright config bullet. R2 — 17G's 60fps Recall scroll assertion at 9k events would fail without virtualization, but the plan said "virtualization required if needed" without mandating a library. v1.3 fix: mandate `@tanstack/react-virtual@^3.10` as a runtime dependency in 17A's package.json so 17G has it ready when the perf assertion fires; bundle budget in 17K accounts for the dep. R3 — 17I referenced `server.rs:175-198` for the route enumeration but Tasks 15+16 modify that range; line numbers will drift by the time 17I runs. v1.3 fix: 17I's wiring contract instructs the worker to enumerate routes by reading `router_with_state` and `protected_post_routes` by function name (search-by-name, not line number); the verified route set is listed in prose for cross-check. R4 — 17K's per-task gate concatenated `bash scripts/check.sh` (~9 min) on top of the full Playwright suite (270 visual snapshots + 42 a11y scans + perf assertion + e2e), comfortably exceeding the 20-minute default timeout from the failure-mode policy. v1.3 fix: split `bash scripts/check.sh` out of the 17K per-task gate (it runs as the post-Phase-5 trunk gate per the existing protocol at the "Trunk gate + integration" section); per-task gate exercises only what 17K owns. R5 — lockfile cadence section's "Workers update `package.json` only" wording contradicted the subordinate sentence about regenerating `pnpm-lock.yaml` in the worktree; a worker reading only the headline rule would skip `pnpm install` and trigger a trunk-gate failure. v1.3 fix: rewrite the cadence section so the headline rule matches the actual procedure: workers MUST run `pnpm install` in the worktree to regenerate `pnpm-lock.yaml` whenever they edit `package.json`, and commit both files together. R6 — 17H bundles three views (Peers, Governance, Entities) and is the heaviest single-view-port task in the chain. **Not fixed in v1.3** because Trey's explicit framing for this run is "find Codex's max-autonomy limits"; 17H staying as a single task is consistent with that framing. If 17H blocks at gate failure, the Cluster D-frontend critical path (17I/17J/17K) blocks too — that's a known and accepted risk for the run. **Nits fixed.** N1 — Phase 5 intro's "9-component React prototype" shorthand conflated components with views. v1.3 fix: reworded to "7 primary views plus shared infrastructure (Shell, Inspector with 10 kinds, ~13 UI primitives, icon set, Tweaks panel)." N2 — 17B's two-pass visual gate was implicit. v1.3 fix: explicit "Run gate twice" instruction in 17B's Step 3 with the rationale (first pass generates baselines; second pass diffs against self for determinism). N3 — fuzzy-match library lockdown specified explicitly in 17J's Files block (`fuse.js@^7` runtime dependency). **Things plan-reviewer explicitly checked and OK:** views.ts cross-task ownership (sequential through 17E→17F→17G→17H→17J via Blocked-by chain, no concurrent writes); Task 16→Task 15 ordering consistent across Phase 5 intro + each task's Blocked-by field; route enumeration in 17I matches the actual `server.rs` post-Tasks-15+16; deletion of `crates/memoryd-web/static/` is clean (no other crates reference those files; the existing `api_contract.rs` test uses CSRF + `/api/*` paths only); `scripts/check.sh` does not invoke the frontend build pipeline (its pnpm scope is workspace-root oxfmt + oxlint only); Task 18 is independent of Phase 5 (lives entirely in `crates/memory-source/`). **Verdict:** all four blockers fixed against verified evidence, all five actionable risks addressed (R6 explicitly accepted as known risk per run framing), all three nits cleared. Ready for execution greenlight or one more sanity pass.
- **v1.2 (2026-05-07):** Phase 5 web dashboard re-scoped against the Claude Design handoff at `docs/design/dashboard-handoff/`. **Source of change:** Trey commissioned a Claude Design pass against the brief in `docs/design/claude-design-brief/`; the result is a 9-component React prototype (Shell + Inspector + UI primitives + icons + 7 views: Inbox, Reality Check, Recall, Dreams, Peers, Governance, Entities) with 6 themes (`warm-dark` default plus `warm-light`, `cool-dark`, `cool-light`, `monochrome`, `high-contrast`), a 25 KB design contract README, a comprehensive Tweaks panel infrastructure, and ~32 KB of realistic mock data fixtures. The handoff explicitly says "do not copy the inline-Babel `<script type='text/babel'>` setup, the global `Object.assign(window, …)` exports, or the `useStateXyz` hook aliases — those are artifacts of running React without a build step. Lift the markup, layout, design tokens, copy, and interaction patterns; rebuild the wiring idiomatically." **Strategic decision (Option B):** rather than vanilla-JS port the prototype, add a real React + Vite + TypeScript build pipeline so the handoff maps near-1:1 and future view work stays idiomatic-React. Rejected Option A (stay vanilla, hand-port to DOM-mutation) on grounds that (a) ~120 KB of JSX across 9 components becomes pure invention labor, (b) the Tweaks panel is React-component-shaped infrastructure that a vanilla port would lose, (c) max-autonomy stress-testing the run benefits from broader scope. **Phase 5 expansion:** Task 17 (single "vanilla JS, no bundler" frontend task) replaced with **Tasks 17A through 17K** (eleven sub-tasks): 17A frontend toolchain bootstrap + e2e harness, 17B tokens + theme infrastructure + visual-regression baseline, 17C shell + UI primitives + icons, 17D Inspector composition (10 kinds + 15 cards), 17E Inbox view (4 layout variants), 17F Reality Check focus mode (5 variants), 17G Recall + Dreams views, 17H Peers + Governance + Entities views, 17I real data wiring (TanStack Query + CSRF + SSE + MSW), 17J Settings page + global keymap + command palette, 17K surface-state coverage + a11y + bundle budgets + integration sweep. Tasks 15, 16, 18 unchanged (backend handler-body work + URL redaction; design handoff doesn't change them). **Cluster D restructured:** the existing single Cluster D (Tasks 15/16/17 sharing `server.rs` + `static/app.js`) splits into **Cluster D-backend** (Tasks 15, 16 sequential through `server.rs`) and **Cluster D-frontend** (Tasks 17A–17K under `crates/memoryd-web/frontend/`, with 17A first; 17B/17C/17D parallel-safe after 17A; 17E/17F/17G/17H sequential through `frontend/src/views.ts`; 17I/17J/17K final integration). Backend and frontend subclusters are independent. **Lockfile cadence updated:** `pnpm-lock.yaml` moves from "likely zero deps" to a real lockfile under `crates/memoryd-web/frontend/`, orchestrator-merged at integration time the same way `Cargo.lock` is, with `pnpm install --frozen-lockfile` part of the trunk gate after Task 17A lands. **Build pipeline:** `crates/memoryd-web/build.rs` invokes `pnpm install --frozen-lockfile && pnpm run build` before Rust compilation; `rust-embed` retargeted from `static/` to `frontend/dist/`. The deployed binary remains a single Rust binary because rust-embed reads at compile time. **Old `crates/memoryd-web/static/` deleted** in 17A (vanilla shell removed; no longer reachable). **E2e ambition baked in (Trey's explicit ask: "robust ass e2e tests" with "objectively verifiable success criteria"):** every Task 17X gate is multi-layer — Vitest unit/component (jsdom + @testing-library/react), Playwright e2e (chromium against fixture daemon), Playwright visual regression (`toHaveScreenshot` per view × theme combination), @axe-core/playwright a11y (zero-violations contract), MSW for API fixtures in unit tests, bundle budgets (CSS ≤ 80 KB gzip, JS ≤ 250 KB gzip), CSP-strict verification (no inline scripts/styles in built `dist/`), and Recall ledger 60fps perf assertion at 9 k events. **Total visual snapshot baseline at end of phase:** 12 (themes) + 66 (primitives × themes) + 60 (inspector kinds × themes) + 24 (inbox layouts × themes) + 30 (reality-check variants × themes) + 24 (recall) + 24 (dreams) + 18 (peers/governance/entities) + ≈12 (settings/palette) = roughly 270 baseline snapshots. **Total a11y scans at end of 17K:** 7 views × 6 themes = 42, each must report zero axe-core violations. **Goal sentence updated** to mention the design handoff as the authoritative contract. **Inter-task coordination Cluster D section** rewritten for the backend/frontend split. **Phase 5 intro paragraph** rewritten to point at the handoff and explain the new sub-task structure. **What did NOT change:** Task 15 (entity graph + entity detail Rust handlers), Task 16 (policy editor + sync dashboard + reality-check history Rust handlers), Task 18 (URL redaction in `memory-source/`); Cluster A, B, C unchanged; Phases 0-4, 6-8 unchanged; system spec §14.1 v1 MCP surface unchanged; Stream G v0.1 data contract unchanged (this phase is presentation-only against shipped APIs). **Verdict:** ready for `plan-reviewer` adversarial pass scoped to Phase 5 (Tasks 15, 16, 17A–17K, 18) before execution greenlight. The reviewer should pay particular attention to (a) the build.rs / rust-embed retarget mechanics in 17A, (b) cross-task ownership of `frontend/src/views.ts` (sequential through 17E–17H), (c) whether 17I's MSW handler matrix actually covers every server route enumerated in `server.rs:175-198` after Tasks 15+16 land, (d) whether the visual-snapshot baseline strategy holds across CI environments (font rendering, subpixel antialiasing tolerance), (e) whether the 60fps Recall scroll assertion is realistic for chromium-headless CI, (f) whether `crates/memoryd-web/static/` deletion in 17A breaks anything that wasn't migrated to `frontend/dist/`.
- **v1.1 (2026-05-07):** `plan-reviewer` adversarial pass on Phase 4 (TUI section only; v1.0 was scoped to that phase, so this round is also Phase-4-scoped). **Five blockers fixed.** B1 — Task 13 dispatched a nonexistent `RealityCheckRequest::Correct { id, replacement, reason }` variant. The real protocol (verified against `crates/memoryd/src/protocol.rs:195-212`) is `RealityCheckRequest::Respond { session_id: String, memory_id: MemoryId, action: RealityCheckAction }`, with `RealityCheckAction::Correct { new_body: String }` carrying only the body string. v1.1 fix: rewritten Task 13 dispatches against the real shape, drops the editor's `reason` collection (the editor is now single-field for body only), uses `app.state.reality_check.session_id` for the session correlation, and adds an explanatory note that adding `reason` would be a separate additive protocol task out of scope for dogfood. Forget-with-reason uses a default `"user-forgot-via-tui"` until a richer prompt is added. B2 — `crates/memoryd-tui/src/state.rs` did not exist on disk, but Task 12 declared `Modify:`; the worker would have failed to find the file. v1.1 fix: Task 11 owns `Create:` of `state.rs` (carrying `RealityCheckState` extracted from the deleted `panels/reality_check.rs`), Task 12's annotation is correctly `Modify:`. B3 — Task 11's `app.rs` rewrite referenced `crate::focus::FocusKind`, but the `focus/` module was scheduled for creation in Task 12, so Task 11's per-task gate (whole-crate compile) would have failed. v1.1 fix: Task 11 creates `focus/mod.rs` as a stub with the `FocusKind` enum (`None | RealityCheck | CorrectEditor`) and an empty render dispatch; Tasks 12 and 13 fill the typed arms via `Modify:`. B4 — `BorderGlyphs` was missing from `memorum-theme`'s `lib.rs` re-exports despite being the type the `theme_glue` seam imports; the inline mid-bullet `**Wait —**` correction in `border.rs`'s spec also left two contradictory instructions in sequence. v1.1 fix: `lib.rs` re-exports list now includes `BorderGlyphs` and `ResolvedTheme` and `ResolvedColor`; the `border.rs` bullet rewritten cleanly with the corrected intent and the full glyph-set field list. B5 — Task 10A's Cargo.toml pinned `notify = "6"` while the workspace root has `notify = "8.0"` (verified at `Cargo.toml:35`); a major-version mismatch would have pulled two notify copies into the workspace. v1.1 fix: `notify.workspace = true` inheriting the workspace pin; added a note that the worker uses notify 8.x's `RecommendedWatcher` builder + `Config` API, not v6 muscle memory. **Six risks addressed.** R1 — per-crate `clippy.toml` shadows the workspace one rather than merging; the workspace's three thresholds (`too-many-lines-threshold = 60`, `cognitive-complexity-threshold = 15`, `too-many-arguments-threshold = 4`) would silently disappear on the TUI crate. v1.1 fix: Task 11's clippy.toml content explicitly mirrors all three thresholds alongside the new `disallowed-methods` / `disallowed-types` rules. R2 — `nucleo-matcher = "0.3"` is outdated; current is 0.5.x. v1.1 fix: bumped to `"0.5"` with an instruction for the orchestrator to verify the latest published version on crates.io before spawning. R3 — `tests/hot_reload.rs` and `tests/theme_hot_reload_e2e.rs` originally said "assert within 1s," which is flaky on slow CI given macOS FSEvents debounce variability. v1.1 fix: both tests use a poll-with-backoff pattern (check every 50ms for up to 2s via `tokio::time::timeout` wrapping a `loop`) instead of a single sleep-and-check. R4 — `Resolver::detect()` had no override path for misdetection (e.g. `TERM=screen-256color` inside a true-color tmux pane). v1.1 fix: added `MEMORUM_FORCE_COLOR=truecolor|256|16|mono` env override and `--color-capability` CLI flag (precedence: CLI > env > auto-detect); Task 14B's runbook documents the common misdetection scenarios. R6 — Task 12 said the slide-in transition was "driven by `frame.count()`," but ratatui 0.29's `Frame` does not expose that; the public counter is `Terminal::frame_count()`. v1.1 fix: Task 12 explicitly adds an App-tracked `tick_counter: u64` field incremented in `on_tick()` and uses `tick_counter * tick_ms` for transition progress, with `--no-motion` forcing immediate completion. R7 — Task 11B's `theme:switch` command had no save path. v1.1 fix: added `theme:save-as <name>` to the command catalog (writes to `~/.config/memorum/themes/<name>.toml` or, if name is omitted, to `~/.config/memorum/theme.toml`). **Risks not actionable:** R5 (`/tmp/memorum-tui-mockup.html` volatility) — design is captured textually in the plan, file is reference-only. R8 (border.rs sloppy correction) — folded into B4. **Plan-reviewer non-actionable:** Task 12's `panels/reality_check.rs:89` reference is clearly historical ("that file is deleted in Task 11") and v1.1 tightens the prose further. **Verdict:** all five blockers fixed against verified evidence (read the real `protocol.rs`, real `Cargo.toml`, real `clippy.toml`, real absence of `state.rs` and `focus/`); ready for execution greenlight or one more sanity pass.
- **v1.0 (2026-05-07):** TUI redesign + Day-1 theming. Phase 4 fully rewritten. **Goal pivot:** the v0.9 plan would have shipped the existing 9-panel tab-bar TUI with sample-fixture-replacement only; v1.0 reframes the work as a real redesign — a unified inbox + inspector + filter pills + command palette, with theming as a foundational invariant rather than a deferred polish item. Reference design captured in `/tmp/memorum-tui-mockup.html`. **Cluster C reshape:** sequential **11 → 11B → 12 → 13 → 14B** (Task 11 = shell rewrite, Task 11B = command palette, Task 12 = Reality Check focus mode, Task 13 = inline correct editor, Task 14B = preset/capability/hot-reload validation). Task 10A precedes Cluster C as a non-collision blocker — entirely inside the new `crates/memorum-theme/` crate. Task 14 stays in Cluster B (unchanged content; consumer surface moved to inspector's policy block). Task 11A stays in Cluster A (same five protocol payloads, framing reworded for the new consumers). **New crate `memorum-theme`** (Task 10A): exhaustive 23-token `ColorTokens` set, OKLCH input, terminal-capability-aware resolution (TrueColor → Indexed256 → Indexed16 → Monochrome), six shipped presets (`default-warm-dark`, `default-light`, `kanagawa`, `gruvbox`, `catppuccin-mocha`, `tokyo-night`), TOML config at `~/.config/memorum/theme.toml` with hot-reload via `notify`, charset detection (`Full`/`Extended`/`Minimal`) with ASCII glyph fallback, configurable `Glyphs`/`BorderStyle`/`Density`/`MotionConfig`/`Keymap`. Crate has zero ratatui dep — `theme_glue.rs` in memoryd-tui (Task 11) is the only seam. Stream G v0.1 spec contract is preserved (data exposed and protocol surface unchanged); presentation only. **New `crates/memoryd-tui/clippy.toml`** (Task 11) blocks `Style::default` and direct `Color::*` constructors outside the `theme_glue` module — every render path is forced through `&Theme`, no token can be silently bypassed. **Test coverage migration is explicit** in Task 11's Files block (old `panels/*.rs` and `tests/panel_render.rs` / `recall_panel.rs` / `keymap.rs` are deleted; coverage moves to `tests/inbox_render.rs` + `inspector_router.rs` + `filter_pills.rs` + `keymap_actions.rs` with the four kept tests — `resize`, `socket_unreachable`, `trust_artifact`, `panic_restore` — rewired to the new App constructor signature only). **Task 11B** introduces `nucleo-matcher` for fuzzy command matching; commands are read-only or theme-only in v1.0 (destructive operations stay behind item-level keys to prevent accidental palette dispatch). **Task 12** kills the v0.9 hardcoded `"0 of 12"` at the deleted `panels/reality_check.rs:89` site; Reality Check becomes a focus-mode takeover view with a 350ms motion-respecting slide-in transition. **Task 13** swaps the v0.9 modals/correct stub for an in-pane editor inside focus mode; preserves session context (side rail and progress gauge stay visible). **Task 14B** validates the loop end-to-end — six-preset render smoke, charset minimal fallback, 16-color floor, hot-reload-on-malformed-TOML banner. New runbook at `docs/runbooks/tui-theming.md`. **What did NOT change:** Task 11A's 5 protocol payloads (only their consumer mapping); Task 14's `STREAM_I_PLACEHOLDER` fix; system spec §14.1 v1 MCP surface (still 10 tools); Stream G v0.1 data contract; the four preserved test fixtures. **Verdict:** ready for `plan-reviewer` adversarial pass scoped to Phase 4 (Tasks 10A, 11A, 11, 11B, 12, 13, 14, 14B) before execution.
- **v0.9 (2026-05-07):** Fire-and-forget hardening pass. Five rounds of plan review are complete (v0.5–v0.8 in this history); the "Pre-execution adversarial review" section was removed because its work is done — keeping it would have read as a required Step 1 to an autonomous orchestrator and blocked the run. **Added "Fire-and-forget operating manual" section** at the top of the plan with five subsections: (1) Launch settings — `approval_policy: never` is required; without it the run halts on the first `cargo test`. (2) Per-task brief template — verbatim string template the orchestrator fills from each task's metadata to spawn the subagent, including explicit skill-loading instructions (per the Codex inventory: skills do not auto-load from frontmatter — they must be named in the brief). (3) Failure-mode policy — six explicit rules covering per-task gate failure, ff-merge failure, lockfile conflicts, trunk-gate regressions, command timeouts, and orchestrator context exhaustion. The default policy is "mark task `Blocked` in `update_plan` + log to `docs/plans/dogfood-execution-log.md` + continue to the next task whose dependencies are satisfied" — no rule pauses for operator input. (4) Lockfile reconciliation cadence — `cargo update --workspace --offline` runs as a separate commit when `--locked` build fails post-integration; pnpm only matters for Task 17 (no bundler, likely zero deps). (5) Checkpoint/resume protocol — orchestrator reads `git log --oneline main` + `dogfood-execution-log.md` + the plan to pick up after a session crash; same `/goal` invocation resumes. **Task 8 sandbox fix:** `install-launchd.sh` now honors `MEMORUM_LAUNCHAGENTS_DIR` (override target dir, default `~/Library/LaunchAgents/`) and `MEMORUM_LAUNCHD_INSTALL_ONLY=1` (skip `launchctl bootstrap`); the test runs against a tempdir with both env vars set, asserting plist file structure and exact `ProgramArguments` arrays without ever invoking `launchctl` — keeps Task 8 inside `workspace-write` sandbox. **Ambiguity scrub:** Task 6's `Parallel: yes (with Tasks 4-5 if no main.rs collisions; safer to wait...)` deterministic-ified to `Parallel: no, Blocked by: Task 5`. Task 27's "Optionally, the orchestrator can spawn `prompt_engineer` first" removed — single-agent `heavy_worker` path is mandated. Three "or whatever the local accessor is" hand-waves replaced with explicit `rg` commands the worker uses to find the actual binding name, with the invariant field paths called out separately. Task 30's atomic-helper extraction now has a numeric decision rule (`if 3+ occurrences`) instead of "if extraction makes sense per worker's judgment". **Verdict:** ready for autonomous fire-and-forget execution. The launch invocation is a single `/goal` against this plan path with `approval_policy: never`.
- **v0.8 (2026-05-07):** Codex `plan_reviewer` round-4 pass patched. **Four blockers fixed:** B1 — Task 27's prompt-version threading reached `DreamRunOptions` and `render_prompt` but **missed the actual seam** where loaded config becomes a build request. `crates/memoryd/src/main.rs:801` defines `execute_dream_run(invocation)`; `main.rs:807` constructs `DreamRunBuildRequest { ... }` from `invocation.dreams.{...}`; that struct (defined at `dream/orchestration.rs:52`) is the entry point to the pass pipeline. Without `prompt_version` on `DreamRunBuildRequest` and a populate at main.rs:807, the new field would be dead in the dispatch path. v0.8 fix: Task 27 owns `crates/memoryd/src/main.rs`, adds `prompt_version` to `DreamRunBuildRequest` (orchestration.rs:52), populates from `invocation.dreams.prompt_version` at main.rs:807 and from the loaded `DreamsConfig` at the handlers.rs:1323 secondary construction site. New test `tests/dream_build_prompt_version.rs` exercises the actual `execute_dream_run` build path end-to-end (V1/V2 fixtures), not just `render_prompt` in isolation. B2 — Task 28 had `crates/memory-governance/src/review.rs` emit `NotificationEvent::ReviewQueueOverThreshold` directly. `memory-governance` does **not** depend on `memoryd` (`Cargo.toml` confirms — no memoryd dep, zero `NotificationEvent` references in the crate); making governance import memoryd's notification type inverts the crate dep. v0.8 fix: governance stays pure — exports `pub const REVIEW_QUEUE_DOGFOOD_THRESHOLD: usize = 25` plus `pub fn over_threshold(queue: &ReviewQueue) -> bool`, no notification awareness. memoryd's `handlers/mod.rs` (post-Task-9 path) at the `RequestPayload::ReviewQueue` handler site calls `over_threshold(&queue)` and emits the notification when `true`. Task 28 joins Cluster A (sequence: `2 → 9 → 11A → 19 → 20 → 21 → 22 → 23 → 24 → 25 → 28`) because it now owns `handlers/mod.rs`. Notification semantics clarified as **passive-only** (logs to in-process notification log readable by doctor/TUI, no osascript/Slack/SMTP external dispatch — 25+ pending is informational, not pageable). B3 — Task 8's plist test was prose-only ("verify both plists installed"). v0.8 adds a falsifiable `ProgramArguments`-shape assertion: the test parses the installed daemon plist (`plutil -convert xml1` or `defaults read`) and asserts the exact array `["memoryd", "serve", "--repo", "<repo>", "--runtime", "<runtime>", "--socket", "<runtime>/memoryd.sock"]`. Any drift fails loudly. Same shape check for the dream-scheduled plist. B4 — The "For Codex" header said "spawn the named subagent per task with `multi_agent`"; per the Codex agent inventory, `multi_agent = true` is the **feature flag** that enables spawning, not the verb. The verb is `spawn_agent` (paired with `wait_agent`). v0.8 fixes the header phrasing. Plus Tasks 19-30 were missing the trailing `on dogfood/task-NN-<slug>` branch metadata — every task now carries it consistently. **Risks fixed:** Task 27 prose contradicted itself on the default for the `prompt_version` field (claimed both "V2 for new installs / V1 for omitted" and "V2 via serde default"); v0.8 settles on **V2 everywhere** (consistent with the dogfood goal of all installs running the new prompts on upgrade). Task 28 notification semantics ambiguity (passive vs. operator-visible) resolved as passive-only. Task 9's stale Task-28 sequencing reference (v0.7 erroneously said Task 28 was outside Cluster A) corrected. **Verdict:** ready for Codex execution greenlight or one more sanity check.
- **v0.7 (2026-05-07):** Codex `plan_reviewer` round-3 pass patched. **Six blockers fixed:** B1 — Task 18's `WebCaptureManifest.redirect_chain[i].location` field (the raw `Location:` header value, distinct from the hop's `url`) was missed in v0.6's redaction list; v0.7 adds it explicitly with a `redact_sensitive_location_header(raw, base)` helper that handles absolute and relative `Location:` values uniformly. Tests cover a `302 → /reset?token=...` flow. B2 — Task 8's daemon LaunchAgent template lacked `--repo {{REPO_PATH}}` despite the existing dream-scheduled plist already having it; without `--repo`, `memoryd serve` cannot locate the substrate. v0.7 fixes the `ProgramArguments` block and aligns log paths with the dream plist convention (`{{RUNTIME_PATH}}/daemon.{out,err}.log`, not `~/Library/Logs/`). B3 — Task 27's `prompt_version` was wired into `crates/memoryd/src/dream/config.rs`, which is the local cleanup-config file, **not** the loaded config path that flows from `memoryd serve`. The actual path is `memory_substrate::config::DreamsConfig` (line 81 of `config/mod.rs`) → consumed when `DreamRunOptions { ... }` is constructed at `dream/orchestration.rs:78` from the loaded config → passed to `render_prompt` (which currently lacks a version param). v0.7 fix: thread `prompt_version` through that path — additive `prompt_version: PromptVersion` field on `DreamsConfig` (Stream A surface touch authorized for additive only), `DreamRunOptions` field add at `dream/run.rs:27`, populate at orchestration.rs:78, change `render_prompt` signature, update all four call sites at `pass1.rs:28`, `pass2.rs:34`, `pass3.rs:39`, and any in-file calls in `dream/run.rs`. B4 — Task 28's `governance.review_queue_threshold` config field does not exist (no `GovernanceConfig` struct in the substrate); v0.6 said "configurable via" without specifying where. v0.7 fix: drop the configurable claim, hardcode `pub const REVIEW_QUEUE_DOGFOOD_THRESHOLD: usize = 25` in `memory-governance/src/review.rs` with a code comment marking it as a post-dogfood follow-up. B5 — Tasks 9, 13, 25, 30 still ran test-only gates without `cargo clippy --tests -- -D warnings && cargo fmt -- --check`; v0.7 adds them. B6 — The "For Codex" header pointed at `scripts/spawn-task-worktree.sh` and `scripts/integrate-task-worktree.sh`, but those helpers hardcode the `stream-a/` branch prefix (this plan uses `dogfood/`) and the integrate helper's "narrow" gate runs `cargo test --workspace` (CLAUDE.md forbids `--workspace` inside task worktrees because stub modules from unstarted tasks fail for the wrong reason). v0.7 fix: helper-script note clarifies the helpers are reference-only; the orchestrator uses explicit `git worktree add -b dogfood/task-NN-<slug>` + the per-task narrow gate from each task's "Per-task gate" line + explicit ff-merge + worktree cleanup. **Non-blocking risks fixed:** Task 9's "parallel-safe" prose was stale vs. the new Cluster A sequence (`2 → 9 → 11A → 19 → 20 → 21 → 22 → 23 → 24 → 25 → 28`); v0.7 clarifies "parallel" means "with non-Cluster-A tasks". Task 22's API doc path was `stream-b-mcp-api.md` — actual filename is `stream-b-daemon-mcp-api.md`; fixed. Task 15 said "Parallel: no" while Task 16 said "Parallel: yes (with Task 15)" but Task 15 listed Task 16 as a `Blocked by` — contradictory; v0.7 fixes both to "Cluster D — sequential 16 → 15 → 17, with Task 18 outside the cluster". Task 16's CSRF-and-localhost phrasing conflated two different protections (CSRF is request-provenance, 127.0.0.1 bind is reachability); v0.7 clarifies that both layers must hold. Task 10's `docs_editor` subagent is read-only per the Codex inventory; changed to `worker` (write-capable, no domain specialization needed for mechanical find-and-replace). **Verdict:** ready for Codex `plan_reviewer` round-4 sanity check or execution greenlight.
- **v0.6 (2026-05-07):** Codex `plan_reviewer` round-2 pass patched. **Eight blockers fixed:** B1 — Task 4 did not own `crates/memoryd/src/cli.rs` despite `Mcp(SocketArgs)` (cli.rs:76) needing replacement with `Mcp(McpArgs { socket, repo, runtime, auto_start })` so the MCP bridge can spawn `memoryd serve` correctly. v0.6: Task 4 owns `cli.rs` and replaces the args struct; resolution of `socket` (still optional) happens in main.rs at dispatch. B2 — Task 5's `cli.rs:63-67` callout covered only one of **12+** `default_value = "/tmp/memoryd.sock"` annotations across `SocketArgs`, `UiArgs`, and every connect-only subcommand's Args struct (search, get, status, write, write-note, supersede, forget, recall, peer, reality-check, source, dream-admin). v0.6: Task 5 replaces every static default with `socket: Option<PathBuf>` (no clap default), introduces `socket::default_runtime_root()` reading `MEMORUM_RUNTIME` env var, and resolves at dispatch in main.rs at every site. Includes a parameterized test that loops through all 12+ subcommand dispatch paths and asserts they hit `resolve_socket_path`. B3 — Task 1 said "add to root `Config`" — there is no root `Config` in the substrate. Real shapes are `SyncedConfig` (synced via git), `LocalDeviceConfig` (per-device, never synced — spec invariant #4), `LoadedConfig` (resolved precedence). v0.6: privacy enforcement field added to `LocalDeviceConfig` (per-device, NOT synced), accessed via `LoadedConfig::privacy_enforcement()`, installed by main.rs from the loaded config. B4 — Task 1's `OnceLock` test design used a single test binary with `#[serial]`, which doesn't isolate the process-global `OnceLock` between cases (the second test's install always errors and the first test's enforcement persists for the rest of the binary). v0.6: split into three separate test binaries (`tests/privacy_runtime_install_classifier_off.rs`, `tests/privacy_runtime_install_full.rs`, `tests/privacy_runtime_install_double.rs`) — each `tests/<name>.rs` is a separate cargo test binary with its own process and fresh `OnceLock`. B5 — Task 2 placed MCP `memory_write` behavior tests in `crates/memory-governance/tests/dogfood_defaults.rs`. `memory-governance` is the policy engine and does not depend on `memoryd`; MCP-write behavior tests belong in `crates/memoryd/tests/handler_contract.rs`. v0.6: governance test scoped to "policy gate accepts `confidence: 0.85` when grounding satisfied"; all MCP/handler-level behavior tests moved to `handler_contract.rs`. B6 — Task 18 said "redact the captured URL" (singular). `WebCaptureManifest` persists **four** URL fields (`original_url`, `final_url`, every `redirect_chain[i].url`) plus the response's `final_url` to the caller. v0.6: redact **every** URL at **every** persistence/return site; tests verify each. B7 — Task 16 said "POST validates + writes (CSRF-protected)" without specifying where the route registers in the existing `protected_post_routes` block at `server.rs:175-178` (which already wraps `/api/reality-check/respond` and `/api/review/action` in `require_csrf` middleware). v0.6: explicit registration of `/api/policy-editor` POST inside `protected_post_routes` so it inherits CSRF protection automatically; GET stays in the public router. B8 — Task 27 had only `clean-code` despite touching 7 Rust files (`prompts.rs`, `config.rs`, `orchestration.rs`, `run.rs`, `pass1.rs`, `pass2.rs`, `pass3.rs`). v0.6: adds `tdd`, `rust-engineer`, plus `cargo clippy --tests -- -D warnings` and `cargo fmt -- --check` to the per-task gate. **Verdict:** ready for Codex `plan_reviewer` round-3 pass.
- **v0.5 (2026-05-07):** Codex `plan_reviewer` round-1 pass patched. **Six blockers fixed:** B1 — Task 1's privacy enforcement flag was added to the config struct but never reached the call sites; v0.4 had `Classifier::with_enforcement(...)` available but all three handler call sites at `handlers.rs:1828, 2413, 3470` still constructed `DeterministicPrivacyClassifier::new()` inline with no AppState handle. v0.5 fix: `OnceLock<PrivacyEnforcement>` in `memory-privacy::policy` set by `memoryd::main` at serve startup, read by `::new()` so existing call sites inherit the runtime config without a structural refactor. Task 1 now joins Cluster B (first slot) and owns `main.rs`. B2 — Task 11's "panels call `memoryd review pending` / `memoryd inspect entities`" was wrong: the TUI dispatches `protocol::Request` payloads via `client.dispatch_daemon_call`, not CLI subprocesses, and 5 of the 6 panels in scope have no corresponding `RequestPayload` variant in `protocol.rs`. v0.5 fix: insert Task 11A into Cluster A (slots between Task 9 and Task 19) to add the 5 missing daemon-protocol read endpoints (`InspectEntities`, `EventsLogPage`, `NamespaceTree`, `GovernancePolicyDump`, `ConflictsList`) before Task 11 wires the TUI; Task 11 reframed against typed protocol payloads. B3 — Task 18 (URL redaction) owned files in `crates/memoryd-web/src/source_capture/url.rs`, a path that does not exist; URL parsing + capture pipeline lives in `crates/memory-source/src/url_safety.rs` + `capture.rs`. v0.5 relocates the task to `memory-source/`. B4 — Task 17's `app.js` polled five fabricated routes (`/api/recall`, `/api/review/pending`, `/api/entity/graph`, `/api/policy`, `/api/sync`); actual server routes are `/api/recall-hits`, `/api/review`, `/api/entity-graph`, `/api/policy-editor`, `/api/sync-dashboard` (from `server.rs:175-198`). Static filename was `styles.css` → actual is `style.css` (singular). v0.5 fixes both. B5 — Task 4 referenced `socket::probe_live_socket` and noted "Task 4 stubs the function signature, Task 5 lands the impl" but Task 4's owned files did not include `crates/memoryd/src/socket.rs` — Task 5 owned the `Create:`. v0.5 fix: Task 4 owns the `Create:` of `socket.rs` with stub returning `Absent`, Task 5 changes from `Create:` to `Modify:` with the real probe + bind logic. B6 — `rust-engineer` skill missing from most Rust-touching tasks despite `CLAUDE.md` saying to reach for it proactively. v0.5 adds it to Tasks 1, 2, 3, 4, 5, 6, 11A, 12, 14, 15, 16, 18. **Risks fixed:** R5 — Task 2 referenced a nonexistent helper `write_governance_meta()`; v0.5 reframes against the real surface (private `GovernanceMeta::default()` impl at `handlers.rs:2965` and `GovernanceWriteInput::parse` at `3020`) by introducing a path-specific `GovernanceMeta::for_mcp_human_write()` constructor with a `MetaSource` enum dispatch so dream/observe paths stay strict. R6 — Task 28 referenced `crates/memory-governance/src/review_queue.rs` (not a real file; actual is `review.rs`) and `crates/memory-privacy/src/audit.rs` (does not exist). v0.5 fixes the review path and narrows scope to 5-of-6 unused notification variants; `LeakedSecretDetected` is deferred to the post-dogfood privacy refactor where the new audit module is the appropriate home. R7 — Per-task gates were test-only with no `cargo fmt --check` or `cargo clippy --tests -- -D warnings`. v0.5 adds both to every Rust task gate as a per-crate suffix. R8 — Pre-execution review item #4 referenced `scripts/owned-files-check.sh` which did not exist on disk. v0.5 creates the script with brace-expansion handling, parens-aware comma splitting, within-task dedup, and an allowed-serial-paths list for cluster-sequenced files; runs cleanly against this plan. **Other fixes:** Cluster A note updated to include Task 9 (handlers.rs → handlers/mod.rs conversion) and Task 11A (sequence: 2 → 9 → 11A → 19 → 20 → 21 → 22 → 23 → 24 → 25); Cluster B note updated to include Task 1 (sequence: 1 → 3 → 4 → 5 → 14); Cluster D note narrowed to Tasks 15-17 (Task 18 left the cluster when relocated); Task 15 framing clarified as "replace deferred-response stub" not "add new route"; Tasks 22 and 30 owned-files lines stripped of prose-prefix that was confusing tooling. **Verdict:** ready for Codex `plan_reviewer` round-2 pass.
- **v0.4 (2026-05-07):** Round-3 (final) plan-reviewer pass patched. **Blockers fixed:** B1 — Tasks 20, 21, 23, 24, 25 owned-files headers and "Modify:" lines updated to reference `handlers/mod.rs` (post-Task-9 module-conversion path) instead of the now-stale `handlers.rs`; the Task 9 cluster note alone was insufficient since workers read their own task header first. B2 — Phase 7 preamble corrected: sequencing is `Task 26 → Task 27 → Task 28` (all three touch `dream/orchestration.rs`); Task 27 now declares `Blocked by: Task 26`; Task 29 remains independent. **Residual risk acknowledged (R1):** Task 26's `DreamError`/`diagnostic` field surface may require a small struct/enum extension; this is implementable within the task and the worker should plan for ~10–20 lines of error-type plumbing rather than treat it as a two-line change. **Other residual risks (R2/R3/R4):** Task 20 owned files now include `mcp.rs` for the schema modification (R2); trunk gate phasing (R3) deferred to orchestrator judgment per established Stream H practice; Task 27 `render_prompt` cascade (R4) covered by existing owned-files list. **Final verdict from round 3:** APPROVED FOR EXECUTION after these mechanical fixes.
- **v0.3 (2026-05-07):** Round-2 plan-reviewer pass patched. **Blockers fixed:** B6 Task 4 commit message corrected (no longer references the dropped `memory_status` tool); B7 Task 11 blocked-by reason corrected (no tool reference); B8 Task 9 explicit `handlers.rs` → `handlers/mod.rs` module-conversion steps added with verification step and Cluster A path-rewrite note for Tasks 19–25; B9 Task 28 BlockingMergeConflict redesigned — `ReconcileReport` gains `blocking_conflicts: Vec<String>` field (return-shape extension), daemon emits notification post-reconcile (substrate stays sync, no async dep injected). **Risks fixed:** R9 Task 1 Luhn path explicitly moves into `SecretOnlyScan` (not split between layers); R10 Task 27 owned-files now include `dream/run.rs`, `pass1.rs`, `pass2.rs`, `pass3.rs` call sites; R11 Task 28 emit-site claim sharpened; R12 Task 4 gate narrowed to skip probe-dependent tests until Task 5 lands the impl. **Nits fixed:** Task 22 §14.1 amendment placement (inside section, not EOF); Task 19 `mcp.rs` and `protocol.rs` added to owned files with Task 22 rebase note.
- **v0.2 (2026-05-07):** Round-1 plan-reviewer pass patched. **Blockers fixed:** B1 invariant #1 preservation — Task 1 redesigned with two-layer `SecretOnlyScan` (always on) + `FullClassifier` (gated) so `secret` content is refused before disk regardless of enforcement; B2 system spec §14.1 freeze — Task 4 drops `memory_status` MCP tool (daemon-protocol-only via socket), Task 22 ratifies 10-tool surface via §14.1 amendment; B3 Task 30 file paths corrected to `crates/memoryd/src/dream/cleanup.rs` (not the nonexistent `memory-substrate/src/events/`); B4 Task 27 prompt files relocated to repo-root `prompts/`; B5 Task 14 sequenced into Cluster B. **Risks fixed:** R1 Cluster D task numbering corrected (15-18 not 16-19); R2 Task 28 emit-site moved off `handlers.rs` to `dream/orchestration.rs` so no Cluster A dependency; R3 Task 15 explicitly blocks on Task 16 for `server.rs` ordering; R4 Task 9 blocks on Task 2 for handlers.rs collision; R5 `DailySynthesisSummaryReady` emit site relocated; R6 Task 27 reassigned to `heavy_worker`; R7 PromptVersion enum-creation made explicit; R8 Task 7 gate-script note added. **Nits fixed:** Stream A surface touch list corrected; Task 22 `rg` invocation made specific; Task 18 two-phase orchestration spelled out as explicit steps; redundant `write-human` skill loads dropped from Tasks 10 and 27.
- **v0.1 (2026-05-07):** Initial draft. Synthesized from Claude six-Explore-agent audit + Codex five-explorer audit + Trey's privacy-flag direction. 30 tasks across 8 phases. Out of scope: privacy classifier rewrite (deferred to post-dogfood ship pass), benchmarks recapture, Tier-3 deferred items.
