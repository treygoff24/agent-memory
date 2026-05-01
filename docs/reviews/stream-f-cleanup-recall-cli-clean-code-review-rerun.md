# Stream F Review Gate D Rerun: Cleanup, Recall Hook, CLI/Status Clean-Code/Correctness Review

## Verdict

FAIL.

The two prior S2 findings are partially/mostly closed:

1. `RequestPayload::DreamNow` no longer returns `not_implemented` for a valid daemon echo request; invalid and unavailable harness cases return typed errors; MCP still excludes dream/admin tools.
2. Pending-attention novelty-window suppression now exists, is tested, and is documented.

However, one severity-2 API/correctness issue remains in Task 14: the daemon `DreamNow` implementation bypasses the manual dream lease path and ignores the request's `force` field. That leaves the daemon/admin protocol capable of running a dream for a scope even when an active lease should reject it, which is the same business-critical concurrency invariant the CLI path enforces.

## Intended outcome

Tasks 12-14 are intended to complete the Stream F cleanup layer, Stream E Pass-3 `<pending-attention>` recall hook, and `memoryd dream` CLI/status/review/toggle/admin protocol surfaces. The product outcome is safe cleanup, bounded dream-question recall, and manual dream triggering that obeys the Stream F lease/election contract while keeping dream admin controls out of MCP.

## Executive summary

The direct prior symptoms are fixed and the scoped cleanup/recall/CLI gates remain green. The recall fix is clean enough for the in-process contract: it keeps a repo-keyed recent surfaced-question hash ring, filters repeated question hashes before cap selection, tests suppression on a second startup, and documents that the ring is runtime-local. The daemon `DreamNow` fix is not yet contract-complete: it runs the dream pipeline with deterministic echo, but it does not call the same lease acquisition path as `memoryd dream now`, does not append/commit a lease record, and drops `force` at dispatch. This is a severity-2 correctness/API-contract issue because daemon callers can bypass the single-active-lease invariant for a scope.

## Confirmations

- Prior finding closed: valid daemon echo request no longer returns `not_implemented`.
  - Evidence: `crates/memoryd/src/handlers.rs:121-123` dispatches `RequestPayload::DreamNow` to `dream_now_response`; `crates/memoryd/src/handlers.rs:144-170` builds `DreamRunOptions`, selects a harness, runs `DreamRunner`, and returns `ResponsePayload::DreamNow`.
  - Test evidence: `crates/memoryd/tests/handler_contract.rs:101-125` sends `RequestPayload::DreamNow { scope: "me", force: false, cli_override: Some("echo") }` and asserts a successful `ResponsePayload::DreamNow` with pass 1 and pass 3 success plus a written journal path.

- Prior finding closed: invalid/unavailable daemon dream cases are typed, not `not_implemented`.
  - Evidence: `crates/memoryd/src/handlers.rs:177-185` returns `invalid_request` for unknown CLI override names and `dream_unavailable` for known-but-unavailable/automatic harness cases.
  - Test evidence: `crates/memoryd/tests/handler_contract.rs:127-160` asserts `invalid_request` for `cli_override: Some("bogus")` and `dream_unavailable` for `cli_override: None`, both non-retryable.

- Prior MCP admin-exclusion invariant remains closed.
  - Evidence: `crates/memoryd/src/mcp.rs:212-224` lists exactly nine MCP tools and includes no dream/admin tool; `crates/memoryd/src/mcp.rs:227-238` maps only those tool names.
  - Test evidence: `crates/memoryd/tests/mcp_manifest.rs:27-53` explicitly asserts `memory_dream_now`, `memory_dream_status`, `memory_dream_enable`, and `memory_dream_disable` are absent from the manifest.

- Prior pending-attention novelty-window finding is closed.
  - Evidence: `crates/memoryd/src/recall/dream_questions.rs:24-27` defines the 7-day recent-window/ring storage; `crates/memoryd/src/recall/dream_questions.rs:59-81` loads recent hashes, passes them into candidate collection, and records selected hashes; `crates/memoryd/src/recall/dream_questions.rs:127-131` suppresses a candidate whose normalized/truncated question hash was recently surfaced; `crates/memoryd/src/recall/dream_questions.rs:234-264` prunes and retains recent hashes by window.
  - Test evidence: `crates/memoryd/tests/dream_recall_integration.rs:27-49` surfaces one question, rewrites the file with that duplicate plus a new question, and asserts only the new question appears on the second startup.
  - Documentation evidence: `docs/api/stream-e-passive-recall-api.md:137` documents the runtime-local recent surfaced-question hash ring; `docs/api/stream-e-passive-recall-api.md:151-153` documents 7-day duplicate suppression and the intentionally absent omission counter.

## Findings

### [S2 / Medium] [API Contract, Correctness, Concurrency] Daemon `DreamNow` bypasses lease acquisition and ignores `force`

- Evidence: `crates/memoryd/src/handlers.rs:121-123` destructures `RequestPayload::DreamNow { scope, force: _, cli_override }`, explicitly discarding `force`; `crates/memoryd/src/handlers.rs:144-170` runs `DreamRunner` directly and never calls `crate::dream::lease::acquire_manual_lease` or appends `leases/journal.lease`. By contrast, the CLI path calls `memoryd::dream::lease::acquire_manual_lease` with `force: args.force` before building the runner (`crates/memoryd/src/main.rs:320-335`), and the lease implementation rejects active leases unless `force` is true (`crates/memoryd/src/dream/lease.rs:121-147`).
- Why it matters: The daemon protocol exposes `DreamNow` as an admin/manual dream trigger. A client using that protocol can currently run a dream even when another active lease exists for the same scope, while the CLI would fail fast with `lease_held`/exit 5. That weakens the Stream F single-active-run invariant and can create duplicate same-scope pass outputs or concurrent writes under `dreams/journal`, `dreams/questions`, and the candidate queue.
- Reasoning: Task 14 explicitly included daemon request handling for `DreamNow`/`DreamStatus` (`docs/plans/2026-04-30-stream-f-dreaming.md:798-809`). The Stream F contract defines manual dream lease semantics for `memoryd dream now`: read active leases, reject if one exists unless `--force`, append and push a new lease before Pass 1 (`docs/specs/stream-f-dreaming-v0.2.md:558-568`), and states `--force` changes lease behavior (`docs/specs/stream-f-dreaming-v0.2.md:570`, `docs/specs/stream-f-dreaming-v0.2.md:759`). Since `RequestPayload::DreamNow` carries `force`, dropping it at the daemon boundary is a contract break, not a harmless unimplemented option.
- Recommendation: Route daemon `DreamNow` through the same lease acquisition helper used by the CLI before constructing `DreamRunOptions`, propagate the request's `force` field, use the acquired lease run id, and map `LeaseError::{Held,Unavailable,DirtyTree,InvalidRequest}` through typed protocol errors. Add a daemon-level regression test that pre-seeds an active same-scope lease and proves `force: false` returns `lease_held` while `force: true` proceeds with `cli_override: Some("echo")`.
- Confidence: High.

## Non-blocking simplifications

- The daemon and CLI now each construct very similar `DreamRunOptions` and deterministic echo harnesses. Once the lease gap is fixed, extracting a shared manual-dream runner that accepts `{ repo, runtime, scope, force, cli_override }` would reduce drift between admin surfaces.

## Test gaps

- Missing daemon-level lease coverage for `RequestPayload::DreamNow`: no test proves active same-scope leases are rejected, `force` is honored, or a lease record is appended before daemon-triggered dream passes.
- Existing handler tests cover the now-fixed echo and typed-error cases, but they would not fail if the daemon continues to bypass lease acquisition.

## Commands run

- `cargo test -p memoryd --test handler_contract --test dream_recall_integration --test mcp_manifest` — PASS: 6 + 9 + 10 tests passed.
- `cargo test -p memoryd --test dream_cleanup --test dream_cli --test startup_recall_mcp --test cli_contract` — PASS: 10 + 7 + 5 + 4 tests passed.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings` — PASS.

## Positives

- The prior `not_implemented` symptom is covered at the daemon handler boundary, not only at the CLI boundary.
- The novelty-window fix is behavior-tested through startup recall and documented with a clear runtime-local persistence boundary.
- MCP dream/admin exclusion remains explicit in both implementation and manifest tests.
