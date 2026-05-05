# Dogfood Readiness Claude Review Fix Implementation Plan

**Goal:** Close every actionable issue in `docs/reviews/2026-05-04-dogfood-readiness-claude-review.md` without hidden fallbacks, misleading green checks, or fossilized placeholder contracts.

**Architecture:** Use vertical TDD slices by surface: installer lifecycle, live eval truthfulness, TUI recall honesty, daemon doctor health, startup peer-update regression coverage, and hygiene/docs decisions. Prefer hard cutover behavior: either the production behavior is real and documented, or the unsupported behavior is removed from the user surface and tracked explicitly.

**Tech Stack:** Rust 2021 workspace (`memoryd`, `memoryd-tui`, `memorum-eval`, `memorum-coordination`), Bash installer scripts, Markdown runbooks/reviews/plans.

---

## Source Review Checklist

- B1: detach installer-started daemon; persistent runtime log; PID file; lifecycle stdout; runbook update.
- B2: live eval smokes must gate on exactly the keys/CLIs their test paths require; no subprocess skip may look like pass; runbook truthfulness.
- B3: Recall panel must not render or test `score:n/a`, `harness:n/a`, `session:n/a`; design doc must match shipped protocol surface and track real-fields work.
- R4: add regression coverage for same-startup peer-write cooldown seeding across same-device to cross-device pass.
- R5: doctor health must be false when no enabled harness authenticates, while missing one harness remains a warning if another is OK.
- R6: rerun and record the canonical `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` gate after fixes.
- R7: run independent cleanup/clean-code/security review passes and preserve reports under `docs/reviews/`.
- N8: installer skips `cargo install` when installed `memoryd --version` equals `crates/memoryd/Cargo.toml`.
- N9: implement a real mid-render PTY panic restore test; documentation-only is not accepted.
- N10: compile-time-gate EchoCli and echo selection; runtime-gate documentation-only is not accepted.
- N11: document the bump-vs-amend spec convention that explains Stream A/F asymmetry.
- N12: document/centralize full-coordination harness allowlist invariants.

## Task 1: Installer lifecycle hard cutover (B1, N8)

**Parallel:** yes
**Blocked by:** none
**Owned files:** `scripts/install-memorum.sh`
**Coordinator-owned docs:** `docs/runbooks/dogfooding-day-one.md`
**Invariants:** Default install starts exactly one daemon for the requested runtime/socket; logs persist under runtime; dry-run remains side-effect free.
**Out of scope:** launchd installer internals except references from the lifecycle stanza.

**Files:**
- Modify: `scripts/install-memorum.sh`
- Modify: `docs/runbooks/dogfooding-day-one.md`

**Step 1: Add installer version-skip behavior.**
- Read expected version from `crates/memoryd/Cargo.toml` with shell-safe parsing.
- If `memoryd` exists and `memoryd --version` reports the same version, print `memoryd vX.Y.Z already installed; skipping cargo install` and do not invoke `cargo install`.
- Otherwise run the existing `cargo install --path crates/memoryd --locked`.
- Dry-run prints the branch it would take without installing.

**Step 2: Add deterministic runtime paths.**
- Define `pid_file="$runtime/memoryd.pid"` and `log_file="$runtime/memoryd.log"` after runtime defaulting.
- `mkdir -p "$repo" "$runtime"` before touching log/pid.

**Step 3: Start detached daemon with PID file.**
- If `memoryd.pid` exists and `kill -0` succeeds, stop that process and wait briefly; otherwise remove the stale PID file.
- Rerunning the installer is a real restart for that runtime/socket, not a competing daemon.
- Start with `nohup memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" </dev/null >>"$log_file" 2>&1 &`.
- Capture `$!`, `disown` when available, probe readiness, then write `$daemon_pid` to `$pid_file` only after readiness succeeds.
- On readiness failure, kill the child, remove pid file, and point stderr at `$log_file`.

**Step 4: Print lifecycle stanza and update runbook.**
- After MCP snippet, print running PID/log, stop, restart, and scheduler commands.
- Runbook records PID/log paths, `kill $(cat ...)`, restart command, and `memoryd status --socket ...` validation.

**Verification plan:**
- `bash -n scripts/install-memorum.sh`
- `scripts/install-memorum.sh --dry-run --repo /tmp/memorum-dry --runtime /tmp/memorum-dry/.memoryd --socket /tmp/memoryd-dry.sock`
- Required real installer smoke: install into `mktemp -d` repo/runtime/socket; let the installer shell exit; from a fresh command verify `kill -0 "$(cat "$runtime/memoryd.pid")"` and `memoryd status --socket "$socket"`; kill the PID in cleanup.

## Task 2: Live harness smoke truthfulness (B2)

**Parallel:** yes
**Blocked by:** none
**Owned files:** `crates/memorum-eval/tests/live.rs`, `crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs`, `docs/runbooks/eval-real-harness-ci.md`
**Invariants:** A live smoke only reports pass after a real harness path ran and at least one assertion count is observed; missing auth/CLI is explicitly reported as `MEMORUM_EVAL_SKIP:*` by the outer live test; nested skip markers can never be accepted as pass.
**Out of scope:** paid live execution without keys; no new high-cost catalog tests.

**Files:**
- Modify: `crates/memorum-eval/tests/live.rs`
- Modify: `crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs`
- Modify: `docs/runbooks/eval-real-harness-ci.md`

**Step 1: Re-map outer live smokes to plan intent.**
- `live::claude_smoke` runs T13 cross-harness substrate sharing and requires both Claude and Codex eval keys plus both CLIs.
- `live::codex_smoke` runs a Codex T15 privacy refusal+retry variant and requires only Codex eval key plus Codex CLI.

**Step 2: Make T15 harness-parametric.**
- Extract shared `run_privacy_filter_refusal_and_retry(harness, eval_key_env, provider_key_env, config_label)` helper that names output/error phases by harness.
- Keep current Claude T15 test for catalog continuity.
- Add `t15_privacy_filter_refusal_and_retry_codex` using `RealHarness::Codex`, `MEMORUM_EVAL_CODEX_KEY`, Codex MCP config, `OPENAI_API_KEY` alias, `CODEX_HOME`, `HOME`, and `PATH`.

**Step 3: Preflight in `live.rs`.**
- Import `HarnessRunner`/`RealHarness`.
- Check env vars and CLI compatibility before spawning nested cargo.
- Print `MEMORUM_EVAL_SKIP:SKIP_NO_AUTH:<vars>` or `MEMORUM_EVAL_SKIP:SKIP_MISSING_CLI:<harness>` and return from the test for honest skips.
- Nested `run_domain_filter` must only be called when outer requirements are satisfied.
- After nested cargo returns success, inspect combined stdout/stderr and fail the outer smoke if it contains `SKIP_NO_AUTH`, `SKIP_MISSING_CLI`, or `MEMORUM_EVAL_SKIP`. Also require a `MEMORUM_EVAL_ASSERTIONS=<n>` marker with `n > 0` for a live pass.

**Step 4: Update eval runbook.**
- Document exact requirements: Claude smoke = both keys and both CLIs because it exercises Codex write plus Claude recall; Codex smoke = Codex key/CLI for T15 Codex privacy retry.

**Verification plan:**
- `env -u MEMORUM_EVAL_CLAUDE_KEY -u MEMORUM_EVAL_CODEX_KEY cargo test -p memorum-eval --features live-harness -- live:: --nocapture`
- `MEMORUM_EVAL_CODEX_KEY=dummy env -u MEMORUM_EVAL_CLAUDE_KEY cargo test -p memorum-eval --features live-harness --test live -- live::claude_smoke --nocapture` must skip/fail honestly because T13 needs both keys.
- `MEMORUM_EVAL_CODEX_KEY=dummy env -u MEMORUM_EVAL_CLAUDE_KEY cargo test -p memorum-eval --features live-harness --test live -- live::codex_smoke --nocapture` must not green-pass without assertions.
- `cargo test -p memorum-eval --test domain t15_privacy_filter_refusal_and_retry -- --nocapture`

## Task 3: Recall panel honest surface (B3)

**Parallel:** yes
**Blocked by:** none
**Owned files:** `crates/memoryd-tui/src/panels/recall.rs`, `crates/memoryd-tui/tests/recall_panel.rs`, `docs/dev/stream-g-tui-recall-panel-design.md`
**Invariants:** Do not extend Stream A protocol in this fix; the panel renders only fields present in `RecallHitSummary`.
**Out of scope:** score/harness/session protocol extension; this is tracked but not implemented.

**Files:**
- Modify: `crates/memoryd-tui/src/panels/recall.rs`
- Modify: `crates/memoryd-tui/tests/recall_panel.rs`
- Modify: `docs/dev/stream-g-tui-recall-panel-design.md`

**Step 1: Remove placeholder columns from UI.**
- Delete the `score:n/a ...` preamble.
- Row format becomes `recalled_at | mem_id | device | seq`, with summary on the next indented line.
- Add one concise tracking comment near the module top pointing to the design doc for score/harness/session protocol extension.

**Step 2: Update tests.**
- Assert the real fields and summary render.
- Assert the frame does not contain `score:n/a`, `harness:n/a`, or `session:n/a`.

**Step 3: Update design doc.**
- State this chooses Claude review option B for this fix: v1 shipped fields from current protocol only.
- Add a named follow-up tracking section for protocol extension to add score/harness/session later; Claude review option A requires explicit Stream A authorization.

**Verification plan:**
- `cargo test -p memoryd-tui recall_panel -- --nocapture`

## Task 4: Doctor health semantics (R5)

**Parallel:** yes
**Blocked by:** none for code; runbook edit is coordinator-owned after Task 1
**Owned files:** `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/main.rs`
**Coordinator-owned docs:** `docs/runbooks/dogfooding-day-one.md`
**Invariants:** Harness failures remain diagnostic findings; missing one harness is not fatal when another enabled harness authenticates.
**Out of scope:** changing harness auth probing implementation.

**Files:**
- Modify: `crates/memoryd/src/handlers.rs`
- Modify: `docs/runbooks/dogfooding-day-one.md`

**Step 1: Add explicit health calculation.**
- While iterating `HarnessCliRegistry::builtin_v0_2().adapters()`, count enabled adapters and `AuthProbeResult::Ok` probes.
- Set `healthy = !has_substrate_findings && (enabled_adapter_count == 0 || authenticated_adapter_count > 0)`.
- Preserve all warning findings for failed probes.

**Step 2: Unit-test the health rule.**
- Extract a small seam/helper used by `doctor_response` so tests exercise the same enabled/authenticated counting and final healthy calculation. Cover: no substrate findings + one authenticated of two => healthy; no substrate findings + zero authenticated of two => unhealthy; substrate findings => unhealthy; empty registry => healthy if substrate clean.
- Update `memoryd doctor` CLI handling so a successful response envelope with `DoctorResponse.healthy == false` prints the response and exits 1.

**Step 3: Update dogfood runbook.**
- Troubleshooting/doctor section documents green/yellow/red semantics and the no-harness failure case.

**Verification plan:**
- `cargo test -p memoryd doctor_health -- --nocapture` or the exact helper test names.

## Task 5: Startup peer-update dedupe regression (R4)

**Parallel:** yes
**Blocked by:** none
**Owned files:** `crates/memoryd/src/recall/startup.rs`, `crates/memoryd/tests/startup_recall_mcp.rs` if needed
**Invariants:** Same-startup same-device peer updates seed cross-device cooldown by memory id/reference before cross-device evaluation.
**Out of scope:** Stream A index schema/protocol changes.

**Files:**
- Modify: `crates/memoryd/src/recall/startup.rs`
- Possibly modify: `crates/memoryd/tests/startup_recall_mcp.rs`

**Step 1: Make the cooldown handoff testable.**
- Extract `surfaced_peer_update_references(insertion: Option<&CoordinationInsertion>) -> HashSet<String>` or equivalent.
- Unit-test that it extracts `PeerUpdateEntry.reference`, not event id/device/summary.
- Keep the production startup path using that helper before calling `cross_device_updates`.

**Step 2: Add a memoryd startup-layer regression guard.**
- Add either an in-module `startup.rs` test around the production extraction/handoff into `cross_device_updates`, or a `crates/memoryd/tests/recall_startup_dedupe.rs`/`startup_recall_mcp.rs` test proving rendered startup output has no duplicate peer-update `<ref>` across same-device/cross-device sections.
- The test must fail if `same_device_surfaced` extraction or cross-device pre-seeding is removed from `startup.rs`, or if the key shape changes away from `PeerUpdateEntry.reference`. Existing `memorum-coordination` tests are supporting evidence only.

**Verification plan:**
- `cargo test -p memoryd surfaced_peer_update_references -- --nocapture`
- `cargo test -p memorum-coordination cross_device_pass_suppresses_ids_already_surfaced_in_same_device_pass -- --nocapture`
- `cargo test -p memoryd startup_recall_mcp -- --nocapture`

## Task 6A: Real mid-render PTY panic restore (N9)

**Parallel:** yes
**Blocked by:** Task 3 if both touch TUI tests in same worker; otherwise none
**Owned files:** `crates/memoryd-tui/src/main.rs`, `crates/memoryd-tui/src/app.rs`, `crates/memoryd-tui/tests/panic_restore.rs`, `crates/memoryd-tui/Cargo.toml`, `Cargo.lock`
**Invariants:** The panic hook must restore raw mode and leave alternate screen after the TUI has actually entered those modes.
**Out of scope:** Cosmetic TUI changes.

**Steps:**
- Add a hidden debug/test-only `--inject-panic-mid-render` flag.
- Thread a test-only panic mode into `app::run` so it enters raw mode/alternate screen, renders at least one frame, then panics from inside the run/render path.
- Add `portable-pty` as a dev dependency and use it to spawn the binary in a PTY.
- Assert the process exits nonzero, stderr includes the injected panic, and the PTY terminal is usable/restored after exit.

**Verification plan:**
- `cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1`
- `cargo check -p memoryd-tui --tests --locked`
- `cargo fmt --all -- --check`

## Task 6B: Compile-time EchoCli gating (N10)

**Parallel:** yes
**Blocked by:** none
**Owned files:** `crates/memoryd/src/dream/harness.rs`, `crates/memoryd/src/dream/orchestration.rs`, `crates/memoryd/src/dream/run.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/cli.rs` if help/docs change, `crates/memoryd/tests/dream_harness_cli.rs`, `crates/memoryd/tests/dream_cli.rs`, `crates/memoryd/Cargo.toml` if feature wiring changes, `Cargo.lock` if deps change
**Invariants:** Release/no-feature builds do not compile EchoCli-specific code and still reject `--cli echo`; dev-fixtures builds keep deterministic echo tests.
**Out of scope:** Changing production harness selection semantics for Claude/Codex.

**Steps:**
- Gate `EchoCli`, deterministic echo harness construction, imports, and every special `name == "echo"` selection path behind `#[cfg(any(test, feature = "dev-fixtures"))]` or equivalent compile-time guards.
- Do not rely on `cfg(test)` for integration-test binaries; echo CLI integration tests must run under `--features dev-fixtures` or be feature-gated.
- Add/adjust tests proving no-feature release/check builds compile and dev-fixtures echo tests still pass.

**Verification plan:**
- `cargo check -p memoryd --no-default-features --locked`
- `cargo build -p memoryd --release --no-default-features --locked`
- `cargo test -p memoryd --features dev-fixtures dream_harness_cli dream_cli -- --nocapture`

## Task 6C: Spec convention and harness allowlist hygiene (N11, N12)

**Parallel:** no
**Blocked by:** none
**Owned files:** `CLAUDE.md`, `docs/specs/system-v0.2.md`, `crates/memorum-coordination/src/session.rs`, `crates/memorum-coordination/tests/session_derivation.rs`
**Invariants:** Unknown harness names remain observe-only by design; no spec version bump without Trey.
**Out of scope:** Changing coordination behavior for known harnesses.

**Steps:**
- Update `CLAUDE.md` and `docs/specs/system-v0.2.md` with the additive-amendment vs behavior-changing version-bump rule. Also update stale Stream F live spec references to v0.3 if present.
- Add `FULL_COORDINATION_HARNESSES` constant and invariant comment in `session.rs`; unknown names default observe-only to prevent silent privilege escalation.
- Update session derivation tests to cover known full-coordination names and unknown observe-only names.

**Verification plan:**
- `cargo test -p memorum-coordination session_derivation -- --nocapture`
- `cargo check -p memorum-coordination --tests --locked`

## Task 7: Review reports and canonical gate (R6, R7)

**Parallel:** no
**Blocked by:** Tasks 1-6C implemented and targeted gates green
**Owned files:** `docs/reviews/2026-05-04-final-gate-report.md`, new `docs/reviews/2026-05-04-dogfood-readiness-*.md`
**Invariants:** Review reports must distinguish read-only findings from applied fixes; canonical gate is run once after integration, not by each subagent.
**Out of scope:** broad unrelated refactors from cleanup agents unless they identify a correctness/security blocker in touched surfaces.

**Files:**
- Modify: `docs/reviews/2026-05-04-final-gate-report.md`
- Create: `docs/reviews/2026-05-04-dogfood-readiness-desloppify-review.md`
- Create: `docs/reviews/2026-05-04-dogfood-readiness-clean-code-review.md`
- Create: `docs/reviews/2026-05-04-dogfood-readiness-security-review.md`

**Step 1: Subagent reviews.**
- Run independent subagents with mandatory skills `clean-code`, `tdd`, `rust-engineer`:
  - cleanup/desloppify review on touched crates/scripts/docs surfaces;
  - clean-code review on installer, recall panel, doctor, live eval;
  - adversarial security review on installer path handling, doctor output, harness auth/probe/env behavior, EchoCli gate.
- Write concise reports with verdict, findings, and evidence.

**Step 2: Fix any blocker findings from reviews.**
- Integrate only scoped, evidence-backed findings.
- Rerun targeted gates for modified files.

**Step 3: Canonical gate.**
- Run `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` once on integrated trunk.
- Append a “post-review-fix canonical gate” stanza to `docs/reviews/2026-05-04-final-gate-report.md` with command, timestamp/date, and outcome. If it fails, fix root cause and rerun.

**Verification plan:**
- Targeted commands from Tasks 1-6C, including PTY and dev-fixtures/no-feature checks.
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh`

## Owned Files Parallelization Check

Task 1, Task 2, Task 3, Task 4, Task 5, Task 6A, and Task 6B are parallel-safe only when overlapping files are not assigned to separate writers. `docs/runbooks/dogfooding-day-one.md` is coordinator-owned and edited once after Task 1 and Task 4 code changes. `Cargo.lock` is coordinator-owned if both Task 6A and Task 6B need dependency/feature changes. Task 6C and Task 7 are coordinator-owned sequential tasks.

## Completion Audit Requirements

Before declaring the goal complete:

1. Map every B/R/N finding above to changed files or an explicit accepted documentation/tracking decision.
2. Confirm no UI/test contains `score:n/a`, `harness:n/a`, or `session:n/a` except in the review doc being fixed.
3. Confirm installer uses runtime log/PID and version skip, and dry-run remains safe.
4. Confirm live smokes cannot produce a nested skip that appears as exercised behavior.
5. Confirm doctor health calculation covers clean/no-harness and clean/some-harness states.
6. Confirm review reports exist and canonical gate outcome is recorded.
7. Check `git status --short` and report all touched/untracked files.
