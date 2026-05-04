# Final gate report — Streams F/G/H/I dogfood readiness

Date: 2026-05-04
Baseline: `4e87c70 Fix post-shipping audit findings`
Verdict: targeted gates green after a narrow CLI-contract fix. Full canonical `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` was attempted once, failed on stale panel-range test coverage, and was not rerun by request.

## Prompt-to-artifact checklist

- Plan doc updated: `docs/plans/2026-05-04-streams-fghi-dogfood-readiness.md` now includes a live progress update.
- TUI Recall: `crates/memoryd-tui/src/panels/recall.rs`, app/client wiring, `tests/recall_panel.rs`.
- Panic restore: `crates/memoryd-tui/src/main.rs`, `tests/panic_restore.rs`.
- Stream F v0.3 spec: `docs/specs/stream-f-dreaming-v0.3.md`.
- EncryptAtRest refusal: `crates/memoryd/src/dream/orchestration.rs`, `handler_contract` coverage.
- Auth probe diagnostics: `crates/memoryd/src/dream/harness.rs`, `dream/status.rs`, `handlers.rs` doctor wiring.
- Cleanup path validation: `crates/memoryd/src/dream/cleanup.rs` now uses `RepoPath::try_new`.
- Echo production hardening: `crates/memoryd/src/dream/orchestration.rs`, `crates/memoryd/Cargo.toml` `dev-fixtures` feature.
- launchd scheduler: `scripts/templates/com.memorum.dream-scheduled.plist.template`, `scripts/install-launchd.sh`, `docs/runbooks/dream-scheduling.md`.
- Stream I naming/docs: `memorum-coordination` helper rename and `docs/api/stream-i-cross-session-api.md`.
- Stale baselines: `.proposed` Stream G/I bench files removed.
- Specgate orphan fix: obsolete Rust Stream A spec stubs removed; `modules/memoryd-web-static.spec.yml` added for the only source file the installed Specgate ownership doctor discovers.
- Baseline discipline: `scripts/check-baseline-discipline.sh` wired into `scripts/check.sh`.
- F-003 audit: `docs/reviews/2026-05-04-f003-ratification-audit.md`.
- Eval deferral/live docs: `memorum-eval` deferred field, `tests/live.rs`, `docs/runbooks/eval-real-harness-ci.md`.
- Day-one onboarding: `scripts/install-memorum.sh`, `docs/runbooks/dogfooding-day-one.md`.
- Final security audit: `docs/reviews/2026-05-04-final-security-audit.md`.

## Verification run in this pass

- `cargo check -p memoryd --tests` — pass.
- `cargo check -p memorum-coordination --tests` — pass.
- `cargo check -p memorum-eval --features live-harness --tests` — pass.
- `cargo test -p memoryd-tui -- --test-threads=2` — pass.
- `cargo test -p memoryd --test dream_pass_pipeline -- --test-threads=2` — pass.
- `cargo test -p memoryd --test handler_contract -- --test-threads=2` — pass.
- `cargo test -p memoryd --test dream_cleanup -- --test-threads=2` — pass.
- `cargo test -p memoryd --test dream_harness_cli -- --test-threads=1` — pass. The earlier parallel `--test-threads=2` run failed after a subprocess-test timeout poisoned the shared test lock; serial rerun passed.
- `cargo test -p memoryd --test doctor_mirror_health -- --test-threads=2` — pass.
- `cargo test -p memorum-coordination -- --test-threads=2` — pass.
- `cargo test -p memorum-eval --features live-harness --test orchestrator_integration deferred_feature_skip_marker_reports_feature_deferred_kind -- --test-threads=1` — pass.
- `cargo test -p memorum-eval --features live-harness --test orchestrator_integration filtered_json_run_reports_spec_result_fields -- --test-threads=1` — pass.
- `cargo test -p memorum-eval --features live-harness --test live -- --test-threads=1` — pass/skips honestly when env vars are absent.
- `cargo build -p memoryd --release` — pass.
- `specgate validate` — pass, 0 warnings.
- `specgate check --output-mode deterministic` — pass, 0 violations.
- `specgate doctor ownership --project-root . --format json` — status ok after ownership-spec cleanup.
- `scripts/install-launchd.sh --dry-run --repo /tmp/foo --runtime /tmp/bar` — rendered expected plist.
- `scripts/install-memorum.sh --dry-run --repo /tmp/memorum-test --runtime /tmp/memorum-test/.memoryd --socket /tmp/memoryd-test.sock` — printed install steps and MCP snippet.
- `scripts/check-baseline-discipline.sh` — pass against current HEAD; synthetic canonical-bench violation fails as expected.

## Canonical gate status

- Full canonical `BENCH_PROFILE=darwin-arm64 bash scripts/check.sh` was run once. It failed during `cargo test --workspace` at `crates/memoryd/tests/cli_contract.rs::test_clap_rejects_panel_out_of_range` because the test still treated panel `9` as out of range after the Recall panel made `9` valid. The stale assertion was updated to reject `10` instead.
- Narrow verification after that fix: `cargo test -p memoryd --test cli_contract -- --test-threads=2` — pass, 18 tests.
- Per Trey instruction, the full canonical gate was not rerun after the narrow fix.
- Live LLM smoke with real `MEMORUM_EVAL_CLAUDE_KEY`/`MEMORUM_EVAL_CODEX_KEY` was not run; local live tests exercised the honest skip path without secrets.
- The plan's intended multi-reviewer/subagent swarm was not used; this report is a local Codex integration audit.
