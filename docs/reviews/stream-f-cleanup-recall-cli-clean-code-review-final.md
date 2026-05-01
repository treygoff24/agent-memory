# Stream F Review Gate D Final: Cleanup, Recall Hook, CLI/Status Clean-Code/Correctness Review

## Verdict

PASS.

No material issues found. No severity-1/2 findings remain for Gate D.

## Intended outcome

This final rerun verifies that the Stream F cleanup/recall/CLI-status/admin protocol slice is clean enough for Gate D after the daemon `DreamNow` lease fix. The specific expected outcome is that daemon-triggered manual dreams now enforce the same lease invariant as the CLI, the prior pending-attention novelty suppression fix remains intact, and dream/admin tools remain excluded from MCP.

## Executive summary

The prior severity-2 daemon `DreamNow` finding is closed. `RequestPayload::DreamNow` now propagates `force`, acquires a manual lease before running the dream pipeline, uses the lease run id for outputs, and maps lease failures into typed protocol errors. The prior novelty-window suppression finding remains fixed and covered by startup recall integration tests. MCP continues to expose only the agent-facing memory tools and explicitly excludes dream/admin tools. Targeted tests, format check, and clippy all passed.

## Confirmations

### Daemon `RequestPayload::DreamNow` acquires/enforces leases and honors `force`

- Evidence: `crates/memoryd/src/protocol.rs:95-99` defines daemon `DreamNow` with `scope`, `force`, and `cli_override` fields.
- Evidence: `crates/memoryd/src/handlers.rs:121-123` dispatches `RequestPayload::DreamNow { scope, force, cli_override }` without discarding `force`.
- Evidence: `crates/memoryd/src/handlers.rs:144-164` parses the dream scope, validates the harness request, and calls `crate::dream::lease::acquire_manual_lease(...)` with the daemon request's `force`, current time, lease window, and `cli_override` before constructing the runner.
- Evidence: `crates/memoryd/src/handlers.rs:166-183` uses `acquired.record.run_id` in `DreamRunOptions`, selects the daemon harness, runs `DreamRunner`, and returns `ResponsePayload::DreamNow`.
- Evidence: `crates/memoryd/src/dream/lease.rs:121-164` enforces manual lease acquisition: validates scope, loads device id, fetches origin, rejects active same-scope leases when `force` is false, rejects dirty trees, appends `leases/journal.lease`, commits, pushes, and rolls back failed push attempts.
- Evidence: `crates/memoryd/src/handlers.rs:2129-2131` maps `LeaseError` to its protocol code (`lease_held`, `lease_unavailable`, `lease_dirty_tree`, or `invalid_request`) with `retryable: false`.
- Test evidence: `crates/memoryd/tests/handler_contract.rs:132-158` seeds an active foreign lease, sends daemon `DreamNow` with `force: false`, asserts `lease_held`, and asserts no dream journal output was written.
- Test evidence: `crates/memoryd/tests/handler_contract.rs:160-182` seeds an active foreign lease, sends daemon `DreamNow` with `force: true`, and asserts a successful dream report.
- Test evidence: `crates/memoryd/tests/handler_contract.rs:184-213` sends daemon `DreamNow`, reads `leases/journal.lease`, asserts the daemon device/scope lease record, asserts the lease commit subject, and then verifies pass output exists.
- Comparison evidence: `crates/memoryd/src/main.rs:320-350` shows the CLI path still uses the same `acquire_manual_lease` helper and lease run id before building `DreamRunOptions`, so the daemon and CLI now enforce the same lease invariant.

### Prior pending-attention novelty suppression finding remains fixed

- Evidence: `crates/memoryd/src/recall/dream_questions.rs:24-27` defines the 7-day recent surfaced-question window and runtime-local hash ring.
- Evidence: `crates/memoryd/src/recall/dream_questions.rs:59-81` loads recent surfaced hashes before candidate collection, applies candidate caps, and records hashes for selected questions.
- Evidence: `crates/memoryd/src/recall/dream_questions.rs:127-131` normalizes/truncates question text, computes the novelty hash, and suppresses candidates already surfaced in the recent window.
- Evidence: `crates/memoryd/src/recall/dream_questions.rs:234-264` stores recent hashes per repo, prunes by window, deduplicates inserts, and bounds the ring to 1,024 entries.
- Test evidence: `crates/memoryd/tests/dream_recall_integration.rs:27-49` surfaces one question, rewrites the same question plus a new one, and asserts the second startup emits only the new question.
- Documentation evidence: `docs/api/stream-e-passive-recall-api.md:137` documents the runtime-local recent surfaced-question hash ring and restart behavior.
- Documentation evidence: `docs/api/stream-e-passive-recall-api.md:151-153` documents 7-day duplicate suppression and the intentionally absent `dream_question_omitted_total` reason for recent-window suppressions.

### MCP still excludes dream/admin tools

- Evidence: `crates/memoryd/src/mcp.rs:212-225` declares exactly nine MCP tools: search, get, write, supersede, forget, reveal, startup, note, and observe. It does not include dream/status/enable/disable admin tools.
- Evidence: `crates/memoryd/src/mcp.rs:227-238` maps only those nine tool names to external MCP names.
- Test evidence: `crates/memoryd/tests/mcp_manifest.rs:26-53` asserts dream/admin tools, including `memory_dream_now`, `memory_dream_status`, `memory_dream_enable`, and `memory_dream_disable`, are absent from the manifest.
- Test evidence: `crates/memoryd/tests/mcp_manifest.rs:20-23` asserts the manifest contains exactly the expected nine tools.

## Findings

No material issues found. No severity-1/2 findings remain for Gate D.

## Non-blocking simplifications

- The daemon and CLI manual-dream paths now correctly share lease acquisition semantics but still duplicate some `DreamRunOptions` construction. A future small extraction could reduce drift, but this is non-blocking and not needed for Gate D.

## Test gaps

No blocking test gaps found for the requested Gate D confirmations. The daemon lease behavior now has direct handler tests for held leases, forced override, and lease-before-output ordering.

## Commands run

- `cargo test -p memoryd --test handler_contract --test dream_recall_integration --test mcp_manifest` — PASS: 9 `dream_recall_integration` tests, 9 `handler_contract` tests, and 10 `mcp_manifest` tests passed.
- `cargo test -p memoryd --test dream_cleanup --test dream_cli --test startup_recall_mcp --test cli_contract` — PASS: 10 `dream_cleanup` tests, 7 `dream_cli` tests, 5 `startup_recall_mcp` tests, and 4 `cli_contract` tests passed.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings` — PASS.

## Questions / uncertainties

- The worktree contains a large existing Stream F dirty set. This review intentionally did not attempt to separate ownership across that whole dirty tree; it focused on the Gate D rerun scope and the prior failing finding.

## Positives

- The daemon fix closes the actual invariant breach instead of only patching the surface symptom: lease acquisition now happens before pass output generation, and `force` is exercised by tests.
- The handler tests now cover the important business cases directly at the daemon protocol boundary.
- MCP admin exclusion remains explicit in both implementation and regression tests.
