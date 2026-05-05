# Dogfood product review

Date: 2026-05-05
Reviewer lane: Task 6 dogfood product review
Scope reviewed: current dirty diff for installer lifecycle, doctor truthfulness, TUI Recall/panic restore, `check-fast`/`check-dogfood`, and day-one runbook. This review judges dogfood readiness only, not full release/product readiness.

## Verdict

No dogfood blockers found.

The dirty diff appears to meet the dogfood bar: Trey can run a bounded dogfood gate, force-install the current worktree binary, keep a detached daemon alive with visible PID/log lifecycle, use the Recall TUI panel without fake protocol fields, and distinguish daemon liveness from doctor health. I found one runbook dogfood risk that should be fixed before handoff polish, but it does not block local dogfooding because the installer, socket defaults, and core commands still provide a usable path.

## Intended outcome

Task 6 is intended to independently review whether the dogfood-only closeout actually lets Trey install and use Memorum locally without relying on the full release gate. The explicit non-goals remain non-blocking for dogfood: T17, T18, rich Recall fields, paid live eval CI, full release gate rerun, and production multi-device hardening.

## Findings

### Dogfood risk - Runbook doctor command can check the wrong repo/runtime

- Classification: dogfood risk
- Evidence:
  - `docs/runbooks/dogfooding-day-one.md:5-8` installs the daemon against `--repo ~/memorum --runtime ~/memorum/.memoryd --socket /tmp/memoryd.sock`.
  - `docs/runbooks/dogfooding-day-one.md:98-99` tells the user that `memoryd doctor` is the health/auth check, but does not show the matching `--repo ~/memorum --runtime ~/memorum/.memoryd` flags.
  - `crates/memoryd/src/cli.rs:176-182` defines doctor `RootArgs` defaults as `--repo .` and `--runtime .memoryd`, unlike the runbook install location.
- Why it matters: A dogfooder following the runbook from the source checkout can run `memoryd doctor` and inspect `./.memoryd` instead of the installed `~/memorum/.memoryd` runtime. That can produce a false red/green relative to the daemon Trey actually installed.
- Reasoning: Most day-one commands are socket-based and safely default to `/tmp/memoryd.sock`; `doctor` is not socket-based. It opens a substrate directly using repo/runtime roots, so the runbook needs to repeat the install roots for doctor specifically.
- Recommendation: In the troubleshooting/health section, change the example wording to `memoryd doctor --repo ~/memorum --runtime ~/memorum/.memoryd`, and optionally add that `memoryd status --socket /tmp/memoryd.sock` is socket liveness while `memoryd doctor --repo ... --runtime ...` checks the installed substrate/harness health.
- Confidence: High

## Non-blocking simplifications

- `scripts/install-memorum.sh:185-200` prints nearly identical lifecycle text for dry-run and real runs. A tiny helper could reduce duplication, but this is not worth blocking dogfood; the current output is clear.
- `crates/memoryd/src/handlers.rs:1421-1438` names the count `enabled_harness_count`, but it currently means built-in active adapters (`claude`, `codex`) rather than a user-config-filtered priority list. That is acceptable for the dogfood bar and matches the current built-in registry, but the naming may need tightening when per-scope CLI priority becomes product-facing health policy.

## Test gaps

- There is no direct automated CLI regression for `memoryd doctor` returning exit code 1 when the response is successful but `healthy == false`. The helper is present at `crates/memoryd/src/main.rs:867-872`, and the health truth table is covered at `crates/memoryd/src/handlers.rs:3765-3769`; I also manually verified a non-zero doctor path with `target/debug/memoryd doctor --repo "$tmp/repo" --runtime "$tmp/runtime"` returning `exit=1`. For dogfood this is acceptable, but an integration test would be a good release hardening follow-up.
- `scripts/check-dogfood.sh:24-25` runs the mid-render panic restore path but not the hidden-flag help assertion in `crates/memoryd-tui/tests/panic_restore.rs:50-58`. That is fine for dogfood because the critical product behavior is terminal restore after entering alternate screen; broader CLI polish can stay release-only.

## Commands run

```bash
sed -n '1,220p' /Users/treygoff/.agents/skill-library/clean-code/SKILL.md
sed -n '1,220p' /Users/treygoff/.agents/skill-library/tdd/SKILL.md
sed -n '1,220p' /Users/treygoff/Code/agent-memory/.codex/skills/rust-engineer/SKILL.md
rg -n "dogfood|closeout|doctor|install|Recall|panic|check-fast|check-dogfood" /Users/treygoff/.ai-profiles/runtime/codex/personal/memories/MEMORY.md
sed -n '347,402p' /Users/treygoff/.ai-profiles/runtime/codex/personal/memories/MEMORY.md
sed -n '1,620p' docs/plans/2026-05-05-dogfood-closeout.md
git status --short
git diff --stat
git diff --name-only
git diff -- scripts/check-fast.sh scripts/check-dogfood.sh scripts/check.sh scripts/install-memorum.sh
git diff -- crates/memoryd/src/handlers.rs crates/memoryd/src/main.rs crates/memoryd/tests/handler_contract.rs
git diff -- crates/memoryd-tui/src/app.rs crates/memoryd-tui/src/main.rs crates/memoryd-tui/src/panels/recall.rs crates/memoryd-tui/tests/panic_restore.rs crates/memoryd-tui/tests/recall_panel.rs crates/memoryd-tui/Cargo.toml
git diff -- docs/runbooks/dogfooding-day-one.md docs/reviews/2026-05-05-dogfood-closeout-gate-report.md
nl -ba scripts/check-fast.sh | sed -n '1,240p'
nl -ba scripts/check-dogfood.sh | sed -n '1,240p'
nl -ba scripts/install-memorum.sh | sed -n '1,260p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '1390,1465p'
nl -ba crates/memoryd/src/main.rs | sed -n '40,85p;855,875p'
nl -ba crates/memoryd-tui/src/app.rs | sed -n '595,645p'
nl -ba crates/memoryd-tui/src/main.rs | sed -n '1,70p'
nl -ba crates/memoryd-tui/src/panels/recall.rs | sed -n '1,115p'
nl -ba crates/memoryd-tui/tests/panic_restore.rs | sed -n '1,110p'
nl -ba docs/runbooks/dogfooding-day-one.md | sed -n '1,130p'
rg -n "struct HarnessCliRegistry|builtin_v0_2|adapters\(|enabled|priority|auth_probe|dream CLI priority" crates/memoryd/src/dream crates/memoryd/src -g '*.rs'
git diff -- crates/memoryd/src/cli.rs crates/memoryd/Cargo.toml crates/memoryd/src/dream/harness.rs crates/memoryd/src/dream/orchestration.rs crates/memoryd/src/dream/run.rs crates/memoryd/src/recall/startup.rs crates/memorum-eval/tests/live.rs
rg -n "doctor_health|Doctor|healthy|doctor_cli_exit|exit" crates/memoryd/tests crates/memoryd/src -g '*.rs'
ls -l scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh scripts/check.sh
nl -ba crates/memoryd/src/dream/registry.rs | sed -n '1,100p'
nl -ba crates/memoryd/src/dream/status.rs | sed -n '35,115p'
nl -ba crates/memoryd/src/dream/config.rs | sed -n '1,70p'
bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh
scripts/install-memorum.sh --dry-run --repo /tmp/memorum-dry-review --runtime /tmp/memorum-dry-review/.memoryd --socket /tmp/memoryd-dry-review.sock
cargo test -p memoryd --lib doctor_health -- --nocapture
cargo test -p memoryd-tui recall_panel -- --nocapture
cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1
tmp=$(mktemp -d); mkdir -p "$tmp/runtime"; printf '999999\n' > "$tmp/runtime/memoryd.pid"; scripts/install-memorum.sh --dry-run --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$tmp/memoryd.sock" >/tmp/agent-memory-review-dry-run-stale-pid.out; test -f "$tmp/runtime/memoryd.pid"; cat /tmp/agent-memory-review-dry-run-stale-pid.out
cargo test -p memoryd --lib surfaced_peer_update_references -- --nocapture
env -u MEMORUM_EVAL_CLAUDE_KEY -u MEMORUM_EVAL_CODEX_KEY cargo test -p memorum-eval --features live-harness --test live -- --nocapture --test-threads=1
nl -ba crates/memoryd/src/cli.rs | sed -n '55,90p;205,235p;176,195p'
tmp=$(mktemp -d); mkdir -p "$tmp/repo" "$tmp/runtime"; PATH=/usr/bin target/debug/memoryd doctor --repo "$tmp/repo" --runtime "$tmp/runtime" >/tmp/agent-memory-review-doctor.out 2>/tmp/agent-memory-review-doctor.err; code=$?; echo exit=$code; head -200 /tmp/agent-memory-review-doctor.out; cat /tmp/agent-memory-review-doctor.err
```

Results of executable checks:

- `bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh`: pass.
- Installer dry-run with explicit repo/runtime/socket: pass; printed deterministic PID/log paths and MCP snippet.
- Installer dry-run stale PID preservation command: pass; dry-run printed `rm -f` but did not remove the existing file.
- `cargo test -p memoryd --lib doctor_health -- --nocapture`: pass, 1 test.
- `cargo test -p memoryd-tui recall_panel -- --nocapture`: pass, 2 recall panel tests.
- `cargo test -p memoryd-tui panic_restore -- --nocapture --test-threads=1`: pass, 1 mid-render panic restore test.
- `cargo test -p memoryd --lib surfaced_peer_update_references -- --nocapture`: pass, 1 startup recall cooldown-key test.
- `env -u MEMORUM_EVAL_CLAUDE_KEY -u MEMORUM_EVAL_CODEX_KEY cargo test -p memorum-eval --features live-harness --test live -- --nocapture --test-threads=1`: pass, 2 tests with explicit `MEMORUM_EVAL_SKIP:SKIP_NO_AUTH` markers.
- `PATH=/usr/bin target/debug/memoryd doctor --repo "$tmp/repo" --runtime "$tmp/runtime"`: exit 1, demonstrating a non-zero unhealthy doctor path.

## Questions / uncertainties

- I did not rerun `./scripts/check-fast.sh`, `./scripts/check-dogfood.sh`, or the real temp installer smoke because Task 5's gate report already records them as passed and the product review lane only needed targeted confidence. I did not run `scripts/check.sh` per instruction.
- I did not judge unrelated dirty files outside the requested dogfood surfaces except where they affected dogfood commands or gate behavior.

## Positives

- The installer now has the right dogfood shape: `--force-reinstall`, deterministic PID/log files, detached `nohup` start, readiness failure cleanup, and copy-pasteable MCP/lifecycle output.
- The Recall panel is honest about current protocol fields and no longer displays fake `score:n/a`, `harness:n/a`, or `session:n/a` placeholders.
- The gate split is operationally useful: `check-fast` and `check-dogfood` are explicit about what they run and avoid the release-only convergence/durability/bench work.
