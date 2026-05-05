# Claude review — `3c6b7ec Implement dogfood readiness streams`

**Reviewer:** Claude (sonnet, fresh-context read against the integrated trunk)
**Subject:** commit `3c6b7ec` on `main` (57 files, ~3,127 LOC) implementing the Streams F/G/H/I dogfood-readiness plan at `docs/plans/2026-05-04-streams-fghi-dogfood-readiness.md`.
**Verdict:** **revisions required before "dogfood-ready" stands.** Three blockers will surface within hours of first use; four risks erode the "shipped honestly" claim; five nits worth folding in.

The hardening surface is genuinely good — `AuthProbeResult` taxonomy, `RepoPath::try_new` propagation in cleanup, `CleanupGit` trait routing, `deferred` catalog field plumbing, launchd plist + installer, baseline-discipline check, specgate cleanup, and the `is_tier1`/`is_tier3` → `is_full_coordination_harness`/`is_observe_only_harness` rename are all correctly executed and behavior-preserving. But there's a cluster of issues that contradict the plan's acceptance criteria or fossilize known gaps as contracts.

This document is structured for Codex execution. Each finding has: location with line numbers, what's wrong, why it matters, the fix (specific enough to act on without re-deriving the design), and acceptance criteria. Order the work by the "Recommended fix order" section at the bottom.

---

## Blockers (will fail day-one dogfooding)

### B1. `install-memorum.sh` leaks the daemon process

**Location:** `scripts/install-memorum.sh:75-94`

**What's wrong.** The installer launches the daemon as a backgrounded child of the install shell:

```bash
memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" >/tmp/memoryd-install.log 2>&1 &
daemon_pid=$!
```

It then waits for socket readiness and prints the MCP snippet — but never `disown`s the child, never writes a PID file, never prints a "daemon PID is X; stop with `kill X`" line, and never uses `nohup` / `setsid`. When the user closes the terminal that ran `bash scripts/install-memorum.sh`, the controlling-terminal SIGHUP propagates to the child and the daemon dies. A dogfooder's first session ends with: "I installed it, opened a new terminal, and now `memoryd status` says socket unreachable."

**Why it matters.** Plan acceptance criterion (Phase 6.1, plan line 516): _"Trey runs it on his machine and gets a working daemon end-to-end with no follow-up configuration."_ Current shape requires the user to either keep the install terminal open forever or know to use `--with-scheduler` (which doesn't exist as a default). The day-one runbook (`docs/runbooks/dogfooding-day-one.md`) directs users to run the installer and assume the daemon stays up — it won't.

**Fix.**

1. Detach the daemon from the controlling terminal. Replace the foreground-and-`&` invocation with one of:
   - `setsid memoryd serve … </dev/null >>"$runtime/memoryd.log" 2>&1 &` (Linux/macOS portable; survives shell exit)
   - `nohup memoryd serve … </dev/null >>"$runtime/memoryd.log" 2>&1 & disown $!`
2. Write the PID to `"$runtime/memoryd.pid"` after a successful readiness probe.
3. Print, after the MCP snippet, an explicit lifecycle stanza:
   ```
   memoryd is running (PID: 12345, log: ~/memorum/.memoryd/memoryd.log).
   To stop:    kill $(cat ~/memorum/.memoryd/memoryd.pid)
   To restart: bash scripts/install-memorum.sh --repo … --runtime … --socket …
   To install as a launchd agent (auto-restart on login): rerun with --with-scheduler.
   ```
4. The install log should land under `"$runtime/memoryd.log"`, not `/tmp/memoryd-install.log`. `/tmp` is purged by macOS periodically and a user diagnosing "why did the daemon stop" needs the log to persist.
5. Update `docs/runbooks/dogfooding-day-one.md` to reference the PID file path and the printed lifecycle commands.

**Acceptance criteria.**

- After running the installer, closing the install terminal, and opening a fresh terminal: `memoryd status --socket /tmp/memoryd.sock` (or whichever socket was passed) returns OK.
- `cat "$runtime/memoryd.pid"` returns a live PID (`kill -0 $(cat …)` succeeds).
- The "To stop / to restart / to install as scheduler" lifecycle stanza prints at the end of installer stdout.
- The daemon log is at `"$runtime/memoryd.log"` (not `/tmp`).

---

### B2. `live::codex_smoke` reports a green pass when only `MEMORUM_EVAL_CODEX_KEY` is set

**Location:** `crates/memorum-eval/tests/live.rs:14-21`, calling into `crates/memorum-eval/tests/eval/domain/t13_cross_harness_substrate_sharing.rs:21-24`.

**What's wrong.** `live::codex_smoke` gates on the presence of `MEMORUM_EVAL_CODEX_KEY` only, then runs T13 (`t13_cross_harness_substrate_sharing`) as a subprocess. T13 itself requires _both_ `MEMORUM_EVAL_CLAUDE_KEY` and `MEMORUM_EVAL_CODEX_KEY` (`missing_auth_keys()` checks both env vars at line 78). When only CODEX_KEY is set:

1. `codex_smoke`'s outer gate passes (CODEX_KEY present).
2. Subprocess runs T13.
3. T13's inner SKIP guard fires (CLAUDE_KEY absent), prints `SKIP_NO_AUTH:…`, returns 0.
4. `run_domain_filter`'s `assert!(output.status.success())` passes.
5. The user sees "codex_smoke: ok" — but no codex behavior was exercised. **Lying-green.**

There's also a plan/impl mapping mismatch: the plan (Phase 5.4, plan line 469) said _"T13 substrate sharing for `claude-smoke`, T15 privacy refusal+retry for `codex-smoke`."_ The implementation has these swapped: claude_smoke runs T15, codex_smoke runs T13.

**Why it matters.** Stream H's whole job is to _not_ lie about coverage. A live-harness smoke that reports success without exercising the live harness directly violates Stream H's stated invariant. Trey's first time setting one key (e.g., to test the codex path before adding the claude key) will produce a false positive.

**Fix.** Pick one. Strong recommendation is option (a):

- **(a) Re-map per the plan and gate per-test on the keys that test actually needs.** `live::claude_smoke` runs T13 (cross-harness; needs both keys; gate on both). `live::codex_smoke` runs T15 (single-harness privacy refusal; needs only one key, currently `CLAUDE_KEY` per `t15_privacy_filter_refusal_retry.rs:15` — verify if the codex path through T15 is actually wired and adjust the env-var name or gate accordingly).
- **(b) Add a "I require both keys" gate to `live::codex_smoke`** so it skips honestly when CLAUDE_KEY is missing rather than producing a green run. Document in the runbook that codex_smoke needs both keys (which is a poor UX but at least honest).
- **(c) Make T13's SKIP path return non-zero** (e.g., exit code 77, the conventional "skipped" code), and have `run_domain_filter` distinguish skip from pass. Riskier — touches T13 directly.

Whichever path is chosen, also update `docs/runbooks/eval-real-harness-ci.md:24-29` so the local-smoke section accurately documents which env vars are required for which test.

**Acceptance criteria.**

- Setting only `MEMORUM_EVAL_CODEX_KEY` and running `cargo test -p memorum-eval --features live-harness -- live::codex_smoke` either skips honestly with a clear `SKIP:` marker on stderr OR exits non-zero. It does not exit 0 with a "passed" appearance.
- Setting only `MEMORUM_EVAL_CLAUDE_KEY` and running `live::claude_smoke` likewise (skip-or-fail unless the new mapping makes claude_smoke exercise something that only needs CLAUDE_KEY).
- Setting both keys runs both smokes through to real harness invocation.
- `docs/runbooks/eval-real-harness-ci.md` documents the per-smoke env var requirements truthfully.

---

### B3. TUI Recall panel pins `score:n/a / harness:n/a / session:n/a` as the test contract

**Locations:**

- `crates/memoryd-tui/src/panels/recall.rs:61` (preamble line) and `:67` (per-row line).
- `crates/memoryd-tui/tests/recall_panel.rs:18-20` (assertions pinning the literal placeholders).
- `crates/memoryd/src/protocol.rs:276` (`RecallHitSummary` shape — fields available).
- `crates/memoryd/src/recall_hits.rs:43-50` (the SQL projection — what the events_log mirror exposes today).

**What's wrong.** Plan decision **D8** specified the panel should render _"`mem_id`, `score`, `harness_source_id`, `surfaced_in_session`."_ The daemon's `RecallHitSummary` only carries `event_id`, `device`, `seq`, `memory_id`, `recalled_at`, `summary` — three of the four headline columns are not in the protocol. Codex's design doc (`docs/dev/stream-g-tui-recall-panel-design.md:27-29`) acknowledged this limitation and chose to render literal `score:n/a`, `harness:n/a`, `session:n/a` strings instead. The implementation does that — _and the test asserts those exact strings render_. That fossilizes the protocol gap as a tested contract: the next person to wire real fields will break the test, and the panel itself shows three "n/a" columns to the user on day one.

The acknowledged limitation is fair; the placeholder strings are not. The product surface implies "we render four data dimensions" while really rendering one (mem_id) plus four pieces of provenance (recalled_at, device, seq, summary).

**Why it matters.** The plan's "why" section for Phase 1 (line 147) says _"the TUI is the headline observability surface."_ A headline surface that displays three "n/a" columns on every row is a worse UX than a panel that doesn't claim to show those columns at all. Worse, the test makes the placeholder a contract — there's no clear forcing function to fix it.

**Fix.** Pick one. Strong recommendation is (a) for a real fix; (b) is the smallest acceptable change.

- **(a) Extend the protocol to carry score, harness_source_id, surfaced_in_session.** Add the columns to `events_log.recall_hit` rows in Stream A (verify Trey's authorization first — Stream A modifications need explicit sign-off; CLAUDE.md "What NOT to do" rule). If those fields are already in the events_log payload JSON, project them out in `recall_hits.rs` rather than the bare summary. Remove the `n/a` strings. Update the test to assert real values from a fixture.
- **(b) Drop the unsupported columns from the panel and the test.** Render only `recalled_at | mem_id | device | seq | summary` — what the protocol actually carries. Drop the misleading preamble line at `recall.rs:61`. Update the test (`recall_panel.rs:18-20`) to assert what the panel really shows; remove the three `assert!(frame.contains("score:n/a"))` lines. Add a single `// TODO(stream-g-vN): score/harness/session require protocol extension; tracking issue …` near the panel module top, and link it to a follow-up plan or issue. Update `docs/dev/stream-g-tui-recall-panel-design.md:27-29` to reflect what shipped.
- **(c) Hybrid: hide the columns behind a `--show-protocol-gaps` debug flag.** Default render is (b); Trey-only escape hatch shows the n/a placeholders for protocol-design conversations. Probably too clever for the value.

If you pick (b), open a follow-up plan documenting the Stream A authorization needed for (a) so this doesn't get lost.

**Acceptance criteria.**

- The TUI Recall panel does not display `score:n/a`, `harness:n/a`, or `session:n/a` to end users (unless under an explicit debug flag).
- `crates/memoryd-tui/tests/recall_panel.rs` does not contain `assert!(frame.contains("score:n/a"))`-style pins on placeholder text.
- The design doc (`docs/dev/stream-g-tui-recall-panel-design.md`) and the panel implementation agree on what's rendered.
- A tracking artifact (issue, follow-up plan, or explicit note in the design doc) records the protocol-gap → real-fields work item if not done in this commit.

---

## Risks (real, but won't block first-day use)

### R4. Phase 3.4's required regression test was not added

**Location:** `crates/memoryd/src/recall/startup.rs:218-240` (the cool-down sharing logic, inherited from `d9628cb`); no test file added in this commit covering same-device/cross-device peer-write dedup.

**What's wrong.** Plan Phase 3.4 (line 357-358) was explicit: _"Add a regression test: a single memory that scores above threshold in both same-device and cross-device categories should appear **once** in a single startup, not twice."_ The cool-down sharing code was actually shipped in `d9628cb` (the prior commit), so this commit's only Phase 3.4 change was the `is_tier3` → `is_observe_only_harness` rename — but that doesn't satisfy the plan's test requirement. `git grep` for `surfaced_peer_write` in `crates/memoryd/tests/` returns nothing.

**Why it matters.** The cool-down logic at `startup.rs:218-240` works by extracting `peer_updates[].reference` (a stringified `MemoryId`) from the same-device pass and seeding it into the cross-device pass's session clone via `record_surfaced_peer_write`. If a future refactor changes either side's key shape (e.g., switches to `event_id` or normalizes case), dedup silently breaks. The test is the regression guard.

**Fix.** Add a test (suggested location: `crates/memoryd/tests/recall_startup_dedupe.rs`):

1. Build a `Substrate` test fixture with a single peer-write memory that:
   - has the same `source_device` as the local device (so it lands in `same_device_rows`)
   - and a paired cross-device peer-write that _would_ score above threshold for the same session context.
2. Run `startup_peer_updates` (or whatever the public entrypoint is from `recall::startup`) with a session at coordination level 2+ and a full-coordination harness.
3. Assert the resulting `same_device.peer_updates` plus `cross_device.peer_updates` together contain each `memory_id` at most once.
4. Bonus assertion: the same memory appearing in `same_device` results in it being absent from `cross_device`, not the other way around (verifies pass ordering).

If the test scaffolding for this is heavy, ask Trey before diverging — but the plan flagged this as required, so the bar is "the test exists," not "the test is elegant."

**Acceptance criteria.**

- A test exists asserting per-startup peer-write dedup across the same-device → cross-device handoff.
- The test fails if the `same_device_surfaced` extraction at `startup.rs:227-230` is removed or its key shape changes.
- `cargo test -p memoryd recall::startup` (or the crate-level test invocation) runs and passes.

---

### R5. Doctor health is "substrate-only" — looser than the plan reads

**Location:** `crates/memoryd/src/handlers.rs:1417-1437`.

**What's wrong.** The doctor handler now has:

```rust
let has_substrate_findings = !findings.is_empty();
let registry = crate::dream::registry::HarnessCliRegistry::builtin_v0_2();
for (name, adapter) in registry.adapters() {
    let probe = adapter.auth_probe().await;
    if !probe.is_ok() {
        findings.push(DoctorFinding { /* harness_cli_warning */ });
    }
}
DoctorResponse {
    healthy: !has_substrate_findings,
    findings,
    …
}
```

The `healthy` flag is set from `has_substrate_findings` (snapshotted _before_ harness probes append findings), so harness misses are _never_ fatal regardless of count. Plan Phase 6.2 (line 523) said: _"Exit code 0 if at least one harness is available; exit code 1 only when **all** doctor checks fail (preserve existing semantics — missing harness alone is a warning, not a failure)."_

The current implementation matches the second clause ("missing harness alone is a warning") but not the first ("at least one available" implies "zero available is a failure"). If both `claude` and `codex` are missing, doctor reports `healthy: true` with two harness warnings — which contradicts the plan.

**Why it matters.** A user with no harness installed should know that dreams are completely disabled, not get a "healthy: true" green light. The current shape silently downgrades a "no harness available at all" state to a warning.

**Fix.** Change the health logic to: `healthy: !has_substrate_findings && (at_least_one_harness_authenticated || zero_enabled_adapters_in_registry)`. Concretely, count harness probes that returned `AuthProbeResult::Ok` while iterating, and require either `>0` Ok results OR an empty registry (the trivial case). Update the runbook (`docs/runbooks/dogfooding-day-one.md`) to document the rule clearly.

Keep the per-finding warning emission (those are useful diagnostic output even when healthy: true).

**Acceptance criteria.**

- With substrate-clean state and at least one authenticated harness: `doctor.healthy == true`.
- With substrate-clean state and zero authenticated harnesses: `doctor.healthy == false`.
- With substrate-dirty state: `doctor.healthy == false` regardless of harness state.
- The runbook documents this rule.

---

### R6. Full canonical gate `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` was not rerun green

**Source:** `docs/reviews/2026-05-04-final-gate-report.md` lines 50-54, which acknowledges the gate ran once, failed on `cli_contract::test_clap_rejects_panel_out_of_range`, the assertion was patched narrowly, and "per Trey instruction, the full canonical gate was not rerun after the narrow fix."

**What's wrong.** The targeted gates listed in the report are all green, but they don't substitute for the workspace-wide invariant. We don't actually know whether the integrated trunk passes the full release gate end-to-end.

**Why it matters.** The plan's Phase 8.2 (line 604) makes the full gate the merge bar. Skipping the rerun leaves the merge story incomplete. If a future commit causes a regression that only surfaces under `--workspace --locked` (e.g., a feature-flag interaction, a doctest, a specgate violation, or a bench check that didn't get isolated), it'll get blamed on that future commit even though the regression entered with this one.

**Fix.** Run `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` once on integrated trunk. If it passes, append a "post-merge gate verification" stanza to `docs/reviews/2026-05-04-final-gate-report.md` recording the run. If it fails, the failure is the new top-priority item — fix it at root and rerun.

**Acceptance criteria.**

- One full canonical gate run is recorded in the final gate report with a green outcome.
- If anything fails, the report is updated with the failure and the corrective fix.

---

### R7. Phase 7 cross-cutting subagent passes were not run

**Source:** `docs/reviews/2026-05-04-final-gate-report.md:56`: _"the planned multi-reviewer/subagent swarm was not used; this report is a local Codex integration audit."_

**What's wrong.** The plan called for `desloppify-deep` 8-axis sweep, `clean-code` on new surfaces, and `security_auditor` on Stream F + auth probe + install scripts. None of those ran. The `2026-05-04-final-security-audit.md` exists but is a single-pass review by the same Codex that wrote the code — not adversarial.

**Why it matters.** Less critical for solo dogfooding; more critical if these streams are claimed as "shipped." The audit-by-author surface area is exactly where dedup, weak types, defensive code, and dead branches accumulate without external eyes.

**Fix.** This is a follow-up rather than a blocker. Run, in this order, with read-only scoping where possible:

1. `desloppify-deep` against the seven crates Phase 7.1 listed (`crates/memoryd/`, `memoryd-tui/`, `memoryd-web/`, `memorum-coordination/`, `memorum-eval/`, `memory-governance/`, plus any others touched). Brief each axis subagent: _"do not run scripts/check.sh; do not run cargo test --workspace; run only cargo check -p <pkg>"_ per CLAUDE.md CPU discipline.
2. `clean-code` pass on the new TUI Recall panel + the auth probe surface + the install scripts.
3. Adversarial `security_auditor` pass on the EncryptAtRest refusal flow, auth probe diagnostics (especially the daemon-PATH disclosure path), install scripts (path injection vectors), and doctor output (sensitive path/env disclosure).

Coordinator runs `scripts/check.sh` once after all axes merge. Findings land in a follow-up review doc, not amended into this commit.

**Acceptance criteria.**

- Three subagent reports under `docs/reviews/2026-05-XX-*.md` with verdict and findings.
- Coordinator-level `bash scripts/check.sh` passes after all axis merges.

---

## Nits (worth folding in, not gate-blocking)

### N8. `install-memorum.sh` always reinstalls

**Location:** `scripts/install-memorum.sh:68-72`.

**What's wrong.** Plan Phase 6.1 (line 508) said: _"`cargo install --path crates/memoryd` (skip if `memoryd --version` already matches `Cargo.toml`)."_ Current script unconditionally runs the install, costing ~30s per re-run.

**Fix.** Read the version from `crates/memoryd/Cargo.toml` (e.g., `grep '^version' crates/memoryd/Cargo.toml | head -1 | sed -E 's/.*"(.+)".*/\1/'` or use `cargo metadata --format-version 1 | jq -r '.packages[] | select(.name=="memoryd") | .version'`). Compare against `memoryd --version` if `memoryd` is on PATH. Skip the install if they match; print a "memoryd vX.Y.Z already installed; skipping cargo install" line.

**Acceptance criteria.** Running the installer twice in a row, the second invocation prints the skip message and does not invoke `cargo install`.

---

### N9. Panic-restore test doesn't actually exercise restore

**Location:** `crates/memoryd-tui/tests/panic_restore.rs`.

**What's wrong.** The test asserts the binary panics on `--inject-panic` and that the flag is hidden from `--help`. Both pass. But the panic happens at `main.rs:30`, _before_ `app::run()` enters raw mode — so `restore_terminal_blocking()` is invoked but is a no-op. A regression where the panic hook fails to actually call `disable_raw_mode` + `LeaveAlternateScreen` won't be caught.

**Why it matters.** Plan Phase 1.4 (line 206) asked for a pty-based test that asserts terminal state is restored. Current shape is a smoke test for hook wiring + flag visibility — useful, but not a regression guard for the actual restore behavior.

**Fix.** Two acceptable paths:

- **(a) Inject the panic _during_ render, not before.** Add a second hidden flag (`--inject-panic-mid-render`) that enters raw mode, takes one frame, then panics. The test runs the binary in a `portable_pty` (workspace dep, see if it's already in the workspace) and asserts the resulting tty state is non-raw afterwards. More work; better signal.
- **(b) Document the limitation explicitly.** Rename the current test to `panic_hook_is_invoked_and_flag_is_hidden`, add a `// TODO: this does not exercise actual raw-mode restore — see Phase 1.4 plan` comment, and accept the gap.

**Acceptance criteria.** Either the test catches a regression where the panic hook stops calling `restore_terminal_blocking()` (option a), or the test name and comments make its limited scope clear (option b).

---

### N10. `EchoCli` runtime-gate vs the plan's compile-time gate

**Location:** `crates/memoryd/src/dream/orchestration.rs:91,130-134` (runtime gate) and `harness.rs:184-234` (`EchoCli` impl, no `cfg` guard).

**What's wrong.** Plan Phase 2.5 (line 279) said: _"Wrap the variant and `--cli echo` CLI option in `#[cfg(any(test, feature = "dev-fixtures"))]`."_ Codex implemented a runtime gate via `echo_cli_override_enabled()` instead. Functionally correct — release builds without `dev-fixtures` correctly reject `--cli echo` with "unknown harness CLI override" — but the `EchoCli` impl still ships in release binaries, and the plan's literal request was a compile-time gate.

**Why it matters.** Defensible deviation. Release-build users can't reach `EchoCli`, so it's not a security gap. But the binary carries dead code, and a future maintainer reading the plan and the code may mistakenly assume it's compile-time-gated.

**Fix.** Two acceptable paths:

- **(a) Match the plan literally.** Add `#[cfg(any(test, feature = "dev-fixtures"))]` to the `EchoCli` struct, impl, and the `--cli echo` arg in `cli.rs`. Audit all callers; replace runtime checks with the compile-time guard.
- **(b) Document the deviation.** Add a comment near `echo_cli_override_enabled()` explaining why runtime gating was chosen (release safety preserved without conditional compilation), and update the plan/spec accordingly so future readers don't get confused.

If (a), confirm with Trey that compile-time gating is required — the plan said so but the runtime gate is functionally equivalent for security.

**Acceptance criteria.** Either `EchoCli` is genuinely absent from release binaries (option a), or the runtime-gate decision is documented in code + plan revision (option b).

---

### N11. Spec amendment vs version bump asymmetry

**Locations:**

- `docs/specs/stream-f-dreaming-v0.3.md` (new, full version bump for one new candidate-refusal reason).
- `docs/specs/stream-a-core-substrate-v1.1.md` (in-place amendment for two new public APIs, per `docs/reviews/2026-05-04-f003-ratification-audit.md` recommendation to keep v1.1).

**What's wrong.** Stream F got a clean v0.2 → v0.3 bump for what's arguably a smaller change (one refusal reason added to Pass 2 candidate policy). Stream A keeps v1.1 with an in-place amendment despite adding two new public-API methods. The asymmetry is fine in principle but right now it's case-by-case with no rule recorded.

**Why it matters.** Future contributors won't know when to bump vs. amend. The next contract change either gets a too-aggressive bump or a too-permissive amendment, depending on which precedent they pick.

**Fix.** Add a short paragraph to `CLAUDE.md` (or the system spec at `docs/specs/system-v0.2.md`) under "Spec/plan conventions" stating the rule. Suggested rule: _"Additive amendments to public surface (new methods, new variants on a non-exhaustive enum) may stay in-version with a dated amendment block. Behavior-changing amendments (changed return shape, new invariants enforced, removed surface) bump the version."_ Adjust to whatever Trey actually wants.

**Acceptance criteria.** A documented rule exists, and both Stream A's and Stream F's choices are consistent with it.

---

### N12. `is_observe_only_harness()` is closed-world without an invariant comment

**Location:** `crates/memorum-coordination/src/session.rs:104-110`.

**What's wrong.** `is_full_coordination_harness()` matches against the literal set `["codex", "codex-cli", "claude-code"]`. `is_observe_only_harness()` is defined as the negation. A future harness like `claude-code-v2` would silently fall into observe-only mode — which is the safe default, but there's no comment recording that intent or guarding against drift.

**Why it matters.** Not a bug today. But the next harness adapter author needs to remember to update this matchlist, and there's no forcing function. Six months from now, debugging "why did `claude-code-v2` not see peer updates?" will trace back to this matchlist with no comment about its semantics.

**Fix.** Add an invariant comment above `is_full_coordination_harness`:

```rust
// Invariant: the explicit allowlist of harness names that are known to support full
// coordination tier (peer-update insertion, claim locks). Adding a new full-coordination
// harness *requires* updating this match; unknown names default to observe-only by design,
// preventing silent privilege escalation when a new harness adapter lands.
//
// If you're adding a new harness adapter and it should participate in coordination, also
// add it here and to the StreamI test fixture at <path>.
```

Optionally extract the allowlist into a `const FULL_COORDINATION_HARNESSES: &[&str] = &[…];` so the test fixture can reference the same list and a future drift warning can fire.

**Acceptance criteria.** A clear invariant comment exists; either the allowlist is centralized as a `const` or the comment explicitly names the test fixture(s) that need to stay in sync.

---

## Recommended fix order

For Codex execution, in dependency-respecting order:

1. **B1** (installer daemon leak) — highest dogfood impact; standalone shell-script change.
2. **B2** (codex_smoke lying-green) — standalone test/runbook change; orthogonal to B1.
3. **B3** (TUI Recall placeholder pin) — option (b) is fastest and unblocks honest UX; option (a) requires Stream A authorization first.
4. **R5** (doctor health logic) — small handler change, ties into B1's runbook updates.
5. **R4** (cool-down regression test) — additive test; can land in parallel with anything.
6. **R6** (full gate rerun) — runs after the above land. If anything fails, that's the new top item.
7. **N8, N10, N12** — small isolated changes, batch as one cleanup commit.
8. **N9, N11** — documentation/test-shape decisions; pair with Trey on the choice between options.
9. **R7** (Phase 7 subagent swarm) — schedule as a separate follow-up plan; not blocking.

The first three are the must-fix-before-claiming-dogfood-ready set. R5 and R4 are must-fix-before-claiming-shipped. R6 is must-fix-before-the-next-merge-on-this-trunk. Everything else is hygiene.

---

## What's good (so this isn't all critique)

- **AuthProbeResult taxonomy** (`harness.rs:60-88`) is clean: five named variants covering all real failure modes, with operator-readable formatting via `operator_message()`. The path disclosure is correctly limited to local diagnostic surfaces per the security audit.
- **`RepoPath::try_new` propagation** in `cleanup.rs:670-672` correctly threads the error through the finding pipeline rather than panicking; the per-call-site error handling at `:150-160`, `:202-213`, `:284-295` is consistent.
- **`CleanupGit` trait routing** at `cleanup.rs:557-604` cleanly captures all `Command::new("git")` invocations inside `RealCleanupGit`. No bypass remains.
- **`is_tier1`/`is_tier3` rename** is genuinely behavior-preserving (verified by walking the prior `4e87c70` source: `is_tier1` matched codex/claude-code → `is_full_coordination_harness` matches the same set; truth values consistent at all callsites).
- **`deferred` field plumbing** in `orchestrator.rs` is uniformly applied across all 19 catalog entries; the format string at `:381` correctly surfaces the new field.
- **Specgate cleanup** removed obsolete Stream A Rust ownership stubs and added the one JS spec the installed Specgate ownership doctor actually discovers — that's the right fix for F-013, not a paper-over.
- **The launchd plist + install-launchd.sh** are simple, idempotent, and correctly use sed-template interpolation. The dry-run path is honest.
- **F-003 ratification audit** is thorough and reaches a defensible verdict on the v1.1-vs-v1.2 question.

The dogfood-readiness goal is closer than the issues above suggest; the blockers are localized to three surfaces (installer, smoke test, recall panel) and the risks are localized to one handler + one test gap + two process gaps. Fix the three blockers and this is genuinely ready for first-week dogfooding.
