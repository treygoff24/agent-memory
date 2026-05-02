# Stream H Fresh-Eyes Review (Claude, 2026-05-02)

Snapshot: 6095cf6
Method: spec/plan/code triangulation, read-only

---

## Verdict

Stream H is substantially real — the test files contain genuine assertions against live daemon state, the orchestrator dispatches via actual `cargo test` subprocesses rather than mocks, and the harness runner properly skips with clear reasons when CLIs or credentials are absent — but three tests (T17, T18, T19) are permanently auto-skipped at the orchestrator layer, making the "19-test catalog" effectively a 16-test catalog in any realistic CI run.

---

## Blockers (must-fix before merge)

**B1: T17 and T18 are semantic-skipped unconditionally in the orchestrator.**
`orchestrator.rs:518-521` — `semantic_skip_reason()` returns `SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED` for entry 17 and `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED` for entry 18 before any test code runs. This means `cargo test -p memorum-eval` will compile and run those test files, but the orchestrator binary (the RC gate surface) will permanently skip them and count them as skipped rather than run. The `--list` output shows 19 tests, the actual gate runs 16 at most. The Codex final-gate report claims "fabricated pass rows are gone" but these are structural skip rows, not fabricated passes — they still obscure incomplete coverage. The spec §3.2 tests #17 and #18 describe fully specified behaviors that the daemon purportedly implements (Stream F lease semantics and Stream D key rotation); skipping them at the orchestrator level without a versioned deferral notice in the spec is a coverage gap, not an honest skip.

**B2: T19 is permanently skipped at the orchestrator level unless `stream-i-deps` feature is enabled.**
`orchestrator.rs:522` — `19 if !cfg!(feature = "stream-i-deps") => Some(STREAM_I_DEPS_DISABLED)`. The feature is not enabled by default in `Cargo.toml` (confirmed absent from the feature list). The CI workflow at `.github/workflows/stream-h-eval.yml` does not pass `--features stream-i-deps` to any `cargo run` or `cargo build` invocation. So T19 skips in every CI run including RC gates. The spec §10.1 says T19 is the "single Stream I framing test" and that it "counts as one entry in the eval catalog." Having it permanently skip in the gate run makes the RC-blocking step meaningless for peer-update framing correctness.

**B3: `passed_result()` reports `assertions: 1, assertions_passed: 1` for every passing test regardless of actual assertion count.**
`orchestrator.rs:641-654` — `passed_result()` hardcodes `assertions: 1, assertions_passed: 1`. This is called for all cargo-test-dispatched entries. Tests like T01 have 6+ distinct assertions; T11 has 9+. The JSON report emitted to CI artifacts will always show `"assertions": 1` for every passing simulator test. The spec §6.2 output format implies these fields should reflect actual assertion counts. This is not just cosmetic: the `"Fail if eval did not pass"` CI step uses `jq -r '.failed'` which is correct, but any tooling or operator reading the JSON report for assertion granularity (e.g., "which assertions passed?") gets misleading data. This is a honesty gap in the JSON output, even though it does not cause false passes.

---

## Risks

**R1: T16 bypasses `EventLogInjector` and instead writes raw SQLite directly.**
`t16_drift_scoring_sanity.rs:161-268` — `seed_drift_inputs()` opens `.memoryd/index.sqlite` directly with `sqlite3` CLI and INSERTs/UPDATEs rows, including injecting `events_log` `recall_hit` entries. The spec §4.2 says this should go through `EventLogInjector` via `RequestPayload::TestInjectEvent` (a `test-utils` feature-gated daemon helper). Going directly to SQLite bypasses any daemon-side invariants on `events_log` (e.g., deduplication, seq number assignment). It also hard-depends on `sqlite3` being in `PATH` on CI machines. If the index schema evolves (column renames, new NOT NULL constraints), this test will fail silently at the sqlite3 layer before Rust panics. Not a blocker because the test still exercises the real drift scoring path and the assertions are genuine, but the injection mechanism diverges from spec §4.2 intent.

**R2: `block_on` in the orchestrator is a home-rolled poll loop without any parking.**
`orchestrator.rs:711-722` — `block_on()` busily polls the future via `yield_now()` in a tight loop. This is used to spin up `DaemonScaffold` for mock harness tests in `run_mock_harness()`. On a loaded CI machine this will busy-spin for however long `DaemonScaffold::fresh()` takes (process spawn + socket readiness). A proper executor or even `std::thread::sleep` in the poll loop would reduce noise. Not a correctness issue but a resource smell.

**R3: T17 test file contains a well-structured skip path for the non-re-entrant lease case.**
`t17_lease_contention_resolution.rs:30-37` — the test itself correctly handles the case where Device A's pre-seeded lease returns `lease_held` (it prints a skip message and returns). But because the orchestrator skips T17 unconditionally at `orchestrator.rs:520` before the test binary ever runs, this self-skip logic never executes. There are two separate skip mechanisms for T17 and they are not coordinated. If the orchestrator skip is ever removed, the test's own skip path will fire correctly — but the current state means that even if the lease reentrancy contract ships, T17 stays skipped unless someone also removes line 520 from the orchestrator.

**R4: `validate_xml` in `assertions.rs` requires root tag to be `memory-recall` but does not validate namespace attributes.**
`assertions.rs:265` — `validate_xml` returns `Err` if the root tag is not `memory-recall`. This means T04 (abstention) would fail if the daemon emits an empty-root-tag response like `<memory-recall version="stream-e-v0.5"/>` with a version attribute containing a slash, since `tag_name()` at line 311 just splits on whitespace and takes the first token. This is fine for current output shapes but fragile if the Stream E version string ever includes `/` or other characters that split weirdly.

---

## Nits

- `orchestrator.rs:836-844` — `timestamp_string()` emits `"unix-ms:<millis>"` instead of ISO 8601. The spec §6.2 example shows `"2026-05-01T03:00:00Z"`. The CI `jq` step does not parse timestamps, so this is not a gate break, but any tooling that parses the JSON for time range queries will get non-standard strings.

- `harness_runner.rs:493-498` — `extract_string_field()` is a hand-rolled JSON field extractor used in `MockHarness::run_test_15`. It will silently return `None` (and fall through to `"unknown"`) for nested or escaped JSON values. Given that the mock harness generates the JSON itself, this is unlikely to bite, but it is unnecessary fragility.

- `t16_drift_scoring_sanity.rs:10` — the `STREAM_G_RC_HANDLER_NOT_SHIPPED` guard at the top of the test correctly causes an early return rather than a panic when Stream G's `RealityCheck(List)` handler is not wired. This is honest — the test emits a clear eprintln and returns, which `cargo test` counts as a pass. The orchestrator would report this as `passed`. This is arguably a nit rather than a risk because the guard is documented inline, but it means T16 can "pass" without actually asserting anything if Stream G is not shipped. Worth calling out as a known limitation in the test's doc comment.

---

## 19-test catalog reality matrix

| Test | File                                       | Status                | Notes                                                                                                                                                                                             |
| ---- | ------------------------------------------ | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T01  | `t01_exact_identifier_recall.rs`           | **real**              | 6 distinct assertions on XML recall, search rank, entity no-duplicate, status counter                                                                                                             |
| T02  | `t02_superseded_fact.rs`                   | **real**              | Exercises supersession chain, recall exclusion, FTS ranking                                                                                                                                       |
| T03  | `t03_cross_project_entity_collision.rs`    | **real**              | Two-namespace project binding, entity isolation                                                                                                                                                   |
| T04  | `t04_abstention.rs`                        | **real**              | Novel-topic empty-result path, XML validity on zero-result block                                                                                                                                  |
| T05  | `t05_poisoned_candidate.rs`                | **real**              | Confidence-floor quarantine, recall exclusion                                                                                                                                                     |
| T06  | `t06_tool_output_preservation.rs`          | **real**              | Artifact handle verbatim preservation                                                                                                                                                             |
| T07  | `t07_subagent_writeback.rs`                | **real**              | Grounding refusal for unresolvable spawn ref; diverges from spec's "promoted or candidate" expectation (asserts `refused`/`grounding`) — behaviorally honest given current governance policy      |
| T08  | `t08_deletion_and_tombstone.rs`            | **real**              | Tombstone creation, recall exclusion, re-insertion block                                                                                                                                          |
| T09  | `t09_recall_budget_pressure.rs`            | **real**              | Budget trimming, gold memory survival, omitted_count                                                                                                                                              |
| T10  | `t10_compaction_resumption.rs`             | **real**              | Two-session compaction simulation, no-duplicate assertion                                                                                                                                         |
| T11  | `t11_self_poisoning.rs`                    | **real**              | Circular grounding refusal, correct supersession, search rank                                                                                                                                     |
| T12  | `t12_temporal_validity.rs`                 | **real**              | expired/future valid_until filtering                                                                                                                                                              |
| T13  | `t13_cross_harness_substrate_sharing.rs`   | **real** (auth-gated) | Real subprocess invocation, one-parse-retry, JSONL fragment scan; skips cleanly without auth                                                                                                      |
| T14  | `t14_merge_driver_semantic_correctness.rs` | **real**              | Invokes `memory-merge-driver` binary, asserts both entity sets and confidence in merged output, runs `memoryd doctor` on result                                                                   |
| T15  | `t15_privacy_filter_refusal_retry.rs`      | **real** (auth-gated) | PII refusal → retry flow, disk-scan assertion for PII absence                                                                                                                                     |
| T16  | `t16_drift_scoring_sanity.rs`              | **partial**           | Real assertions on component scores and weighted sum; skips gracefully (but as a pass) if Stream G handler not wired; SQLite injection bypasses spec's `EventLogInjector`                         |
| T17  | `t17_lease_contention_resolution.rs`       | **partial**           | Test code is real and well-structured; orchestrator unconditionally skips it before the binary runs (B1)                                                                                          |
| T18  | `t18_encrypted_tier_key_rotation.rs`       | **partial**           | Test code is real; orchestrator unconditionally skips it (B1); test has its own contract-present guard if the skip is ever removed                                                                |
| T19  | `t19_peer_update_framing.rs`               | **partial**           | Full sampling-matrix structure with `FramingAssertion` inline impl, threshold tests included; permanently skip in CI because `stream-i-deps` feature is not enabled anywhere in the workflow (B2) |

---

## Coherence observations

The 12-hour run held together well at the structural level. The crate compiles, the orchestrator has real parallelism logic, the mock harness actually exercises daemon protocol paths (it sends real JSON frames over the Unix socket), and the JSON output format matches the spec's §6.2 shape with the exception of the assertion-count fields (B3) and the timestamp format (nit).

The real-harness invocation path in `harness_runner.rs` is the most credible part of the implementation: it actually shells out (`Command::new(cli)`), delivers prompts via stdin pipe (per spec §5.1's stdin-transport requirement), parses `--help` output to validate CLI compatibility before flagging `HARNESS_INCOMPATIBLE_CLI`, and emits `HARNESS_TIMEOUT` on the stderr of timed-out runs. This is honest dispatch.

The main structural problem is the T17/T18/T19 triple-skip: the orchestrator binary that serves as the RC gate will always report `total: 19, passed: 16, skipped: 3` in mock mode and `total: 19, passed: 16 (or fewer), skipped: 3` in real-harness mode. The spec's RC-gate semantics at §7.5 say "a `partial: true` run exits 1 for RC tags" — and indeed, `skipped > 0` sets `partial: true` in `orchestrator.rs:175` — but `exit_code_for_report` at line 404-415 only fails on `partial && harness_mode != Mock`. So in mock mode, these three permanent skips will not fail the RC gate. This is arguably the right call for T13/T15/T19 (auth-dependent), but for T17 and T18 (simulator tests that are skipped purely because their contracts haven't shipped yet) it means those gaps are invisible to the RC gate.

---

## Spec coverage matrix

| Spec section             | Covered          | Gap or note                                                                                                               |
| ------------------------ | ---------------- | ------------------------------------------------------------------------------------------------------------------------- |
| §3.1 tests #1–#12        | Yes              | All 12 have real assertions                                                                                               |
| §3.2 test #13            | Yes (auth-gated) | Dispatch honest, skips clearly                                                                                            |
| §3.2 test #14            | Yes              | Merge-driver binary invocation, doctor check                                                                              |
| §3.2 test #15            | Yes (auth-gated) | PII refusal path exercised                                                                                                |
| §3.2 test #16            | Partial          | Stream G guard allows silent pass when handler not wired                                                                  |
| §3.2 test #17            | Partial          | Test code real; orchestrator skips unconditionally                                                                        |
| §3.2 test #18            | Partial          | Test code real; orchestrator skips unconditionally                                                                        |
| §8 / regression          | Yes              | T19 slot exists with real sampling-matrix structure                                                                       |
| §6.2 JSON output         | Partial          | `assertions` field hardcoded to 1 (B3); timestamp non-ISO                                                                 |
| §7 CI workflow           | Mostly           | No `if: false` or `continue-on-error`; real `jq`-based gate; T19 feature-flag gap (B2)                                    |
| §5 real-harness dispatch | Yes              | Actual subprocess invocation with stdin transport, timeout, retry                                                         |
| §7.4 MockHarness         | Yes              | Calls real daemon protocol; annotates output with "mode: mock"                                                            |
| §9 meta-acceptance tests | Yes              | Orchestrator smoke, simulator connectivity, dream-path exclusion, privacy filter tests all present as separate test files |
