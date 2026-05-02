# Stream H Eval Harness Implementation Plan

**Status (2026-05-01):** Draft. Ready for execution.

**Goal:** Build the `crates/memorum-eval/` eval harness for Memorum — a 19-test catalog (12 handbook tests + 6 domain tests + 1 Stream I framing test), a `SimulatorAgent` that drives `memoryd` deterministically, a `HarnessRunner` for real-harness end-to-end tests, a `memorum-eval` orchestrator binary, and CI integration with RC-blocking and daily non-blocking modes. Every production failure that slips through becomes a permanent regression test.

**Source contract:** `docs/specs/stream-h-eval-harness-v0.1.md` (patched 2026-05-01). Cross-stream dependencies: Stream G v0.1 §5.7 for drift-scoring RC protocol (test #16); Stream I v0.1 §10.4 for peer-update framing matrix (test #19); Stream D v0.1 rotation contract in spec §3.2 (test #18).

**Execution model:** Claude executes sequentially in `main` — no worktrees, no parallel subagents running gates. Per-task gate: `cargo test -p memorum-eval --test <test_name>`. Trunk gate after Stream H integrates: `bash scripts/check.sh`.

**Stream A invariants apply throughout.** Stream H must not mutate canonical Stream A files outside test temp trees. The `test-utils` feature on `memoryd` is the only production-surface change Stream H lands; all other test machinery lives in `crates/memorum-eval/`.

---

## Inter-stream coordination

Stream H runs in parallel with Stream G (Codex) and Stream I (Codex). Several touchpoints require explicit sequencing.

**Hard-blocking sequencing (Stream H pauses until upstream lands on `main`):**

- **Test #16** (drift scoring sanity, Task 13) blocks on Stream G plan Task 2 (`EventKind::RecallHit` + `events_log` SQLite mirror) AND Stream G plan Task 5 (`RealityCheckRequest::List` daemon protocol) shipping to `main`. Until both are integrated, Task 13 implements the test stub against the contract, runs RED, and returns `STREAM_G_DEPS_NOT_SHIPPED` skip when invoked. Once Stream G integrates, the skip-guard returns false and the test runs the full sequence.
- **Test #19** (peer-update framing, Task 17) blocks on Stream I plan Task 20 (prompt fixture authorship under `crates/memorum-eval/fixtures/prompts/t19_peer_update_framing.md`) AND Stream I shipping `memorum-coordination::framing_tests::assert_framing`. The `stream-i-deps` cargo feature gate (added in Task 17) cleanly handles compile-time absence; the runtime skip-guard handles fixture absence. Stream I plan Task 20 declares Stream H Task 1 as its prerequisite (sequencing is bilateral: H must create the directory; I must populate the file).
- **Test #18** (key rotation, Task 14) blocks on Stream D v0.1.1 rotation-contract amendment landing on `main`. Test #14 implements a contract-semantic skip-guard that returns `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED` until the decommissioned-keys directory + active.json manifest + multi-key fallback decryption are present.

**Non-blocking coordination (file collisions Stream H rebases through):**

- **`crates/memoryd/Cargo.toml`** is modified by both Stream G plan Task 8 (adds `reqwest`, `lettre` deps for notification dispatch) and Stream H plan Task 5 (adds `[features] test-utils = []` and `stream-g-events = []`). **Stream G ships first**: Task 8 lands its dep additions to `main`; Stream H Task 5 rebases its feature additions on top. The orchestrator (Trey) checks `git log --oneline main | head` before integrating Stream H's Task 5 worktree.
- **`crates/memoryd/src/protocol.rs`** is touched by Stream G Task 5 (adds `RealityCheck*` variants + `MethodNotAllowedOnMcp` error variant) and Stream I plan Task 10 (adds `PeerHeartbeat`/`PeerClaimAcquire`/`PeerClaimRelease` variants) and Stream H Task 5 (adds `TestInjectEvent` behind `#[cfg(feature = "test-utils")]`). All three are additive at the enum level; conflicts are resolvable by accepting all three sets of variant additions. The `MethodNotAllowedOnMcp` error variant is created by Stream G Task 5 — Stream H reuses it without redeclaration.
- **`crates/memoryd/src/mcp.rs`** match arms for MCP rejection: Stream G adds RealityCheck arms; Stream I adds peer-state arms; Stream H adds the `TestInjectEvent` arm. All three return `MethodNotAllowedOnMcp`.

**Trunk gate runs once after all three streams integrate** — not per-stream-per-task. The trunk gate `bash scripts/check.sh` is the source of truth for "Memorum compiles and passes its workspace tests with G + H + I in place."

---

### Task 1: Workspace Skeleton and Crate Layout

**Parallel:** no
**Blocked by:** none
**Owned files:** `Cargo.toml`, `crates/memorum-eval/Cargo.toml`, `crates/memorum-eval/src/lib.rs`, `crates/memorum-eval/src/main.rs`

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/memorum-eval/Cargo.toml`
- Create: `crates/memorum-eval/src/lib.rs`
- Create: `crates/memorum-eval/src/main.rs`
- Create: `crates/memorum-eval/src/orchestrator.rs` (stub)
- Create: `crates/memorum-eval/src/simulator.rs` (stub)
- Create: `crates/memorum-eval/src/harness_runner.rs` (stub)
- Create: `crates/memorum-eval/src/daemon_scaffold.rs` (stub)
- Create: `crates/memorum-eval/src/assertions.rs` (stub)
- Create: `crates/memorum-eval/fixtures/policies/.gitkeep`
- Create: `crates/memorum-eval/fixtures/trees/.gitkeep`
- Create: `crates/memorum-eval/fixtures/prompts/.gitkeep`
- Create: `crates/memorum-eval/tests/eval/regression/.gitkeep`
- Test: `crates/memorum-eval/tests/crate_compiles.rs`

**Step 1: Write the crate compile test**
Create a test that imports the top-level `memorum_eval` re-exports and asserts the binary name resolves. This immediately fails because the crate does not exist.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test crate_compiles`
Expected: fail — crate not in workspace.

**Step 3: Add the crate and stubs**
Add `memorum-eval` to the workspace. Implement `main.rs` with a `clap` parser stub (just `--version` for now). Create stub modules with `todo!()` public surfaces. Create fixture directory skeleton.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test crate_compiles`
Expected: pass.

---

### Task 2: DaemonScaffold — Isolated memoryd Per Test

**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memorum-eval/src/daemon_scaffold.rs`, `crates/memorum-eval/tests/daemon_scaffold_smoke.rs`

**Files:**
- Modify: `crates/memorum-eval/src/daemon_scaffold.rs`
- Test: `crates/memorum-eval/tests/daemon_scaffold_smoke.rs`

**Step 1: Write the scaffold smoke test**
Create a test that calls `DaemonScaffold::fresh().await`, confirms the temp tree exists and the socket path is non-empty, then drops the scaffold and asserts the process has exited. Use a unique ULID socket path. Assert `doctor()` returns a healthy report.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test daemon_scaffold_smoke`
Expected: fail — `DaemonScaffold` is a stub.

**Step 3: Implement DaemonScaffold**
Spawn `memoryd serve --memory-dir <tmpdir> --socket <tmpdir>/memoryd.sock` as a child process using `tokio::process::Command`. Wait for the socket to appear (poll up to 5 s, 100 ms interval). Implement `Drop` to send SIGTERM and wait for clean exit. Implement `from_fixture(name)` to extract a `.tar.zst` archive from `fixtures/trees/` into a temp dir before spawning. Implement `doctor()` to send a `RequestPayload::Doctor` over the socket and return the `DoctorReport`. Use unique ULID in socket path per §4.4 of the spec.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test daemon_scaffold_smoke`
Expected: pass.

---

### Task 3: DaemonScaffold::two_device — Shared Git Remote

**Parallel:** yes (independent of Task 4)
**Blocked by:** Task 2
**Owned files:** `crates/memorum-eval/src/daemon_scaffold.rs`, `crates/memorum-eval/tests/two_device_scaffold_smoke.rs`

**Files:**
- Modify: `crates/memorum-eval/src/daemon_scaffold.rs`
- Test: `crates/memorum-eval/tests/two_device_scaffold_smoke.rs`

**Step 1: Write the two-device scaffold test**
Test calls `DaemonScaffold::two_device(remote_path)`, receives a `TwoDeviceScaffold { device_a, device_b, remote_path }`. Both scaffolds start healthy (two `doctor()` calls succeed). Both see the same git remote. Assert that a commit on Device A's bare tree is visible to Device B after `git pull`.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test two_device_scaffold_smoke`
Expected: fail — `two_device` does not exist.

**Step 3: Implement two_device**
Initialize a bare git repo at a temp path as the shared remote. Clone it into two separate temp trees (Device A, Device B). Start a `DaemonScaffold` against each tree, configured with the bare repo as the git remote. Expose `TwoDeviceScaffold` with both scaffold handles and a `remote_path` accessor.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test two_device_scaffold_smoke`
Expected: pass.

---

### Task 4: SimulatorAgent and Base Action Vocabulary

**Parallel:** yes (independent of Task 3)
**Blocked by:** Task 2
**Owned files:** `crates/memorum-eval/src/simulator.rs`, `crates/memorum-eval/tests/simulator_agent_smoke.rs`

**Files:**
- Modify: `crates/memorum-eval/src/simulator.rs`
- Test: `crates/memorum-eval/tests/simulator_agent_smoke.rs`

**Step 1: Write the simulator smoke test**
Spin up a `DaemonScaffold::fresh()`. Create a `SimulatorAgent` pointed at the scaffold's socket. Run a script: `[Startup, Search { query: "test" }, Write { body: "hello eval world", meta: GovernanceMeta { confidence: 0.95, source_kind: "agent_primary", source_ref: Some("eval_test_1") } }, Assert { condition: last_write_status_is_not_refused }]`. Assert `observations.last_write_outcome` is not `refused`.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test simulator_agent_smoke`
Expected: fail — `SimulatorAgent` is a stub.

**Step 3: Implement SimulatorAgent and base actions**
Implement `SimulatorAgent` using `memoryd`'s existing `client.rs` (`Client::connect(socket_path)`). Implement the base `SimulatorAction` variants: `Startup`, `Search`, `Write`, `Supersede`, `Forget`, `Get`, `Reveal`, `Assert`, `NewSession`. `run_script` executes each action in order, populating `SimulatorObservations` (last write outcome, last search results, last startup block, etc.). `Assert` evaluates an `AssertionSpec` against `SimulatorObservations` and panics with a descriptive message on failure.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test simulator_agent_smoke`
Expected: pass.

---

### Task 5: test-utils Feature on memoryd and TestInjectEvent

**Parallel:** no
**Blocked by:** Task 4
**Cross-plan dependency:** Stream G plan Task 2 ships `EventKind::RecallHit` and the four other new variants (`RealityCheckConfirmed`, `RealityCheckForgotten`, `RealityCheckNotRelevant`, `ClaimLockContention`) plus the `events_log` SQLite mirror table. Stream H Task 5 references `EventKind::RecallHit` in the simulator's `InjectableEventKind` mapping. **Until Stream G has shipped to trunk, Stream H Task 5 maps `InjectableEventKind::RecallHit` to a placeholder via a stub function `try_construct_recall_hit() -> Option<EventKind>` that uses `cfg(feature = "stream-g-events")` (added below) to emit the real variant when available; without the feature, the inject path returns a runtime "Stream G events not yet shipped" skip.** This avoids the compile-time gap the plan-reviewer caught.

**Owned files:** `crates/memoryd/Cargo.toml`, `crates/memoryd/src/protocol.rs`, `crates/memoryd/src/handlers.rs`, `crates/memoryd/src/mcp.rs`, `crates/memorum-eval/Cargo.toml`, `crates/memorum-eval/src/simulator.rs`, `crates/memoryd/tests/test_utils_release_rejection.rs`, `crates/memorum-eval/tests/inject_event_smoke.rs`

**Files:**
- Modify: `crates/memoryd/Cargo.toml` — add `[features] test-utils = []` and `stream-g-events = []`. Note this collides with Stream G's plan Task 8 (which adds `reqwest`/`lettre` deps); see "Inter-stream coordination" near the top of this plan for the rebase rule (Stream G ships first; Stream H rebases its Cargo.toml additions on top).
- Modify: `crates/memoryd/src/protocol.rs` — add `RequestPayload::TestInjectEvent` behind `#[cfg(feature = "test-utils")]`. The `MethodNotAllowedOnMcp` error variant is added by Stream G plan Task 5; this task reuses it.
- Modify: `crates/memoryd/src/handlers.rs` — add `handle_test_inject_event` behind `#[cfg(feature = "test-utils")]` that appends a synthetic event to the events log using `events::log::append`. The `EventKind::RecallHit` construction is gated `#[cfg(feature = "stream-g-events")]`; without the feature, the handler returns a structured "stream-g-events feature disabled" error.
- Modify: `crates/memoryd/src/mcp.rs` — `RequestPayload::TestInjectEvent(_)` returns `MethodNotAllowedOnMcp` in the rejected match arm.
- Modify: `crates/memorum-eval/Cargo.toml` — declare `memoryd` as `{ path = "../memoryd", features = ["test-utils", "stream-g-events"] }` for `[dev-dependencies]`. Production callers should never depend on the test-utils feature.
- Modify: `crates/memorum-eval/src/simulator.rs` — add `InjectEventLogEntry` variant to `SimulatorAction` per spec §4.2 (post-patch).
- Test: `crates/memoryd/tests/test_utils_release_rejection.rs` — release-build feature-absence assertion (see Step 5).
- Test: `crates/memorum-eval/tests/inject_event_smoke.rs`.

**Step 1: Write the inject-event smoke test**
Spin up a daemon built with `test-utils + stream-g-events` features. Write a memory, record its id. Run `SimulatorAction::InjectEventLogEntry { kind: InjectableEventKind::RecallHit, memory_id: <id>, ts: Utc::now(), harness: None, session_id: None }`. Assert the daemon responds successfully (no `MethodNotAllowed`). Then verify via a `Status` call that the events log has grown.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test inject_event_smoke`
Expected: fail — `TestInjectEvent` variant does not exist.

**Step 3: Implement the test-utils feature and InjectEventLogEntry**
Wire up the modifications listed above. In production builds (no `test-utils` feature, no `stream-g-events` feature), the `TestInjectEvent` arm returns `ResponsePayload::Error(MethodNotAllowedOnMcp)`. The simulator client sends `TestInjectEvent` over the socket.

**Step 4: Run the test to verify it passes (test-utils feature present)**
Run: `cargo test -p memorum-eval --test inject_event_smoke`
Expected: pass.

**Step 5: Add release-build feature-absence regression test**
Create `crates/memoryd/tests/test_utils_release_rejection.rs`:
- Compile a daemon binary at the workspace level with `--release --no-default-features` (no `test-utils`).
- Send a `TestInjectEvent` request to the release-built daemon.
- Assert the response is `Error(MethodNotAllowedOnMcp)`, not a panic, not a successful injection.

This regression test prevents accidentally enabling `test-utils` on shipped binaries. Workspace gate (`scripts/check.sh`) must include this.

**Step 6: Run release-rejection test**
Run: `cargo test -p memoryd --test test_utils_release_rejection --release --no-default-features`
Expected: pass.

---

### Task 6: HarnessRunner Core — CLI Detection and MCP Config Injection

**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memorum-eval/src/harness_runner.rs`, `crates/memorum-eval/tests/harness_runner_detection.rs`

**Files:**
- Modify: `crates/memorum-eval/src/harness_runner.rs`
- Test: `crates/memorum-eval/tests/harness_runner_detection.rs`

**Step 1: Write the CLI detection test**
Test calls `HarnessRunner::detect_cli(RealHarness::Claude)` and `HarnessRunner::detect_cli(RealHarness::Codex)`. If neither CLI is in `$PATH`, the test passes by asserting `detect_cli` returns `Ok(None)` rather than panicking. If one is present, assert `detect_cli` returns `Ok(Some(HarnessCli { path, mcp_config_flag }))` and that `mcp_config_flag` was validated against `--help` output. Assert no temp files are left behind.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test harness_runner_detection`
Expected: fail — `HarnessRunner` is a stub.

**Step 3: Implement HarnessRunner core**
Implement `detect_cli`: run `which claude` / `which codex` to find the binary. Then run `<bin> --help` and grep for `--mcp-config` in stdout; if found, record the flag name. If the flag is absent, return `Err(HarnessIncompatibleCli { reason })`. Implement `HarnessRunner::new(harness, socket_path)`. Implement `write_mcp_config_file(sandbox_dir, run_id)` which writes a per-invocation temp file in the format specified in spec §3.2 (Claude: JSON `mcpServers`; Codex: TOML `[mcp.<name>]`). Prompts are passed via stdin exclusively per spec §5.1. Implement `run(prompt_template, env, timeout)` as a skeleton returning `HarnessRunResult { stdout, stderr, exit_code, duration }`.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test harness_runner_detection`
Expected: pass.

---

### Task 7: MockHarness

**Parallel:** yes (independent of Task 6 runtime, depends only on Task 4)
**Blocked by:** Task 4
**Owned files:** `crates/memorum-eval/src/harness_runner.rs`, `crates/memorum-eval/tests/mock_harness_smoke.rs`

**Files:**
- Modify: `crates/memorum-eval/src/harness_runner.rs`
- Test: `crates/memorum-eval/tests/mock_harness_smoke.rs`

**Step 1: Write the MockHarness smoke test**
Spin up a `DaemonScaffold::fresh()`. Create a `MockHarness` for test #13 behavior (observe-then-recall). Assert it calls `memory_observe` directly via the daemon protocol, then `memory_startup` and `memory_search`, and returns a synthesized JSON object `{ found: true, fragment_text: "<…>" }`. Assert `"mode: mock"` annotation is present in the output metadata.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test mock_harness_smoke`
Expected: fail — `MockHarness` does not exist.

**Step 3: Implement MockHarness**
Implement `MockHarness` as a struct with a `run_test(test_id: u8, scaffold: &DaemonScaffold)` method. For test #13: directly call `memory_observe` over the daemon socket (as the Codex phase), then call `memory_startup` and `memory_search` (as the Claude phase), synthesize the JSON output the test expects. For test #15: call `memory_write` with PII, observe the refusal, call `memory_write` again with PII stripped, synthesize `{ first_attempt_status, retry_status, retry_id }`. For test #19: **the implementation must be `#[cfg(feature = "stream-i-deps")]`-gated** — when the feature is off (the default until Stream I lands), `run_test(19, _)` returns `TestOutcome::Skipped { reason: "stream-i-deps feature disabled — peer-update framing requires `memorum-coordination::framing_tests::assert_framing`" }`; when the feature is on, the gated path constructs a `<memory-delta>` containing a synthetic `<peer-update>` and runs `assert_framing` on it. Without the cfg gate, `mock_harness_smoke.rs` fails to compile in default-features mode because `assert_framing` is not visible. The `#[cfg(...)]` gate must be applied at the function body level (a `match test_id { 19 => { ... } }` arm guarded by `cfg`), not at the use-statement level — the import is also gated. Emit `mode: mock` in all outputs per spec §7.4.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test mock_harness_smoke`
Expected: pass.

---

### Task 8: Assertions Helpers and XML Parser

**Parallel:** yes (independent of other tasks, depends on Task 1)
**Blocked by:** Task 1
**Owned files:** `crates/memorum-eval/src/assertions.rs`, `crates/memorum-eval/tests/assertions_unit.rs`

**Files:**
- Modify: `crates/memorum-eval/src/assertions.rs`
- Test: `crates/memorum-eval/tests/assertions_unit.rs`

**Step 1: Write assertion unit tests**
Test `parse_recall_block(xml_str)` returns a structured `RecallBlock { memories: Vec<RecallMemory>, omitted_count, pending_attention_items }`. Test `assert_memory_in_recall(block, ref_id)` succeeds when the ref is present, panics with a descriptive message when absent. Test `assert_no_memory_in_recall(block, ref_id)` likewise. Test `assert_xml_valid(xml_str)` on a well-formed block and an intentionally malformed one.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test assertions_unit`
Expected: fail — `assertions.rs` is a stub.

**Step 3: Implement assertions helpers**
Implement `parse_recall_block` using a minimal XML parser (the `quick-xml` crate is appropriate; it is likely already available in the workspace via memoryd). Implement `assert_memory_in_recall`, `assert_no_memory_in_recall`, `assert_xml_valid`, `assert_status_eq`, `assert_governance_outcome`, and `assert_no_pii_on_disk(tree_dir, pii_string)` (walks the temp tree recursively looking for the PII string in any file). All assertion functions return `Result<(), AssertionError>` with a rich `AssertionError` type that includes what was expected, what was found, and which test step generated the failure.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test assertions_unit`
Expected: pass.

---

### Task 9: Stream H Meta-Acceptance Tests (§9)

**Parallel:** no
**Blocked by:** Tasks 2, 4, 7
**Owned files:** `crates/memorum-eval/tests/meta_orchestrator_smoke.rs`, `crates/memorum-eval/tests/meta_simulator_connectivity.rs`, `crates/memorum-eval/tests/meta_dream_path_exclusion.rs`, `crates/memorum-eval/tests/meta_privacy_filter_connectivity.rs`

**Files:**
- Test: `crates/memorum-eval/tests/meta_orchestrator_smoke.rs`
- Test: `crates/memorum-eval/tests/meta_simulator_connectivity.rs`
- Test: `crates/memorum-eval/tests/meta_dream_path_exclusion.rs`
- Test: `crates/memorum-eval/tests/meta_privacy_filter_connectivity.rs`

**Step 1: Write meta-acceptance tests**
Write four tests matching spec §9:
1. `meta_orchestrator_smoke`: spawn `memorum-eval --list`, assert exit 0 and output contains exactly 19 lines starting with `#` (test entries).
2. `meta_simulator_connectivity`: spin up `DaemonScaffold::fresh()`, `SimulatorAgent::new(socket)`, call `Startup`, assert `ResponsePayload::Startup` returns.
3. `meta_dream_path_exclusion`: spin up scaffold, write a file to `dreams/journal/me/<today>.md` directly in the temp tree with no frontmatter, call `memory_search` with its unique text, assert zero results; also assert `Substrate::read_memory_envelope` on the path returns `ReadError::NotACanonicalMemory`.
4. `meta_privacy_filter_connectivity`: spin up scaffold, attempt `memory_write` with body containing `4111111111111111` (Luhn-valid Visa test number), assert response `status == "refused"` and `reason` contains `"privacy"` or `"policy"`.

**Step 2: Run the tests to verify they fail**
Run: `cargo test -p memorum-eval --test meta_orchestrator_smoke` (and the other three)
Expected: fail — orchestrator `--list` not wired, simulator/scaffold integration incomplete.

**Step 3: Wire orchestrator --list and fix any integration gaps**
Implement `memorum-eval --list` to enumerate the test catalog from a static `TEST_CATALOG` constant (19 entries: number, name, mode, group). No logic executed; just print and exit 0. Fix any gaps found in steps 1–2 (scaffold startup timing, socket connect retry, etc.).

**Step 4: Run the tests to verify they pass**
Run: `cargo test -p memorum-eval` filtering to the four meta tests.
Expected: all pass.

---

### Task 10: Handbook Tests #1–#6 (Simulator-Driven, Simple)

**Parallel:** no
**Blocked by:** Tasks 4, 8
**Owned files:**
- `crates/memorum-eval/tests/eval/handbook/t01_exact_identifier_recall.rs`
- `crates/memorum-eval/tests/eval/handbook/t02_superseded_fact.rs`
- `crates/memorum-eval/tests/eval/handbook/t03_cross_project_entity_collision.rs`
- `crates/memorum-eval/tests/eval/handbook/t04_abstention.rs`
- `crates/memorum-eval/tests/eval/handbook/t05_poisoned_candidate.rs`
- `crates/memorum-eval/tests/eval/handbook/t06_tool_output_preservation.rs`

**Files:**
- Create: six test files above per spec §3.1

**Step 1: Write the six test functions**
Implement each test as a `#[tokio::test]` calling `DaemonScaffold::fresh()`, constructing a `SimulatorAgent` with the script described in spec §3.1 for that test number, running the script, and asserting all assertions in the spec's Assertions list. Use the `assertions.rs` helpers from Task 8.

Key assertions per test:
- **#1**: recall block contains sentinel `EVAL_SENTINEL_XF7Q9`; `startup_total >= 1`.
- **#2**: search ranks PostgreSQL 16 above 14; old memory is `superseded`; no old body in recall.
- **#3**: two project namespaces; JWT memory stays in proj_alpha recall; session-cookie stays in proj_beta recall.
- **#4**: search and recall return zero results for `EVAL_NOVEL_TOPIC_ZK8T`; response is success not error.
- **#5**: poisoned write returns `quarantined` or `refused`; not in default search results; visible with `trust_level: quarantined` filter.
- **#6**: artifact type promoted; `artifact://` handle preserved verbatim in body.

**Step 2: Run to verify RED**
Run: `cargo test -p memorum-eval --test t01_exact_identifier_recall` (repeat for each)
Expected: fail — test bodies not yet linked into binary.

**Step 3: Register tests and fix any assertion failures**
Add each test module to the test binary via `mod` declarations in the test driver. Run each test individually. Fix any discrepancies between expected daemon behavior and actual — these are real correctness checks.

**Step 4: Run to verify GREEN**
Run: `cargo test -p memorum-eval` filtering to tests #1–#6.
Expected: all pass.

---

### Task 11: Handbook Tests #7–#12 (Simulator-Driven, More Complex)

**Parallel:** no
**Blocked by:** Task 10
**Owned files:**
- `crates/memorum-eval/tests/eval/handbook/t07_subagent_writeback.rs`
- `crates/memorum-eval/tests/eval/handbook/t08_deletion_and_tombstone.rs`
- `crates/memorum-eval/tests/eval/handbook/t09_recall_budget_pressure.rs`
- `crates/memorum-eval/tests/eval/handbook/t10_compaction_resumption.rs`
- `crates/memorum-eval/tests/eval/handbook/t11_self_poisoning.rs`
- `crates/memorum-eval/tests/eval/handbook/t12_temporal_validity.rs`

**Files:**
- Create: six test files above per spec §3.1

**Step 1: Write the six test functions**
Key assertions per test:
- **#7**: subagent write preserved (attribution `source_kind: "subagent"` intact); parent delta-block includes it; not refused on grounding grounds when valid spawner session ref present.
- **#8**: `memory_forget` returns `tombstoned`; not in recall or default search; re-insertion refused with `reason: "tombstone"`; `tombstones/` file exists in temp tree.
- **#9**: 40 memories written; gold memory appears in recall block despite budget trimming; `RecallExplanation.omitted_count > 0`.
- **#10**: two simulated compaction events; recall block after each includes expected subset of pre-compaction memories; no duplicates; all statuses remain `active`.
- **#11**: incorrect candidate not in factual recall; self-referencing confidence escalation refused or requires human review; correct supersession promoted.
- **#12**: expired memory (`valid_until` past) absent from recall; fresh memory present; future-valid memory (`valid_from` future) absent from recall.

**Step 2: Run to verify RED**
Run each test independently.
Expected: fail.

**Step 3: Implement and fix discrepancies**
Register modules. Run and fix any behavioral gaps found.

**Step 4: Run to verify GREEN**
Run: `cargo test -p memorum-eval` filtering to tests #7–#12.
Expected: all pass.

---

### Task 12: Tests #14 and #17 — Two-Device Tests (Serial Group)

**Parallel:** no
**Blocked by:** Tasks 3, 4, 8
**Owned files:**
- `crates/memorum-eval/tests/eval/domain/t14_merge_driver_semantic_correctness.rs`
- `crates/memorum-eval/tests/eval/domain/t17_lease_contention_resolution.rs`

**Files:**
- Create: two test files above per spec §3.2

**Step 1: Write the two test functions**

**Test #14 setup and assertions:**
Use `DaemonScaffold::two_device(remote_path)`. Device A and Device B each supersede the same shared memory — A updates `confidence` to 0.92 and adds `ent_merge_test_alpha`; B updates `summary` and adds `ent_merge_test_beta`. Invoke the `memory-merge-driver` binary directly (via `Command`) with base/ours/theirs as per its merge driver protocol. Assert:
- Exit 0.
- Merged `entities` contains both `ent_merge_test_alpha` and `ent_merge_test_beta`.
- `confidence = 0.92`.
- `updated_at` not earlier than Device A's write timestamp.
- `memoryd doctor` on the merged temp tree returns zero validation errors.

**Test #17 setup and assertions:**
Use `TwoDeviceScaffold`. Pre-seed `leases/journal.lease` in the shared bare repo attributed to `device_id_a` with TTL `now + 60s`. Both devices `git pull`. Device B sends `DreamNow { scope: "me", force: false }`; assert response has `error_code: "lease_unavailable"` and no journal file appears. Device A sends the same; assert `pass_1.status: "success"`. After Device A releases, Device B retries; assert success. Assert `dreams/journal/me/<today_date>.md` exists only in Device A's tree.

**Step 2: Run to verify RED**
Run: `cargo test -p memorum-eval --test t14_merge_driver_semantic_correctness`
Run: `cargo test -p memorum-eval --test t17_lease_contention_resolution`
Expected: fail.

**Step 3: Implement and fix discrepancies**
Register modules. Fix timing issues (both tests involve multiple daemon instances and git operations; use generous timeouts with `tokio::time::timeout`).

**Step 4: Run to verify GREEN**
Run: `cargo test -p memorum-eval` filtering to tests #14 and #17.
Expected: pass.

---

### Task 13: Test #16 — Drift Scoring Sanity (Depends on Stream G §5.7)

**Parallel:** no
**Blocked by:** Tasks 5, 8
**Owned files:**
- `crates/memorum-eval/tests/eval/domain/t16_drift_scoring_sanity.rs`

**Files:**
- Create: `crates/memorum-eval/tests/eval/domain/t16_drift_scoring_sanity.rs` per spec §3.2

**Step 1: Write the test function**
This test depends on two Stream G additions (spec §1.3 #1–#2): `EventKind::RecallHit` variant and the covering index `events_log(kind, memory_id, ts)`. It also depends on `RequestPayload::RealityCheckRequest::List` and `RealityCheckResponse::Pending { items }` from Stream G §5.7, and on `ComponentScores` wire shape.

Write three memories (A: fresh, recalled 30×, two sources; B: stale 95d, zero recalls, one source, sensitive; C: 30d, recalled 5×, one source). Inject events-log rows via `SimulatorAction::InjectEventLogEntry` for Memory A (30 `RecallHit` entries plus one extra `WriteCommitted`) and Memory C (5 `RecallHit` entries). Call `memoryd doctor --reindex`. Send `RealityCheckRequest::List { namespace: None, limit: Some(12) }` over the daemon socket. Parse `RealityCheckResponse::Pending { items }`.

Assertions (per spec §3.2):
- Strict ordering: `score(B) > score(C) > score(A)`.
- `score(A) ≤ 0.25`; `score(B) ≥ 0.65`; `score(C)` in `(0.25, 0.65)`.
- Each item's `component_scores` exposes the five fields per Stream G §5.7 `ComponentScores` contract.
- Reconstruct weighted sum from components; assert equals reported `score` within `1e-9`.

**Step 2: Run to verify RED**
Run: `cargo test -p memorum-eval --test t16_drift_scoring_sanity`
Expected: fail — depends on Stream G's RC protocol implementation. If Stream G is not yet shipped, the test will fail at the `RealityCheckRequest::List` send step. Mark with a `#[ignore = "waiting: stream-g-rc-protocol"]` guard on Step 3 build so CI marks it skipped rather than failing the build.

**Step 3: Implement test; add Stream G guard**
Register the module. Add a runtime guard: if the daemon returns `MethodNotAllowed` or `UnknownVariant` for `RealityCheckRequest::List`, skip with a clear message `"SKIP: Stream G Reality Check protocol not yet shipped."` This avoids failing tests #1–#15 and #17–#19 over a dependency not yet in.

**Step 4: Run to verify GREEN (when Stream G ships)**
Remove the skip guard once Stream G's RC protocol lands. Run: `cargo test -p memorum-eval --test t16_drift_scoring_sanity`.
Expected: pass.

---

### Task 14: Test #18 — Key Rotation (Depends on Stream D Rotation Contract)

**Parallel:** no
**Blocked by:** Tasks 2, 4, 8
**External dependency:** spec §3.2 test #18's preamble describes a rotation contract (atomic active-key swap, decommissioned key directory under `~/.memoryd/keys/decommissioned/`, no bulk re-encryption, forward-secrecy property, `EventKind::DeviceKeysRotated` audit event). **The shipped `crates/memory-privacy/src/keys.rs::FileKeyProvider::onboard_local_file` overwrites the active key with no decommissioning concept — none of the contract is implemented today.** The contract is a Stream D v0.1.1 amendment that has not landed yet. Stream H test #18 implements the *test*, not the contract.

**Owned files:**
- `crates/memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs`

**Files:**
- Create: `crates/memorum-eval/tests/eval/domain/t18_encrypted_tier_key_rotation.rs` per spec §3.2

**Step 1: Write the test function (with semantic skip-guard, not just CLI-existence guard)**

The rotation contract (spec §3.2 preamble) specifies: atomic active-key swap, decommissioned key directory, no bulk re-encryption, forward secrecy.

Setup: temp tree with privacy filter configured with a test-scoped age key at a temp path. Write a memory with a private email address; confirm it lands under `encrypted/` in the tree. Rotate the key via `memoryd device rotate-keys`. Read the original memory via `memory_reveal`; assert decrypts to original body. Write a new PII memory using the new active key. Step 5: attempt to decrypt the new ciphertext using the old (decommissioned) key directly using the `age` crate — assert decryption fails. Step 6: `memory_reveal` with the current key succeeds.

Assertions per spec:
- After rotation: key provider's active key fingerprint differs from pre-rotation key.
- Old memory readable via `memory_reveal`.
- Old key cannot decrypt new content (forward secrecy).
- Both reveals are recorded as `EventKind::EncryptedContentRevealed` events.
- Rotation appends `EventKind::DeviceKeysRotated` to the events log.

**Semantic skip-guard (not just CLI existence):** the previous draft of this plan's skip-guard checked only whether `memoryd device rotate-keys` was wired as a CLI subcommand. That subcommand is wired today (it calls `onboard_local_file`), but it does NOT implement the rotation contract — there is no decommissioned directory, no `active.json` manifest, no multi-key fallback decryption. A CLI-existence guard would let the test proceed past `rotate-keys` and then fail silently at step 5 because the decommissioned directory is empty.

The correct guard is **contract-semantic**:

```rust
// At test start, before any setup:
let key_dir = test_runtime_root.join(".memoryd/keys");
let decommissioned_dir = key_dir.join("decommissioned");
let active_manifest = key_dir.join("active.json");
if !decommissioned_dir.exists() || !active_manifest.exists() {
    // Probe the contract by performing a rotation and inspecting state.
    // If after rotation the decommissioned directory still doesn't exist,
    // the rotation contract has not shipped; skip.
    eprintln!("test #18 skipped: Stream D rotation contract (decommissioned dir + active.json manifest) not present");
    return TestOutcome::Skip {
        reason: "STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED".to_string(),
    };
}
```

**Step 2: Run to verify RED (or controlled skip)**
Run: `cargo test -p memorum-eval --test t18_encrypted_tier_key_rotation`
Expected: skip with `STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED` against the shipped daemon (until Stream D v0.1.1 lands). The "RED" assertion at this stage is just that the test compiles and the skip-guard is reached.

**Step 3: Implement test with semantic guard + crypto-layer test helper**
Register module. Implement the contract-semantic skip-guard. For Step 5 (test-internal decryption with old key): access the old key material from the `decommissioned/` directory. Use the `age` crate directly (not the daemon) to attempt decryption. This is a crypto-layer assertion per spec §10.5 — add a test-only function in `memory-privacy/src/crypto.rs` behind `#[cfg(test)]` if needed to expose the `DecryptKeyNotFound` path.

**Step 4: Run to verify GREEN (when Stream D v0.1.1 rotation contract ships)**
Once the rotation contract is shipped (decommissioned directory + active.json manifest + multi-key fallback decrypt + `DeviceKeysRotated` event), the skip-guard returns false and the test runs the full sequence.
Run: `cargo test -p memorum-eval --test t18_encrypted_tier_key_rotation`
Expected: pass.

---

### Task 15: Test #13 — Cross-Harness Substrate Sharing (Real-Harness)

**Parallel:** no
**Blocked by:** Tasks 6, 7
**Owned files:**
- `crates/memorum-eval/tests/eval/domain/t13_cross_harness_substrate_sharing.rs`
- `crates/memorum-eval/fixtures/prompts/t13_codex_observe.md`
- `crates/memorum-eval/fixtures/prompts/t13_claude_recall.md`

**Files:**
- Create: test file and two prompt templates per spec §3.2

**Step 1: Write the test function and prompt templates**
Prompt templates: per spec §3.2's HarnessRunner MCP config injection table. `t13_codex_observe.md` instructs Codex to call `memory_observe` with sentinel `EVAL_T13` and entity `ent_eval_t13_xk9m`. `t13_claude_recall.md` instructs Claude to call `memory_startup` and `memory_search` for entity `ent_eval_t13_xk9m` and output `{found: bool, fragment_text: string|null}` to stdout.

Test function: check if both `claude` and `codex` CLIs are available; if not, skip with `SKIP_NO_AUTH`. Otherwise:
1. Build sandbox temp tree, start `DaemonScaffold::fresh()`.
2. Write per-invocation MCP config files via `HarnessRunner::write_mcp_config_file`.
3. Invoke `codex exec` via `HarnessRunner::run` for the Codex phase.
4. Invoke `claude -p` for the Claude phase.
5. Parse Claude's JSON output. Assert `found: true`. Assert fragment text contains `"EVAL_T13"` or factual content. Assert substrate fragment file under `substrate/<device_id>/` contains `"ent_eval_t13_xk9m"` in the `entities` array. One automatic retry on parse failure.

Check for `MEMORUM_EVAL_CLAUDE_KEY` and `MEMORUM_EVAL_CODEX_KEY` env vars; skip with `SKIP_NO_AUTH` if absent.

**Step 2: Run to verify RED**
Run: `cargo test -p memorum-eval --test t13_cross_harness_substrate_sharing`
Expected: fail (or skip if CLIs not installed).

**Step 3: Implement HarnessRunner::run fully**
Wire `run` in `harness_runner.rs` to spawn the subprocess, write the rendered prompt to stdin, capture stdout/stderr, enforce the per-harness timeout. Register module.

**Step 4: Run to verify GREEN (or SKIP_NO_AUTH on machines without CLIs)**
Run: `cargo test -p memorum-eval --test t13_cross_harness_substrate_sharing`
Expected: pass with CLIs; skip without.

---

### Task 16: Test #15 — Privacy Filter Refusal and Retry (Real-Harness)

**Parallel:** no
**Blocked by:** Tasks 6, 7, 9 (meta_privacy_filter_connectivity must pass first)
**Owned files:**
- `crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs`
- `crates/memorum-eval/fixtures/prompts/t15_privacy_retry.md`

**Files:**
- Create: test file and prompt template per spec §3.2

**Step 1: Write the test function and prompt template**
Prompt template instructs Claude to: (a) call `memory_write` with `"EVAL_T15_PRIVACY_RETRY: The operations contact is reachable at +15550000001."`, (b) observe the refusal, (c) retry with PII removed, (d) output `{first_attempt_status, retry_status, retry_id}` to stdout.

Test function: skip if `MEMORUM_EVAL_CLAUDE_KEY` absent. Invoke Claude via `HarnessRunner::run`. Parse JSON output. Assert `first_attempt_status` contains `"refused"`. Assert `retry_status == "promoted"` or `"candidate"`. Assert `retry_id` is non-null. Call `memory_search` for `"EVAL_T15_PRIVACY_RETRY"`; assert result found. Call `assert_no_pii_on_disk(tree_dir, "15550000001")`; assert no file in the temp tree contains the PII string.

**Step 2: Run to verify RED**
Expected: fail (or skip without Claude).

**Step 3: Wire full harness invocation and register module**
If `AGENT_DID_NOT_RETRY` failure occurs, record as a test failure kind with clear diagnostic output per spec §3.2 non-determinism handling.

**Step 4: Run to verify GREEN (or SKIP_NO_AUTH)**
Expected: pass with Claude; skip without.

---

### Task 17: Test #19 — Peer-Update Framing Correctness (Real-Harness, Stream I)

**Parallel:** no
**Blocked by:** Tasks 6, 7
**Group placement:** **serial** (per spec §6.3 + §10.1, harmonized; an earlier draft of the spec contradicted itself on this — fixed). Inner concurrency: `--max-concurrent 4` per harness. The test slot itself runs alone in the orchestrator's worker pool because most of its wall-clock is LLM-I/O wait.
**Owned files:**
- `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs` (test runner — Stream H owns)

**Files NOT owned by Stream H** (consumed read-only):
- `crates/memorum-eval/fixtures/prompts/t19_peer_update_framing.md` — **Stream I plan Task 20 owns and authors** this prompt template per Stream I §10.4. Stream H consumes the file from disk; it does not author or modify it. Cross-plan note: until Stream I ships Task 20, the file does not exist and test #19 skips with reason `STREAM_I_FIXTURE_NOT_PRESENT`.
- `crates/memorum-coordination/src/framing_tests.rs` — Stream I-owned crate exposing `assert_framing(response: &str) -> FramingOutcome`.

**Files:**
- Create: `crates/memorum-eval/tests/eval/regression/t19_peer_update_framing.rs`

**Compile-time dependency on Stream I (must not break Stream H build before Stream I ships):**

Stream H's `Cargo.toml` declares `memorum-coordination` as an **optional** dependency behind a `stream-i-deps` cargo feature:

```toml
[dependencies]
memorum-coordination = { path = "../memorum-coordination", optional = true }

[features]
stream-i-deps = ["dep:memorum-coordination"]
```

Test #19's source file is gated `#[cfg(feature = "stream-i-deps")]` for the actual `assert_framing` call. When the feature is off (the default until Stream I lands), the test compiles to a stub that emits status `SKIP: stream-i-deps feature disabled` and exits 0. The orchestrator's `--list` includes the test entry regardless of the feature; the per-case execution simply skips. **This avoids the compile-time gap the plan-reviewer caught**: previously the plan declared a runtime skip-guard for an unconditional import, which would have failed to compile when `memorum-coordination` was absent.

**Step 1: Write the test function**
Per Stream I §10.4 sampling matrix: 6 temperature/harness combinations (`claude` at `[0.0, 0.5, 1.0]` and `codex` at `[0.0, 0.5, 1.0]`), each run 3 times for stochastic-noise tolerance, majority-vote per case. The prompt template authored by Stream I instructs the agent to respond to `"What should I do next given what you know?"` with a synthetic `<memory-delta>` block containing one `<peer-update>` and one normal recall item injected as session context.

When the `stream-i-deps` feature is enabled, the test reads the prompt template (skip with `STREAM_I_FIXTURE_NOT_PRESENT` if missing), invokes the harness, and calls `memorum_coordination::framing_tests::assert_framing` on each response. Pass criterion per spec §10.1: ≥5 of 6 framing-correct outcomes per harness (≥10 of 12 total). Report per-case `{harness, temperature, run, framing_correct}` in `details`. Fail with `framing_correct: <N>/12` on fewer than 10/12.

Skip with `SKIP_NO_AUTH` if CLIs absent; mark run `partial: true`.

**Step 2: Run to verify RED**
Expected: skips with `SKIP: stream-i-deps feature disabled` when feature is off (the normal local-dev case before Stream I ships); compiles cleanly. The "RED" assertion at this stage is just that the file compiles and the orchestrator picks it up — substantive RED comes from running `cargo test -p memorum-eval --features stream-i-deps -- --test t19_peer_update_framing` once Stream I has landed Task 20.

**Step 3: Implement test under feature gate**
Stub when feature off; full runner when feature on. Inner concurrency `--max-concurrent 4` per harness per spec §10.1.

**Step 4: Run to verify GREEN (post-Stream-I)**
After Stream I ships, run `cargo test -p memorum-eval --features stream-i-deps -- --test t19_peer_update_framing`. Expected: pass when CLIs are present; skip with `SKIP_NO_AUTH` otherwise.

---

### Task 18: Orchestrator Binary — Full CLI and Parallel/Serial Grouping

**Parallel:** no
**Blocked by:** Tasks 9–17 (all tests registered)
**Owned files:** `crates/memorum-eval/src/main.rs`, `crates/memorum-eval/src/orchestrator.rs`

**Files:**
- Modify: `crates/memorum-eval/src/main.rs`
- Modify: `crates/memorum-eval/src/orchestrator.rs`
- Test: `crates/memorum-eval/tests/orchestrator_integration.rs`

**Step 1: Write the orchestrator integration test**
Test invokes `memorum-eval --list` and asserts 19 entries. Test invokes `memorum-eval --filter "t01" --output json` and asserts the JSON output contains `total: 1`, `tests[0].number: 1`, `tests[0].status` is `"passed"` or `"failed"` (not absent). Test invokes `memorum-eval --harness mock --output json` and asserts `partial: true` (real-harness tests skip) and `failed == 0` — matching spec §7.2's `.failed == 0` pass criterion.

**Step 2: Run to verify RED**
Run: `cargo test -p memorum-eval --test orchestrator_integration`
Expected: fail — orchestrator not yet wired beyond `--list`.

**Step 3: Implement the full orchestrator**
Wire up `--harness`, `--filter`, `--output`, `--output-file`, `--timeout`, `--workers`, `--no-cleanup`, `--list`, `--verbose` flags per spec §6.1. Implement **parallel group** (tests #1–#12, #16; up to `--workers` concurrent, each with its own `DaemonScaffold`) and **serial group** (tests #13, #14, #15, #17, #18, #19; one at a time after parallel group completes — matches spec §6.3 + §10.1 harmonized). Emit JSON output per spec §6.2, including `partial`, `missing_credentials` fields. Exit codes per spec §6.4: 0 = no failures, 1 = failures, 2 = orchestrator error, 3 = timeout. In mock mode, real-harness tests emit `status: "skipped", skip_reason: "SKIP_NO_AUTH"` and count as `partial: true` but do not count as `failed`.

**Step 4: Run to verify GREEN**
Run: `cargo test -p memorum-eval --test orchestrator_integration`
Expected: pass.

---

### Task 19: CI Workflow

**Parallel:** yes (does not run code, just writes YAML)
**Blocked by:** Task 18
**Owned files:** `.github/workflows/stream-h-eval.yml`

**Files:**
- Create: `.github/workflows/stream-h-eval.yml` per spec §7.2

**Step 1: Write a workflow shape test**
Create `crates/memorum-eval/tests/ci_workflow_shape.rs` that reads the YAML file as a string and asserts ALL of:
- The `push.tags` pattern `'v[0-9]+.[0-9]+.[0-9]+-rc.[0-9]+'` is present.
- The `schedule` cron `'0 3 * * *'` is present.
- The gate step contains `jq -r '.failed' "$RESULT_FILE"` (string match), with the comparison `"$FAILED" != "0"`.
- The diagnostic line uses the **correct field names** per spec §6.2: it must contain `.number` and `.failure_detail`. The diagnostic must NOT contain `.test_id` or `.failure_reason` (those names don't exist in the JSON output schema; an earlier draft of this plan used them, fixing it here so the meta-test catches regressions).
- The partial-run step contains `jq -r '.partial // false'` and `jq -r '.missing_credentials // [] | join(", ")'`.

**Step 2: Run the test to verify it fails**
Run: `cargo test -p memorum-eval --test ci_workflow_shape`
Expected: fail — YAML file does not exist.

**Step 3: Write the workflow YAML**
Implement exactly the workflow from spec §7.2 + §7.5, with these field-name correctnesses pinned by the test in Step 1:

- Gate step `Fail if eval did not pass`:
  ```yaml
  - name: Fail if eval did not pass
    run: |
      RESULT_FILE=eval-results-full.json
      [ -f "$RESULT_FILE" ] || RESULT_FILE=eval-results-mock.json
      FAILED=$(jq -r '.failed' "$RESULT_FILE")
      if [ "$FAILED" != "0" ]; then
        echo "Eval harness failed: $FAILED test(s) reported failure"
        jq -r '.tests[] | select(.status == "failed") | "  - #\(.number) \(.name): \(.failure_detail // \"no detail\")"' "$RESULT_FILE"
        exit 1
      fi
  ```
- RC partial-run step (spec §7.5):
  ```yaml
  - name: Fail if RC run is partial
    if: startsWith(github.ref, 'refs/tags/v1.') && contains(github.ref, '-rc.')
    run: |
      RESULT_FILE=eval-results-full.json
      [ -f "$RESULT_FILE" ] || RESULT_FILE=eval-results-mock.json
      PARTIAL=$(jq -r '.partial // false' "$RESULT_FILE")
      if [ "$PARTIAL" = "true" ]; then
        MISSING=$(jq -r '.missing_credentials // [] | join(", ")' "$RESULT_FILE")
        echo "RC eval run is partial — missing credentials: $MISSING"
        echo "RC gates require a full eval (set MEMORUM_EVAL_CLAUDE_KEY and MEMORUM_EVAL_CODEX_KEY)."
        exit 1
      fi
  ```

Include `MEMORUM_EVAL_CLAUDE_KEY` and `MEMORUM_EVAL_CODEX_KEY` secret injection for the real-harness step. Upload artifacts always. Full run on `workflow_dispatch` with `harness_mode` input.

**Step 4: Run the test to verify it passes**
Run: `cargo test -p memorum-eval --test ci_workflow_shape`
Expected: pass.

---

### Task 20: Regression-as-Test Scaffolding and Docs

**Parallel:** yes (independent of CI)
**Blocked by:** Task 9 (directory structure established)
**Owned files:**
- `crates/memorum-eval/tests/eval/regression/.gitkeep`
- `crates/memorum-eval/README.md`
- `docs/api/stream-h-eval-api.md`
- `docs/dev/stream-h-test-catalog.md`

**Files:**
- Confirm: `crates/memorum-eval/tests/eval/regression/.gitkeep` (already created in Task 1)
- Test: `crates/memorum-eval/tests/regression_meta.rs`
- Create: `docs/api/stream-h-eval-api.md`
- Create: `docs/dev/stream-h-test-catalog.md`
- Create: `crates/memorum-eval/README.md`

**Step 1: Write regression meta test**
Test that any file in `crates/memorum-eval/tests/eval/regression/` named `t<NN>_*.rs` contains a `//!` doc-comment block with the required fields: test number, incident date, description, root cause, fix commit. Assert this with a simple regex scan rooted at the cargo `CARGO_MANIFEST_DIR`-relative path `tests/eval/regression/`. **Note:** the prior plan revision said `src/tests/eval/regression/`; that path does not exist after the global move of test files from `src/tests/eval/` to `tests/eval/` (cargo `--test <name>` only resolves files under `tests/`, not `src/tests/`). Scanning `src/tests/eval/regression/` would always pass vacuously because the directory is empty/absent. The test must scan `tests/eval/regression/`. Assert the `regression/` directory is scannable (confirms the path is correct in the compiled output tree); if missing, the test fails with a clear message rather than silently passing.

**Step 2: Run to verify RED**
Expected: fail — no `t*.rs` files in regression yet, and the meta-check for the format is not wired.

**Step 3: Add meta-check and write API/test-catalog docs**
Register the meta-check. Write `docs/api/stream-h-eval-api.md` documenting the `memorum-eval` CLI, JSON output format, exit codes, and all test numbers with their mode and group. Write `docs/dev/stream-h-test-catalog.md` with the full 19-test catalog table (number, name, mode, group, spec section, regression guarded). Write `crates/memorum-eval/README.md` with local run instructions, debug tips, and how to add a regression test.

**Step 4: Run to verify GREEN**
Run: `cargo test -p memorum-eval --test regression_meta`
Expected: pass (zero regression test files is a valid state; the meta-check only validates format of files that exist).

---

### Verification

Per-task gate (run after each task):

```bash
cargo fmt --all -- --check
cargo test -p memorum-eval --test <task_test_name>
```

Intermediate workspace check after Task 5 (the only task that modifies `memoryd`):

```bash
cargo test -p memoryd
cargo clippy -p memoryd -- -D warnings
```

Trunk gate after Stream H integrates on `main`:

```bash
bash scripts/check.sh
```

The trunk gate runs the full workspace, including fuzz targets and the bench regression check. Do not run it inside individual tasks — only once at integration time.

**Dogfood gate note:** The daily eval workflow (`0 3 * * *`) begins firing the day after the CI workflow YAML lands on `main`. If the dogfood week (system-v0.2 §20.3) is active when Stream H lands, any daily run failure extends the dogfood week until a clean day passes (§7.6).
