# Dogfood test/gate review

Date: 2026-05-05
Reviewer lane: `test_hardener`
Mandatory skills applied: `clean-code`, `tdd`, `rust-engineer`
Scope: dogfood gate adequacy and speed only for `scripts/check-fast.sh`, `scripts/check-dogfood.sh`, targeted tests, skipped live eval behavior, TUI panic restore test, doctor health tests, and installer smoke coverage.

## Verdict

No dogfood blockers found.

The dogfood gate split is adequate for local dogfooding. On this warm-cache review run, `check-fast` completed in 27s and `check-dogfood` completed in 72s. Both are well under the plan budgets of under 10 minutes for fast loops and under 20 minutes for the dogfood confidence gate. I did not run `scripts/check.sh`.

## Findings

### Dogfood risk: `memoryd doctor` nonzero CLI exit is implemented but not directly covered by the dogfood gate

- Classification: dogfood risk
- Files/lines:
  - `scripts/check-dogfood.sh:27-28` runs only the private/lib-filtered health predicate test: `cargo test -p memoryd --lib doctor_health -- --nocapture`.
  - `crates/memoryd/src/handlers.rs:1445-1450` contains the `doctor_is_healthy(...)` predicate.
  - `crates/memoryd/src/handlers.rs:3764-3770` covers the four health predicate cases.
  - `crates/memoryd/src/main.rs:57-67` maps the doctor response to a process exit.
  - `crates/memoryd/src/main.rs:867-870` returns exit code `1` for an unhealthy successful doctor response.
- Why it matters: Task 4 asked for direct CLI/integration verification that unhealthy `memoryd doctor` exits nonzero. The code path is simple and readable, and the predicate itself is covered, so this is not a dogfood blocker. But if the CLI response/exit wiring regresses while the predicate still passes, `check-dogfood.sh` would not catch it.
- Suggested narrow follow-up: add or include one targeted CLI/integration test that drives an unhealthy doctor response and asserts process exit `1`, then include that specific filter in `check-dogfood.sh` if it remains fast.

### Release-only: skipped live-eval marker is visible but not asserted by a meta-test

- Classification: release-only
- Files/lines:
  - `scripts/check-dogfood.sh:33-35` unsets provider keys and runs the live-harness wrapper test with `--nocapture`.
  - `crates/memorum-eval/tests/live.rs:11-32` returns early without real provider calls when required env vars are missing.
  - `crates/memorum-eval/tests/live.rs:13-15` and `25-27` print `MEMORUM_EVAL_SKIP:SKIP_NO_AUTH...` before returning.
  - `crates/memorum-eval/tests/live.rs:50-72` rejects nested domain tests that silently skip once keys/CLIs are present.
- Why it matters: The dogfood run visibly prints skip markers and avoids paid live evals, which is correct for local dogfood. A stronger release gate could meta-test that the no-key path always emits an explicit skip marker rather than merely returning `ok`.
- Why not a dogfood blocker: paid live eval CI is explicitly out of dogfood scope, and the current dogfood command surfaces the skip marker in normal output.

## Adequacy notes by surface

### Fast gate

- `scripts/check-fast.sh:31-67` is a bounded loop gate: rustfmt, shell syntax, targeted clippy for touched dogfood crates, live-harness compile, JS format/lint when available, Specgate when available, and baseline discipline.
- It does not run workspace-wide tests, release tests, convergence, durability, or benches.
- Speed observed in this review: `check-fast duration: 27s`; shell `time` reported `27.078 total`.
- The existing closeout gate report also records a final fresh/recompile dogfood run where fast-gate time was 335s and dogfood time was 678s, still inside the plan budget (`docs/reviews/2026-05-05-dogfood-closeout-gate-report.md:14-28`).

### Dogfood gate

- `scripts/check-dogfood.sh:18-38` calls the fast gate and then adds only focused dogfood tests: Recall panel, panic restore, doctor health predicate, peer-update reference extraction, no-key live-harness skip behavior, and minimal-feature `memoryd` compile.
- It avoids the expensive release gate, workspace-wide nextest, durability matrix, convergence, and benches.
- Speed observed in this review: `check-dogfood duration: 72s`; shell `time` reported `1:11.14 total`.

### TUI panic restore test

- `crates/memoryd-tui/tests/panic_restore.rs:16-48` uses a real PTY, enters alternate screen, triggers a mid-render panic, drains PTY output concurrently to avoid deadlock, and asserts the alternate-screen leave sequence appears after the enter sequence.
- This is deterministic enough for dogfood and would catch the important regression: panic after entering alternate screen leaves the terminal stuck.

### TUI Recall panel test

- `crates/memoryd-tui/tests/recall_panel.rs:4-21` asserts the panel renders the dogfood-visible hit information and explicitly does not render placeholder rich fields (`score:n/a`, `harness:n/a`, `session:n/a`).
- `crates/memoryd-tui/tests/recall_panel.rs:23-35` covers the empty state.
- This is adequate for the closeout scope because rich Recall fields are explicitly out of dogfood scope.

### Doctor health tests

- `crates/memoryd/src/handlers.rs:1395-1442` builds doctor findings from substrate repair/mirror state and harness probes.
- `crates/memoryd/src/handlers.rs:1445-1450` keeps health logic narrow: substrate findings are unhealthy; no enabled harness with clean substrate is healthy; at least one authenticated enabled harness with clean substrate is healthy; enabled harnesses with zero authenticated harnesses are unhealthy.
- `crates/memoryd/src/handlers.rs:3764-3770` covers those predicate cases directly.
- Residual risk is limited to CLI exit integration, listed above.

### Skipped live eval behavior

- The no-key dogfood command intentionally unsets `MEMORUM_EVAL_CLAUDE_KEY` and `MEMORUM_EVAL_CODEX_KEY` (`scripts/check-dogfood.sh:33-35`).
- The observed run printed:
  - `MEMORUM_EVAL_SKIP:SKIP_NO_AUTH:MEMORUM_EVAL_CLAUDE_KEY,MEMORUM_EVAL_CODEX_KEY`
  - `MEMORUM_EVAL_SKIP:SKIP_NO_AUTH:MEMORUM_EVAL_CODEX_KEY`
- This is the right dogfood behavior: no paid provider call, explicit skip signal, green local gate.

### Installer smoke coverage

- `scripts/install-memorum.sh:96-105` supports `--force-reinstall` so dogfood smokes do not trust only semver equality.
- `scripts/install-memorum.sh:108-135` handles an existing PID file without creating competing daemons.
- `scripts/install-memorum.sh:141-170` starts `memoryd` detached, waits for `memoryd status`, kills/removes PID on readiness failure, and writes the PID only after readiness.
- `scripts/install-memorum.sh:173-200` prints MCP, PID, log, stop, restart, and scheduler instructions.
- I ran the dry-run, stale-PID dry-run, and real temp install smoke. The real smoke force-reinstalled `memoryd`, started a temp daemon, verified the PID with `kill -0`, verified `memoryd status --socket`, and killed the temp daemon. The smoke passed in 2.112s on this run.
- This coverage is adequate as a dogfood closeout smoke. It does not need to be folded into `check-dogfood.sh` because the plan treats installer smoke as a separate Task 5 verification step, not as the pre-install confidence gate.

## Commands run

```bash
git status --short --branch
sed -n '1,260p' docs/plans/2026-05-05-dogfood-closeout.md
sed -n '260,620p' docs/plans/2026-05-05-dogfood-closeout.md
find scripts crates tests docs -maxdepth 4 -type f | sort | rg 'check-(fast|dogfood)|doctor|install|installer|tui|panic|dogfood|eval|closeout|smoke|cli_contract'
nl -ba scripts/check-fast.sh | sed -n '1,240p'
nl -ba scripts/check-dogfood.sh | sed -n '1,260p'
nl -ba crates/memoryd-tui/tests/panic_restore.rs | sed -n '1,220p'
nl -ba crates/memoryd-tui/tests/recall_panel.rs | sed -n '1,220p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '1390,1455p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '3720,3815p'
nl -ba crates/memoryd/src/main.rs | sed -n '40,72p;860,874p'
nl -ba crates/memorum-eval/tests/live.rs | sed -n '1,150p'
nl -ba scripts/install-memorum.sh | sed -n '1,260p'
ls -l scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh
bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh
time ./scripts/check-fast.sh
time ./scripts/check-dogfood.sh
scripts/install-memorum.sh --dry-run --repo /tmp/memorum-dry --runtime /tmp/memorum-dry/.memoryd --socket /tmp/memoryd-dry.sock
tmp=$(mktemp -d); mkdir -p "$tmp/runtime"; printf '999999\n' > "$tmp/runtime/memoryd.pid"; scripts/install-memorum.sh --dry-run --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$tmp/memoryd.sock"; test -f "$tmp/runtime/memoryd.pid"; echo "stale pid preserved: $(cat "$tmp/runtime/memoryd.pid")"
tmp=$(mktemp -d); sock="$tmp/memoryd.sock"; command -v memoryd || true; memoryd --version || true; time scripts/install-memorum.sh --force-reinstall --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$sock"; command -v memoryd; memoryd --version; pid=$(cat "$tmp/runtime/memoryd.pid"); kill -0 "$pid"; memoryd status --socket "$sock"; kill "$pid"; rm -rf "$tmp"
nl -ba docs/reviews/2026-05-05-dogfood-closeout-gate-report.md | sed -n '1,260p'
```

## Command results

- `bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh`: passed.
- `time ./scripts/check-fast.sh`: passed; `check-fast duration: 27s`; shell total `27.078s`.
- `time ./scripts/check-dogfood.sh`: passed; `check-dogfood duration: 72s`; shell total `1:11.14`.
- Dry-run installer smoke: passed and printed MCP/lifecycle instructions.
- Dry-run stale PID preservation: passed; file remained and still contained `999999`.
- Real temp installer smoke with `--force-reinstall`: passed; `memoryd --version` printed `memoryd 0.1.0`; PID was live; `memoryd status --socket` returned JSON state `ready`; temp daemon was killed.

## Explicit non-blockers / deferred by scope

I did not classify any of these as dogfood blockers: workspace-wide release tests, durability, convergence, benches, T17 lease re-entrancy, T18 key rotation, rich Recall score/harness/session fields, or live paid eval CI. Those remain release/product work, not dogfood gate adequacy blockers.
