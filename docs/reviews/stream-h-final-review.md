### Verdict

Changes requested

### Intended outcome

Stream H is intended to ship a durable Memorum eval harness: a complete 19-test catalog, simulator-driven tests for deterministic memory behavior, real-harness tests for Claude/Codex auth-dependent flows, JSON reporting for CI, and an RC-blocking workflow that cannot pass vacuously. The implementation appears to be trying to land the harness crate, catalog, docs, CI workflow, and concrete tests for Tasks 1-20 while preserving skip semantics for auth-dependent or cross-stream-dependent tests.

### Executive summary

The underlying integration tests are much more substantial than a stub-only harness, and `cargo test -p memorum-eval` passes locally. However, the `memorum-eval` orchestrator binary does not actually execute those tests; it synthesizes pass/skip rows directly from catalog metadata. That breaks the central business outcome: the CI workflow and RC gate can report a successful eval run without exercising the eval suite. I also found a too-weak Test #19 framing threshold, cargo-level skips that appear as passes in the requested test runs, and current `memorum-eval` clippy failures. This should not ship as the final Stream H gate until those are fixed.

Validations run:

- `cargo test -p memorum-eval` — passed.
- `cargo test -p memorum-eval --test domain -- --nocapture` — passed, but #13/#15 skipped for missing auth, #18 skipped for missing Stream D rotation contract, and #17 partially skipped after verifying only the loser backoff path.
- `cargo test -p memorum-eval --test t19_peer_update_framing -- --nocapture` — passed, but default-feature path only printed `SKIP: stream-i-deps feature disabled`.
- `cargo fmt -p memorum-eval -- --check` — passed.
- `cargo clippy -p memorum-eval --all-targets --all-features -- -D warnings` — failed with 4 diagnostics in `assertions.rs`, `harness_runner.rs`, and `simulator.rs`.

### Findings

[High] [Correctness] Orchestrator reports passes without executing the eval tests

- Evidence: `crates/memorum-eval/src/orchestrator.rs`, `run_catalog_entry` returns `passed_result` for every simulator entry and every real-harness entry with credentials (`lines 469-477`), with no call into the handbook/domain/regression tests, `DaemonScaffold`, `SimulatorAgent`, `HarnessRunner`, `MockHarness`, or a cargo test subprocess.
- Why it matters: `.github/workflows/stream-h-eval.yml` runs `cargo run --release -p memorum-eval -- --harness ...`; if the binary never executes the tests, the CI artifact and RC gate can claim Stream H passed while the actual harness behavior is broken. This directly defeats the Stream H goal of “tests that can fail.”
- Reasoning: The implementation has real integration tests under `crates/memorum-eval/tests/**`, and those tests do run under `cargo test`. But the productized orchestrator used by CI is separate from Cargo’s integration-test runner. Its result generation is metadata-only: simulator tests always become one-passed-assertion rows; real-harness tests either become `SKIP_NO_AUTH` or passed rows. A failing handbook/domain test would not affect `memorum-eval --output json` unless the orchestrator invokes that test logic.
- Recommendation: Make `memorum-eval` execute the catalog entries through real runner functions shared with the tests, or intentionally invoke the appropriate cargo test targets/subprocesses and translate their results into the JSON schema. Add an integration test that proves a deliberately failing catalog entry causes `failed > 0` and exit code 1, rather than only checking JSON shape.
- Confidence: High

[Medium] [Tests] Several requested “skips” are indistinguishable from passes under cargo test

- Evidence: `crates/memorum-eval/tests/eval/domain/t13_cross_harness_substrate_sharing.rs:20-27` returns early on missing auth/CLI; `t15_privacy_filter_refusal_retry.rs:22-29` does the same; `t17_lease_contention_resolution.rs:29-37` returns early after verifying only Device B loser backoff; `t18_encrypted_tier_key_rotation.rs:17-24` returns early when the Stream D rotation contract is absent; `t19_peer_update_framing.rs:14-18` prints a skip message and exits successfully when `stream-i-deps` is disabled. The requested `cargo test ... -- --nocapture` commands reported all of these as `ok`.
- Why it matters: A reviewer or CI job looking only at cargo’s pass/fail result cannot distinguish “full behavior verified” from “dependency/auth unavailable, semantic coverage skipped.” That creates a false sense of coverage, especially for the real-harness and cross-stream contract cases Trey explicitly asked this review to scrutinize.
- Reasoning: Early-return skips are reasonable for local developer ergonomics, but the current cargo-level tests do not emit machine-readable skip status. Until the orchestrator actually runs tests and records `skipped`/`partial` accurately, the readout is vacuous for several critical Stream H claims.
- Recommendation: Centralize skip handling in the orchestrator result model and make cargo tests assert the skip result explicitly rather than silently returning. For direct cargo integration tests, consider `#[ignore]` for auth-only real-harness tests plus a separate mock/skip-contract test, or panic in full-gate modes when required dependencies are absent.
- Confidence: High

[Medium] [Correctness] Test #19 pass threshold is weaker than the spec

- Evidence: `docs/specs/stream-h-eval-harness-v0.1.md:1259` requires >=5 of 6 framing-correct outcomes per harness. The enabled implementation in `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs:255-268` counts successful temperature cases and allows `case_count - 1`; each case is marked correct at `lines 280-284` when only 2 of 3 runs pass. With 3 temperature cases per harness, a harness can pass with 2 correct cases \* 2 correct runs = 4 correct runs out of 9.
- Why it matters: Peer-update framing is the exact abuse/correctness property Test #19 is meant to guard. A harness that mishandles framing in most invocations could still pass if failures cluster in the tolerated runs/cases.
- Reasoning: The implementation applies a majority-per-temperature threshold and then tolerates one failed temperature case. The spec’s wording is outcome-count based, not nested-majority based. Even if the final intended matrix is 9 invocations per harness instead of 6, the threshold should be expressed against total run outcomes with the documented tolerance, not double-discounted by case grouping.
- Recommendation: Count total `RunOutcome` values per harness and enforce the spec threshold directly. If Stream I’s final matrix is 9 per harness, update the spec/docs and use an equivalent explicit threshold (for example, at most one miss per harness, or a documented percentage), then report `framing_correct: <correct>/<total>` in details.
- Confidence: High

[Medium] [Maintainability] `memorum-eval` does not pass the required clippy gate

- Evidence: `cargo clippy -p memorum-eval --all-targets --all-features -- -D warnings` failed with: `clippy::manual_strip` in `crates/memorum-eval/src/assertions.rs:156-157`; `clippy::too_many_arguments` in `src/harness_runner.rs:182` and `src/simulator.rs:158`; and `clippy::no_effect_replace` in `src/simulator.rs:313-315`.
- Why it matters: The repo instructions and requested validation include a no-warnings clippy gate. Beyond hygiene, the `no_effect_replace` diagnostic is on JSON escaping code, where mistakes can corrupt daemon requests containing backslashes.
- Reasoning: These are current compile-lint failures under the exact requested command. They will block any CI path that enforces the stated Rust quality gate.
- Recommendation: Fix the four diagnostics rather than suppressing them. Use `strip_prefix`, introduce small request/config structs for the too-many-argument helpers where it improves readability, and correct the JSON backslash escaping to replace `\` with `\\`.
- Confidence: High

### Non-blocking simplifications

- Share the `block_on`/no-op-waker helper used across several sync tests, or just use `#[tokio::test]` consistently now that `tokio` is already a dev dependency. This would reduce repeated unsafe test scaffolding.
- Prefer one JSON serialization path (`serde_json`) for daemon requests and orchestrator output instead of hand-built strings. Several helpers manually escape JSON, and the clippy finding shows this is easy to get subtly wrong.

### Test gaps

- No test proves that `memorum-eval` executes a real catalog entry and propagates an assertion failure into JSON/exit code. Current orchestrator tests mainly verify catalog count and report shape.
- No machine-readable test verifies the cargo-level skip contract for real-harness auth absence, Stream D absence, and Stream I dependency absence; they print skip text but are reported as `ok`.
- Test #19’s all-features path is not validated by the requested normal `cargo test` command because the default-feature test is only a skip. The all-features clippy build compiles the path, but it does not exercise the matrix.
- Real-harness auth behavior could not be fully validated in this environment because the required `MEMORUM_EVAL_CLAUDE_KEY` / `MEMORUM_EVAL_CODEX_KEY` and authenticated CLIs were not available to the cargo tests I ran.

### Questions / uncertainties

- Is the intended production orchestrator supposed to run Rust integration tests via Cargo, or should the catalog logic be refactored into reusable library functions? The current implementation is neither, so the desired architecture needs a decision.
- The spec has some historical 18-test examples in the JSON section, but later sections and docs consistently say 19. I treated 19 as the current intended contract.
- Stream D rotation and Stream I framing dependencies appear intentionally cross-stream-gated. I could only verify that their skip paths compile/run, not the full semantic behavior.

### Positives

- The 19-test catalog, docs/api, docs/dev catalog, and workflow field names are broadly aligned: `number`, `name`, `failure_detail`, `partial`, and `missing_credentials` are used consistently in docs/tests/workflow checks.
- The actual handbook/domain integration tests are substantive and exercise daemon/socket behavior rather than only unit-level stubs.
- MockHarness output is clearly annotated as mock mode, which is important for not overstating LLM reasoning coverage.
