### Verdict

Approve

Approved.

### Intended outcome

Stream G appears intended to ship the user-facing observability contract over already-shipped Streams A-F: `memoryd ui`, localhost web dashboard routes, Reality Check CLI/daemon protocol, notification event semantics, trust artifact/audit rendering, and release-gate bench evidence. This rerun specifically checks whether the prior API-contract blockers in `docs/reviews/stream-g-final-api-contract-review.md` were fixed: the Reality Check docs/CLI/protocol mismatch, the web audit response shape mismatch, and tests that previously accepted the wrong contract.

### Executive summary

The previously blocking API-contract drift is resolved. `docs/api/stream-g-observability-api.md` now documents the actual implemented Reality Check CLI commands, the daemon `RealityCheckRequest`/`RealityCheckResponse` variants, the top-level web audit response shape, and the internal-only seven-variant `NotificationEvent` channel. The web audit route now returns top-level audit/trust fields rather than the old `{ memory_id, artifact, sections }` envelope, and `crates/memoryd-web/tests/api_contract.rs` explicitly asserts both the required top-level fields and the absence of the old wrapper fields. Narrow API-contract validation passed for memoryd CLI/protocol/notification tests and memoryd-web API tests. A parallel TUI keymap test run hit a socket-name collision in the test harness, but the failed test passed in isolation and the full TUI keymap test suite passed serially; I do not consider that an API-contract blocker for this review.

### Findings

No material issues found.

### Non-blocking simplifications

- The TUI keymap test helper `record_daemon_request` generates Unix socket paths from process id plus system-time nanoseconds (`crates/memoryd-tui/tests/keymap.rs:30-40`). One parallel run hit `File exists` before the same test passed in isolation and with `--test-threads=1`. This is not a Stream G contract blocker, but using `tempfile::TempDir` plus a unique filename, or including an atomic counter/UUID in the socket path, would remove a small source of CI flake.

### Test gaps

- No blocking API-contract gaps found for the prior blockers. The web audit contract is now asserted against the top-level shape and against absence of the old `artifact`/`sections` wrapper (`crates/memoryd-web/tests/api_contract.rs:101-121`).
- TUI daemon-dispatch coverage is materially stronger than the prior review state: review actions are dispatched to a socket-backed fake daemon and cleared/retried visibly (`crates/memoryd-tui/tests/keymap.rs:147-187`), and Reality Check actions assert the selected row's `memory_id` reaches `RequestPayload::RealityCheck(Respond { ... })` (`crates/memoryd-tui/tests/keymap.rs:189-247`). The residual gap is mostly harness robustness, not contract semantics.

### Questions / uncertainties

- I did not rerun the full workspace gate because the prompt states `cargo test --workspace --all-targets --all-features` and the broader integrated validation have already passed. I ran the narrower tests most directly tied to this API-contract rerun.
- The Stream G bench evidence still acknowledges synthetic TUI/web measurements for some non-scoring paths (`docs/reviews/stream-g-bench-evidence.md:108-113`). That is already recorded as residual performance evidence risk, not an API-contract mismatch, and the canonical Stream G baseline exists and asserts successfully per the same artifact (`docs/reviews/stream-g-bench-evidence.md:20-27`, `docs/reviews/stream-g-bench-evidence.md:54-58`).

### Positives

- The API docs now describe the implemented CLI/protocol contract precisely: no public `reset` CLI, `run --top-n` forwarding as `limit`, `snooze --until` as an optional UTC date, and admin-only Reality Check protocol with MCP rejection (`docs/api/stream-g-observability-api.md:81-136`).
- The web audit fix is contract-driven and test-protected: `AuditMemoryResponse` flattens the trust artifact into top-level fields (`crates/memoryd-web/src/routes/audit.rs:14-56`), the route maps daemon `TrustArtifact` responses into that shape (`crates/memoryd-web/src/routes/audit.rs:116-138`), and tests assert the old false-positive wrapper is gone (`crates/memoryd-web/tests/api_contract.rs:101-121`).
- The daemon/UI boundary is cleaner than in the prior review: the MCP forwarder fail-closes admin/UI payloads before socket I/O (`crates/memoryd/src/mcp.rs:223-242`), and the TUI dispatch loop now drains queued daemon calls during the production event loop (`crates/memoryd-tui/src/app.rs:324-341`, `crates/memoryd-tui/src/app.rs:581-593`).

### Files inspected

- `docs/reviews/stream-g-final-api-contract-review.md`
- `docs/api/stream-g-observability-api.md`
- `docs/specs/stream-g-observability-v0.1.md`
- `docs/plans/2026-05-01-stream-g-observability.md`
- `docs/reviews/stream-g-contract-map.md`
- `docs/reviews/stream-g-bench-evidence.md`
- `bench/stream-g-observability-results.darwin-arm64.json`
- `crates/memoryd/src/cli.rs`
- `crates/memoryd/src/protocol.rs`
- `crates/memoryd/src/mcp.rs`
- `crates/memoryd/src/trust_artifact.rs`
- `crates/memoryd/tests/cli_contract.rs`
- `crates/memoryd/tests/protocol_contract.rs`
- `crates/memoryd/tests/notification_channel.rs`
- `crates/memoryd/tests/trust_artifact.rs`
- `crates/memoryd-web/src/routes/audit.rs`
- `crates/memoryd-web/src/routes/mod.rs`
- `crates/memoryd-web/src/server.rs`
- `crates/memoryd-web/tests/api_contract.rs`
- `crates/memoryd-tui/src/app.rs`
- `crates/memoryd-tui/src/client.rs`
- `crates/memoryd-tui/tests/keymap.rs`

### Verification run

```bash
cargo test -p memoryd --test protocol_contract --test notification_channel --test cli_contract
cargo test -p memoryd-web --test api_contract
cargo test -p memoryd-tui --test keymap test_expired_review_action_reaches_daemon_and_clears_queue -- --exact --nocapture
cargo test -p memoryd-tui --test keymap -- --test-threads=1
```

Results:

- `memoryd` CLI/protocol/notification contract tests: PASS, 18 + 2 + 16 tests.
- `memoryd-web` API contract tests: PASS, 15 tests.
- Focused TUI dispatch test: PASS.
- TUI keymap suite serial: PASS, 15 tests.

Note: an earlier parallel `cargo test -p memoryd-tui --test keymap` run failed once in `test_expired_review_action_reaches_daemon_and_clears_queue` at socket bind with `File exists`; the same test passed when rerun directly and serially, so I classify it as a non-blocking test-harness flake.
