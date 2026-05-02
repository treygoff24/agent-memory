# Stream G Review Gate B Security/Contract Rerun

## Verdict: Approve

The previous Gate B High/Medium findings are closed for the Stream G Tasks 4-7 scope. The daemon-backed Reality Check mutation path is now serialized, forget reasons are sanitized before synced persistence/event artifacts, the `RealityCheckDue` source hook has a daemon startup/hourly source, encrypted metadata-only rows are included through the real `List`/`Run` handler path without revealing titles/bodies, and `confirm` now drives the `observed_at` staleness source used by scoring.

I found no new High or Medium security/contract blockers.

## Scope reviewed

- Prior reviews:
  - `docs/reviews/stream-g-review-gate-b-security.md`
  - `docs/reviews/stream-g-review-gate-b-clean-code.md`
- Plan slice:
  - `docs/plans/2026-05-01-stream-g-observability.md` Review Gate B and Tasks 4-7.
- Code paths:
  - `crates/memoryd/src/state.rs`
  - `crates/memoryd/src/server.rs`
  - `crates/memoryd/src/handlers.rs`
  - `crates/memoryd/src/protocol.rs`
  - `crates/memoryd/src/mcp.rs`
  - `crates/memoryd/src/reality_check/{scheduling,scoring,session,types}.rs`
  - `crates/memory-substrate/src/api.rs`
  - `crates/memory-substrate/src/index/query.rs`
  - `crates/memory-substrate/src/model.rs`
  - Relevant Gate B tests under `crates/memoryd/tests/` and observed-at substrate tests.

## Findings

### High

None.

### Medium

None.

### Low

#### Low: Invalid Reality Check cron fallback still appears silent

**Files:**

- `docs/plans/2026-05-01-stream-g-observability.md:569`
- `crates/memoryd/src/reality_check/scheduling.rs:17-22`

**Exploitability:** No direct security exploit.

**Impact:** The plan says invalid cron expressions fall back to `0 9 * * SUN` with a warning log. The current `RcSchedule::parse_or_default` does safely fall back, but it does not emit an operator-visible warning. This is an observability/contract polish issue, not a Gate B blocker.

**Minimal remediation:** Emit a `tracing::warn!` or equivalent when an invalid expression is replaced with the default. Keep the safe fallback behavior.

## Previous findings closure

### Closed: Reality Check mutating actions are serialized / double-submit safe

- `HandlerState` now owns `reality_check_lock: Mutex<()>` and a shared notification sender (`crates/memoryd/src/handlers.rs:64-75`).
- `RealityCheckRequest::Run`, `Respond`, `Snooze`, and `Reset` take the shared lock before session/state mutation (`crates/memoryd/src/handlers.rs:171-186`).
- The real daemon server constructs one shared `Arc<HandlerState>` and passes it into every spawned connection task (`crates/memoryd/src/server.rs:73-83`, `crates/memoryd/src/server.rs:295-299`).
- Regression coverage exercises concurrent `forget` versus `not_relevant` for the same session and requires exactly one accepted mutation plus one stale-session refusal (`crates/memoryd/tests/responses.rs:249-287`).

### Closed: Forget reasons are redacted before synced persistence/event artifacts

- `RealityCheckAction::Forget` still validates the raw reason length before governance (`crates/memoryd/src/handlers.rs:263-272`).
- The value passed into governance forget and `EventKind::RealityCheckForgotten` is now `sanitize_forget_reason(&reason)`, not the raw input (`crates/memoryd/src/handlers.rs:272`, `crates/memoryd/src/handlers.rs:377-397`).
- Sanitization rejects unsafe classifier output plus obvious secret/PII markers and stores `[redacted]` (`crates/memoryd/src/handlers.rs:505-536`).
- Regression coverage verifies phone/email/secret canaries do not appear in repo text and the Reality Check forgotten event carries `[redacted]` (`crates/memoryd/tests/responses.rs:223-247`).

### Closed: `RealityCheckDue` startup/hourly source hook is wired for Gate B scope

- `NotificationEvent` includes `RealityCheckDue` without memory body/content payloads (`crates/memoryd/src/protocol.rs:275-284`).
- `HandlerState` stores the `broadcast::Sender` and exposes `subscribe_notifications()` (`crates/memoryd/src/handlers.rs:64-88`).
- The daemon-backed serve paths call the due check at startup and spawn an hourly loop (`crates/memoryd/src/server.rs:50-55`, `crates/memoryd/src/server.rs:73-83`, `crates/memoryd/src/server.rs:86-108`).
- Scheduler tests cover due, not-due, snooze, overdue, fallback cron parsing, direct event firing, and shared `HandlerState` event firing (`crates/memoryd/tests/scheduling.rs:7-83`).

Task 8 still owns actual notification dispatch routing. Residual integration risk: when Task 8 adds a real dispatcher, subscribe it before or during startup due evaluation so a startup `RealityCheckDue` event is not lost before a receiver exists.

### Closed: Encrypted metadata-only rows are safe in the real `List`/`Run` handler path

- Substrate now exposes `query_recall_index_including_metadata_only` (`crates/memory-substrate/src/api.rs:1059-1068`).
- The index query only applies `memories.metadata_only = 0` when the include-metadata-only path is not selected (`crates/memory-substrate/src/index/query.rs:890-914`).
- `RcSessionHandler::scored_items` uses the metadata-only-inclusive path for Reality Check scoring/listing (`crates/memoryd/src/reality_check/session.rs:163-181`).
- Wire conversion blanks the title for encrypted items (`crates/memoryd/src/reality_check/session.rs:220-230`).
- Handler-level regression coverage writes an encrypted memory, calls Reality Check `List`, and asserts the item is present with `encrypted = true` and `title = ""` (`crates/memoryd/tests/responses.rs:289-304`).

### Closed: `confirm` uses observed-at/staleness semantics

- `confirm` writes `observed_at` into frontmatter extras and bumps confidence without using governance (`crates/memoryd/src/handlers.rs:294-312`).
- The substrate index maps frontmatter `extras["observed_at"]` into the SQLite `memories.observed_at` column, falling back to `created_at` when absent (`crates/memory-substrate/src/index/query.rs:569`, `crates/memory-substrate/src/index/query.rs:917-925`).
- Scoring reads `COALESCE(observed_at, created_at)` from the index rather than using unrelated `updated_at` as the staleness source (`crates/memoryd/src/reality_check/scoring.rs:149-160`).
- Regression coverage verifies confirm persists `observed_at`, metadata-only `not_relevant` updates do not reset it, and substrate index writes `observed_at` from the frontmatter extra (`crates/memoryd/tests/responses.rs:57-123`, `crates/memory-substrate/tests/recall_index_row_indexed_at.rs:27-50`).

## Other positive validations

- Reality Check requests remain rejected by the MCP forwarder before socket I/O (`crates/memoryd/src/mcp.rs:222-234`).
- State-file load now reports corrupt/version-mismatched fallback reasons while preserving safe startup defaults (`crates/memoryd/src/state.rs:31-46`, `crates/memoryd/src/state.rs:61-78`).
- State/session writes still use create-temp, `sync_all`, rename, and directory fsync (`crates/memoryd/src/state.rs:254-272`).
- Notification event payload shapes do not contain memory body content (`crates/memoryd/src/protocol.rs:275-284`).
- Scoring stays on index/events data paths; I did not find per-item `Substrate::read_memory` calls in the scoring library.

## Validations run

```bash
cargo test -p memoryd --test daemon_state_files --test responses --test scheduling --test protocol_contract --test notification_channel --test scoring --test doctor_mirror_health
```

Result: passed.

- `daemon_state_files`: 15 passed
- `doctor_mirror_health`: 3 passed
- `notification_channel`: 2 passed
- `protocol_contract`: 12 passed
- `responses`: 15 passed
- `scheduling`: 8 passed
- `scoring`: 20 passed

```bash
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Result: passed.

## Residual risk and confidence

Residual risk is low for Gate B. This was a code-and-test rerun, not a live UI/dispatcher review. Task 8 should explicitly handle dispatcher subscription order around the startup due event. The public `handlers::handle_request` convenience helper still creates a fresh `HandlerState` per call; the real daemon path uses the shared-state entrypoint, but future in-process Reality Check callers should use `handle_request_with_state` if they need the same serialization and notification semantics.

Confidence is high for the prior High/Medium closure because the fixes are visible at the handler/server/substrate boundaries and the requested targeted gates pass.
