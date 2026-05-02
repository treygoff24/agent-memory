### Verdict

Changes requested

### Intended outcome

Stream H is intended to ship a durable Memorum eval harness: a complete 19-test catalog, simulator-driven tests for deterministic memory behavior, real-harness tests for Claude/Codex auth-dependent flows, JSON reporting for CI, and an RC-blocking workflow that cannot pass vacuously. This rerun specifically checks whether the prior blockers were fixed: runnable tests must not be represented by fabricated pass rows, skip states must be machine-readable in JSON, Test #19 must enforce the spec threshold, and the `memorum-eval` clippy gate must be clean.

### Executive summary

The blocker fixes materially improved the implementation: simulator catalog entries now dispatch to `cargo test` subprocesses, a fake-cargo regression proves runnable failures propagate into JSON/exit status, skip states are represented as `skipped` with `partial` and `missing_credentials`, Test #19 now enforces 5/6 per harness and 10/12 total, and all requested Cargo gates pass. However, the final RC path is still not shippable because real-harness entries #13 and #15 are skipped as `REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED` even when credentials are present, and the orchestrator hard-skips runnable Test #16 even though the direct domain test now passes. This means the productized `memorum-eval --harness all` path cannot produce a full eval pass and still omits required coverage.

Validations run:

- `cargo test -p memorum-eval` — passed.
- `cargo test -p memorum-eval --test domain -- --nocapture` — passed; #13/#15 printed `SKIP_NO_AUTH`, #18 printed `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED`, and #17 printed `SKIP_ADAPTATION`.
- `cargo test -p memorum-eval --test t19_peer_update_framing -- --nocapture` — passed; default-feature path printed `SKIP: stream-i-deps feature disabled`.
- `cargo clippy -p memorum-eval --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt -p memorum-eval -- --check` — passed.
- Extra JSON check: `cargo run -p memorum-eval -- --harness all --output json` with no eval keys exited 1 and emitted `partial: true`, `missing_credentials: ["MEMORUM_EVAL_CLAUDE_KEY", "MEMORUM_EVAL_CODEX_KEY"]`, and `skipped` rows for #13/#15/#16/#17/#18/#19.
- Extra JSON check: `MEMORUM_EVAL_CLAUDE_KEY=fake MEMORUM_EVAL_CODEX_KEY=fake cargo run -p memorum-eval -- --harness all --output json` exited 1 with `partial: true`, no `missing_credentials`, and #13/#15 skipped as `REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED`.

### Findings

[High] [Correctness] Full real-harness mode still cannot run the real-harness tests

- Evidence: `crates/memorum-eval/src/orchestrator.rs:503-516` dispatches real-harness catalog entries to `Skip(REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED)` whenever credentials are present. With fake credential env vars set, `cargo run -p memorum-eval -- --harness all --output json` produced `partial: true`, `missing_credentials: []`, and skipped #13/#15 with `skip_reason: "REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED"`.
- Why it matters: Stream H's release-candidate gate is supposed to execute the real Claude/Codex flows for tests #13 and #15 when auth is available. As implemented, the full gate can never become a full pass: missing auth skips as `SKIP_NO_AUTH`, while present auth skips as "not implemented." This preserves the prior business risk in a different form: the CI artifact cannot prove the real harness behavior works.
- Reasoning: The spec says the `memorum-eval` binary runs the tests and that `--harness <MODE>` determines which harness backs real-harness tests (`docs/specs/stream-h-eval-harness-v0.1.md:904-915`). The same spec marks #13 and #15 as serial real-harness tests (`docs/specs/stream-h-eval-harness-v0.1.md:981-987`) and says `--harness all` full runs should fail only when `partial` because of missing auth (`docs/specs/stream-h-eval-harness-v0.1.md:993-1002`). A non-auth "not implemented" skip is not a valid final-state result for those entries.
- Recommendation: Wire real-harness catalog entries to real execution. The simplest acceptable fix is to dispatch #13 and #15 through their existing cargo integration tests when credentials/CLIs are available, and only emit `SKIP_NO_AUTH` for missing auth/CLI. A stronger design is to extract their runner logic into reusable library functions and have both cargo tests and the orchestrator call the same code. Add an integration test that sets credential env vars and proves #13/#15 do not resolve to `REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED`.
- Confidence: High

[Medium] [Correctness] The orchestrator still skips runnable Test #16

- Evidence: `crates/memorum-eval/src/orchestrator.rs:519-524` unconditionally maps catalog entry #16 to `STREAM_G_DEPS_NOT_SHIPPED`. But `cargo test -p memorum-eval --test domain -- --nocapture` ran `t16_reality_check_drift_scores_order_and_explain_components` and it passed. The direct test has its own runtime dependency guard (`crates/memorum-eval/tests/eval/domain/t16_drift_scoring_sanity.rs:14-27`), so it can decide whether Stream G is actually present. The orchestrator bypasses that logic.
- Why it matters: The spec and plan place Test #16 in the parallel simulator group, not in a permanent skip bucket (`docs/specs/stream-h-eval-harness-v0.1.md:971-979`; `docs/plans/2026-05-01-stream-h-eval-harness.md:643-644`). The updated catalog-count section says mock mode should skip #13, #15, and #19, yielding 16 passed / 19 total in the absence of real-harness credentials (`docs/specs/stream-h-eval-harness-v0.1.md:1255`). The current mock JSON reported only 15 passed and skipped #16.
- Reasoning: This is the inverse of the original "fabricated pass" blocker: a runnable catalog entry is now hidden behind a stale hard-coded dependency skip. The cargo test already checks the live daemon contract and only returns early if the dependency is absent; the orchestrator should not preempt it with stale dependency knowledge.
- Recommendation: Remove the hard-coded #16 semantic skip and dispatch #16 through `cargo_dispatch` like the other simulator tests. If Stream G is absent in another checkout, let the test's own runtime guard decide the result, or make the guard return a machine-readable skip to the orchestrator through a shared result layer.
- Confidence: High

### Non-blocking simplifications

- The orchestrator currently shells out to `cargo test` for simulator entries, while tests also contain direct runnable logic. This is acceptable as a blocker fix, but a shared runner API would be cleaner long-term: one execution path would feed both cargo tests and JSON orchestration, and semantic skip reasons could be returned as typed outcomes instead of parsed or duplicated.
- `partial` currently means any skipped test, not only missing-auth skips. That is conservative for RC gating and useful for the current semantic skips, but it diverges slightly from spec language that defines `partial` around missing real-harness auth. If the team keeps the broader meaning, update the spec text to say partial means any intentionally incomplete eval run.

### Test gaps

- No test covers the "credentials present" full-harness path. Add a test with fake credential env vars and fake harness/CLI adapters proving #13/#15 are executed or fail as real runs, not skipped as `REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED`.
- No test protects #16 from being skipped after Stream G dependencies are present. Add an orchestrator integration assertion that `--harness mock --output json` does not skip #16 when the runtime probe succeeds, matching the spec's 16 simulator-driven tests.
- The direct cargo tests still use stdout/stderr early-return skips for #13/#15/#17/#18/#19. The orchestrator now gives machine-readable skip rows for its own run, but cargo-only consumers still see these as `ok`; that is acceptable for local ergonomics but should be documented or replaced with typed shared outcomes if direct cargo runs are treated as release evidence.

### Questions / uncertainties

- Should #17 remain a semantic skip until Stream F lease re-entrancy changes, or is the current "verified loser backoff only" path acceptable for Stream H? I treated it as an intentional semantic partial because the test prints `SKIP_ADAPTATION` and the orchestrator reports `SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED`.
- Should #18 remain a semantic skip until Stream D rotation contract work lands? I treated it as intentional because the direct test probes for the contract and reports `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED`.
- Test #19's enabled path now matches the spec threshold numerically, but I could not exercise the full Stream I/real-harness path in this environment because `stream-i-deps` is disabled and real harness credentials/CLIs are not available.

### Positives

- The original fabricated-pass issue for simulator entries is substantially addressed: `run_catalog_entry` now dispatches to `cargo test`, and `runnable_catalog_failure_propagates_to_json_and_exit_code` proves a subprocess failure becomes a JSON failure and non-zero exit.
- Skip semantics are now visible in orchestrator JSON: `status: "skipped"`, `skip_reason`, top-level `partial`, and `missing_credentials` are present and parseable.
- The previous clippy blockers are closed, and the exact requested clippy command passes cleanly.
