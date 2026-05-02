### Verdict

Approved

### Intended outcome

Stream H is intended to ship a durable Memorum eval harness: a complete 19-test catalog, simulator-driven tests for deterministic memory behavior, real-harness tests for Claude/Codex auth-dependent flows, JSON reporting for CI, and an RC-blocking workflow that cannot pass vacuously. This second rerun specifically checks the prior rerun blockers: #13/#15 must dispatch when credentials and CLIs are present instead of skipping as `REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED`, #16 must dispatch instead of stale `STREAM_G_DEPS_NOT_SHIPPED`, and the previously fixed issues must remain fixed: no fabricated pass rows, machine-readable skips, the T19 threshold, clippy, and fmt.

### Executive summary

The latest blocker fixes address the prior rerun findings. The orchestrator now dispatches #13, #15, and #16 through cargo test execution when the relevant credentials/CLIs are present, and regression coverage explicitly prevents the old not-implemented and stale Stream G skip behavior from returning. The requested Cargo gates pass, the focused JSON checks show #16 passing as an executed simulator test, and fake present credentials/CLIs cause #13/#15 to fail through actual dispatched test execution rather than skip. I did not find material blockers in this rerun.

Validation run:

- `cargo test -p memorum-eval --test orchestrator_integration -- --nocapture` — passed; 7 passed, 0 failed.
- `cargo test -p memorum-eval` — passed; all memorum-eval unit, integration, regression, and doc tests passed in the default feature set.
- `cargo clippy -p memorum-eval --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-eval -- --check` — passed.
- Focused #16 JSON check: `cargo run -p memorum-eval -- --harness mock --filter t16 --output json` — exited 0 with `passed: 1`, `skipped: 0`, `partial: false`, and #16 `status: "passed"`, `skip_reason: null`.
- Focused #13 present-credentials/CLI check: with fake `claude`/`codex` CLIs in `PATH` and fake `MEMORUM_EVAL_CLAUDE_KEY` / `MEMORUM_EVAL_CODEX_KEY`, `cargo run -p memorum-eval -- --harness all --filter t13 --output json` exited 1 with #13 `status: "failed"`, `skipped: 0`, `partial: false`, `missing_credentials: []`, and `skip_reason: null`; failure detail came from the dispatched `cargo test -p memorum-eval --test domain t13_cross_harness_substrate_sharing` path invoking the fake Codex CLI.
- Focused #15 present-credentials/CLI check: same fake CLI/key setup, `cargo run -p memorum-eval -- --harness all --filter t15 --output json` exited 1 with #15 `status: "failed"`, `skipped: 0`, `partial: false`, `missing_credentials: []`, and `skip_reason: null`; failure detail came from the dispatched `cargo test -p memorum-eval --test domain t15_privacy_filter_refusal_and_retry` path invoking the fake Claude CLI.
- Focused T19 threshold check: `cargo test -p memorum-eval --all-features --test t19_peer_update_framing t19_threshold_requires_five_of_six_per_harness_and_ten_total -- --nocapture` — passed; the test intentionally caught panics for 4/6 per-harness and 9/12 total fixtures.

### Findings

No material issues found.

### Non-blocking simplifications

- The orchestrator still shells out to `cargo test` for catalog entries (`crates/memorum-eval/src/orchestrator.rs:601-605`). That is acceptable for closing the blocker because it proves real tests execute and failures propagate, but a shared runner API would eventually be cleaner and would avoid nested Cargo invocation overhead.
- There is still spec/plan wording drift around mock mode: `docs/specs/stream-h-eval-harness-v0.1.md:914-915` and §7.4 describe MockHarness backing #13/#15, while `docs/specs/stream-h-eval-harness-v0.1.md:1255` and `docs/plans/2026-05-01-stream-h-eval-harness.md:644` say mock mode skips real-harness tests. The implementation and current regression test choose the MockHarness interpretation for #13/#15 and skip only #19 until Stream I deps are enabled. This is not a code blocker, but the docs should be reconciled before relying on the spec as operator documentation.

### Test gaps

- I could not run #13/#15 against real authenticated Claude/Codex CLIs in this environment. The fake CLI/key checks prove the prior orchestrator blocker is fixed — dispatch occurs and is not skipped — but they do not prove the live LLM/MCP flows succeed.
- The default `cargo test -p memorum-eval` path still treats some dependency/auth skips as successful direct cargo tests, especially default-feature T19 (`stream-i-deps feature disabled`) and unauthenticated real-harness tests. The orchestrator JSON gives machine-readable skip/failure status for release evidence, so this is acceptable as long as reviewers treat the orchestrator output, not direct cargo stdout alone, as the final gate artifact.

### Questions / uncertainties

- Real #13/#15 live behavior remains unverified without authenticated local/CI Claude and Codex CLIs. If those credentials are available in CI, the next RC run should be checked for an actual full-harness `partial: false`, `failed: 0` report.
- #17 and #18 remain intentional semantic skips in the orchestrator (`SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED` and `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED`). I treated those as outside this rerun's blocker scope because the user specifically called out #13/#15, #16, T19, fabricated pass rows, machine-readable skips, clippy, and fmt.

### Positives

- The previous #13/#15 blocker is directly protected by `real_harness_with_present_credentials_does_not_skip_as_not_implemented` in `crates/memorum-eval/tests/orchestrator_integration.rs:92-120`.
- The previous #16 blocker is directly protected by `t16_dispatches_instead_of_stale_stream_g_skip` in `crates/memorum-eval/tests/orchestrator_integration.rs:123-145` and the current #16 JSON run passes with no skip.
- The T19 threshold is now expressed as aggregate run counts (`crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs:264-282`) and has a regression test for 4/6 per-harness and 9/12 total failures (`lines 357-369`).
