# Stream F Review Gate D: Cleanup, Recall Hook, CLI/Status Clean-Code/Correctness Review

## Verdict

FAIL.

The Task 12-14 implementation is mostly cohesive and the targeted Gate D tests pass, but two contract-level issues remain:

1. `RequestPayload::DreamNow` is still a daemon `not_implemented` path even though the protocol exposes it and Task 14 required daemon request handling.
2. The pending-attention novelty-window requirement is not implemented; question hashes are used only as a sort key, not to suppress recently surfaced duplicate questions.

Because these are Task 13/14 contract misses, this gate should not pass until they are fixed or explicitly de-scoped in the plan/spec.

## Intended outcome

Tasks 12-14 are intended to complete the Stream F cleanup layer, Stream E Pass-3 `<pending-attention>` recall hook, and `memoryd dream` CLI/status/review/toggle surfaces after Gate C. The user-facing outcome is that cleanup is safe and git-disciplined, dream questions surface deterministically without adding hot-path risk or policy drift, and the CLI/admin protocol can inspect and manually run dreaming with stable exit/error behavior.

## Executive summary

The implementation passes the requested targeted tests and is generally readable: cleanup operations are split into bounded helpers, report DTOs are centralized, recall question selection is isolated, and CLI status/review/toggle behavior has useful coverage. I did not find evidence of body deletion in cleanup, and the dirty-tree cleanup path stages only cleanup-mutated paths while recording `commit_deferred`. Event compaction uses the declared `zstd` dependency and preserves a framed live tail. However, the daemon protocol's `DreamNow` surface is still a stub, and the recall hook omits the spec-required recent-question novelty suppression. Those are meaningful correctness/API-contract gaps.

## Findings

### [Medium] [API Contract] Daemon `DreamNow` request is exposed but still returns `not_implemented`

- Evidence: `crates/memoryd/src/protocol.rs:95-100` defines `RequestPayload::DreamNow { scope, force, cli_override }` and `RequestPayload::DreamStatus {}`; `crates/memoryd/src/protocol.rs:156-157` defines matching `ResponsePayload::DreamNow` and `DreamStatus`. But `crates/memoryd/src/handlers.rs:119-120` handles every `DreamNow` request with `HandlerError::not_implemented("memoryd dream now is not implemented yet")`. Task 14 explicitly called for daemon request handling for `DreamNow`/`DreamStatus`.
- Why it matters: Any client using the daemon protocol contract cannot manually trigger a dream run even though the protocol advertises that capability. The binary CLI currently bypasses the daemon and runs `run_manual_dream` locally, so `dream now --cli echo` passes while the daemon/API surface remains broken.
- Reasoning: Stream F v0.2 adds `RequestPayload::DreamNow` as a daemon protocol addition, not just a local CLI implementation. A protocol variant that always returns `not_implemented` creates a false contract for downstream clients/tests and can regress the intended admin surface once callers route through the daemon.
- Recommendation: Implement `RequestPayload::DreamNow` in `handlers.rs` by reusing the same lease acquisition + harness selection + `DreamRunner` path as the CLI, returning `ResponsePayload::DreamNow(Box<DreamRunReport>)` and preserving the documented error codes/retryability. Add a daemon-level regression test that sends `DreamNow { scope: "me", cli_override: Some("echo") }` through `handle_request_with_state` or a socket-backed server.
- Confidence: High.

### [Medium] [Correctness] Pending-attention novelty-window suppression is missing

- Evidence: The spec requires the deterministic ordering step to include novelty handling that skips questions whose text hash matches a question surfaced in the last 7 days, with `dreams.pending_attention_recent_window_days` defaulting to 7 (`docs/specs/stream-f-dreaming-v0.2.md:175`, `docs/specs/stream-f-dreaming-v0.2.md:369`). The implementation computes `novelty_hash` in `crates/memoryd/src/recall/dream_questions.rs:113-120` and sorts by it in `crates/memoryd/src/recall/dream_questions.rs:187-193`, but `select_pending_attention_questions` takes only `repo`, `namespaces_in_scope`, and `active_entity_ids` (`crates/memoryd/src/recall/dream_questions.rs:45-49`) and there is no recent-surfaced hash ring/state lookup in `startup.rs:77-81` or `dream_questions.rs`.
- Why it matters: Repeated Pass-3 questions can be shown on every startup until the question file changes, undermining the cognitive-load goal of the 2/scope and 6-total caps. It also means newer or different questions can be crowded out by stale duplicates even though the spec explicitly tried to prevent that.
- Reasoning: A hash used only as a tie-breaker does not satisfy the specified duplicate-suppression behavior. The current tests cover deterministic sort order and caps, but they would not fail if the same question appeared across repeated startup recall calls within the recent window.
- Recommendation: Add a small in-memory recent-question hash ring keyed by surfaced question hash and timestamp, parameterized by `dreams.pending_attention_recent_window_days`; skip matching hashes before cap selection. Add a regression test that surfaces a question once, invokes startup again within the window, and verifies the duplicate is skipped while another eligible question can surface.
- Confidence: High.

## Non-blocking simplifications

- Consider extracting the shared `scope_from_dream_path` logic currently duplicated between `dream/status.rs` and `dream/review.rs` into one small helper in the `dream` module. This is not blocking, but it would reduce drift risk around Stream F's colon-free on-disk scope encoding.
- Consider moving the CLI's manual dream execution path out of `main.rs` into a `dream::now`/`dream::run_command` module so the daemon handler and CLI can share one implementation instead of diverging.

## Test gaps

- No daemon-level test covers `RequestPayload::DreamNow`; existing `dream_cli` coverage exercises only the binary's direct local path.
- No recall test covers recent-window duplicate suppression for previously surfaced dream questions.
- `memoryd status` currently reports `dreams: Default::default()` through `handlers.rs:126-132`; if the intended status surface includes dream counters outside `memoryd dream status`, add coverage for that contract or explicitly de-scope it.

## Commands run

- `cargo test -p memoryd --test dream_cleanup --test dream_recall_integration --test dream_cli` — PASS (10 + 7 + 8 tests passed).
- `cargo clippy -p memoryd --all-targets --all-features -- -D warnings` — PASS.
- `cargo test -p memoryd --test startup_recall_mcp --test cli_contract` — PASS (5 + 4 tests passed).

## Positive notes

- Cleanup report shape and commit metadata are centralized in `dream/report.rs`, which keeps the git/report contract easy to audit.
- Cleanup tests cover idempotence, no body deletion, dirty-tree deferral, zstd archive decoding, and simulated dual-device convergence.
- The recall hook preserves the Stream E policy string and keeps question parsing/selection isolated in `recall/dream_questions.rs`, which is the right module boundary for this feature.
