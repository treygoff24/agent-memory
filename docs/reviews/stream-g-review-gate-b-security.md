# Stream G Review Gate B Security/Contract Review

## Verdict: Changes requested

Stream G Tasks 4-7 are close on the happy-path tests, but I found security/contract blockers in the Reality Check mutation path. The primary issue is that Reality Check state and memory mutations are not serialized even though the spec relies on a single-writer daemon model. A concurrent double-submit can apply stale session state after another action has already tombstoned or mutated the same memory.

## Scope reviewed

- Plan: `docs/plans/2026-05-01-stream-g-observability.md` Review Gate B and Tasks 4-7.
- Spec: `docs/specs/stream-g-observability-v0.1.md` Reality Check response semantics, §5.7 protocol wire shapes, and §5.8 state-file recovery.
- Code paths:
  - `crates/memoryd/src/state.rs`
  - `crates/memoryd/src/protocol.rs`
  - `crates/memoryd/src/mcp.rs`
  - `crates/memoryd/src/handlers.rs`
  - `crates/memoryd/src/reality_check/{scoring,session,scheduling,types}.rs`
  - Relevant daemon server dispatch in `crates/memoryd/src/server.rs`
  - Relevant substrate event/index behavior where needed.

## Findings

### High: Reality Check mutating actions are not serialized; stale responses can corrupt memory/session state

**Files:**

- `crates/memoryd/src/server.rs:98-104`
- `crates/memoryd/src/handlers.rs:191-240`
- `crates/memoryd/src/handlers.rs:376-405`
- `crates/memoryd/src/handlers.rs:333-352`
- `crates/memoryd/src/state.rs:172-175`
- `crates/memoryd/src/state.rs:200-216`

**What happens:**

The spec assumes mutating Reality Check access is serialized through the daemon. In practice, the daemon accepts each Unix-socket connection and `tokio::spawn`s a per-connection task (`server.rs:98-104`). `RealityCheckRespond` then:

1. Loads the current session (`handlers.rs:193`).
2. Performs the memory mutation/event side effect (`handlers.rs:198-237`).
3. Advances and saves/deletes the session file (`handlers.rs:240`).

There is no per-daemon mutex/actor guarding that load-mutate-advance critical section. The state writer also uses one fixed temp path per file (`state.rs:205-216`), and `RcSessionStore::save` has no mutual exclusion (`state.rs:172-175`).

**Exploitability:**

A user can trigger this accidentally with two UI tabs or intentionally by submitting two socket requests for the same `session_id`/`memory_id` at nearly the same time. A concrete race:

1. Request A starts `not_relevant` and reads the active memory in `mutate_reality_check_metadata`.
2. Request B starts `forget`, loads the same session, tombstones the memory, and emits `RealityCheckForgotten`.
3. Request A writes its stale full-memory snapshot back with `WriteMode::AdminRepair` and `expected_base_hash: None` (`handlers.rs:389-395`), preserving the old active status while changing only the metadata fields.

That can effectively undo or partially overwrite the forget action, violating the user's expectation that forget/tombstone is final. Even when it does not resurrect state, two concurrent advances can lose session progress or produce duplicate/inconsistent events.

**Impact:**

- Breaks the Reality Check session integrity contract.
- Can undermine `forget`/tombstone semantics.
- Can produce duplicate or contradictory event history.
- Violates the plan/spec's reliance on daemon-side serialized mutation.

**Minimal remediation:**

- Add a single-writer path for Reality Check mutations. The simplest patch is an `Arc<tokio::sync::Mutex<()>>` or narrower `RealityCheckStateLock` in `HandlerState`, acquired around `Run`, `Respond`, `Snooze`, and `Reset` from session load through state save/delete. An actor/queue is also fine.
- For metadata-only updates (`confirm`, `not_relevant`), avoid stale full-memory overwrites. Prefer an API that updates frontmatter fields with a base hash/CAS check, or expose/read the base hash and pass it as `expected_base_hash`.
- Add a regression test with two concurrent `Respond` calls for the same item, especially `forget` racing `not_relevant` or `confirm`.

### Medium: Free-form Reality Check forget reasons are persisted without privacy filtering

**Files:**

- `crates/memoryd/src/handlers.rs:221-230`
- `crates/memoryd/src/handlers.rs:339-350`
- `crates/memoryd/src/handlers.rs:1638-1661`
- `crates/memory-substrate/src/api.rs:893-903`
- `crates/memory-substrate/src/events/log.rs:120-128`

**What happens:**

`RealityCheckAction::Forget` validates only `reason.trim().len() >= 3` before calling the governance forget path (`handlers.rs:221-230`). The same raw reason is then:

- stored in the tombstoned memory's frontmatter tombstone event via `TombstoneRequest.reason` (`api.rs:893-903`);
- written to `tombstones/memoryd-forget.jsonl` as `reason_text` (`handlers.rs:1638-1661`);
- written again into the canonical event log as `EventKind::RealityCheckForgotten { reason }` (`handlers.rs:346-350`, event shape at `events/log.rs:120-128`).

No `safe_plaintext_fragment`/privacy classifier check is applied to that user-provided reason.

**Exploitability:**

This is easy to hit accidentally. A user prompt that asks "why forget?" often elicits text like "contains API key sk-..." or "old phone number was ...". That text is then persisted into repo/event artifacts. The event log and tombstone rule files are not encrypted.

**Impact:**

- Potential plaintext secret/PII leakage into synced repo artifacts.
- The leak is amplified by writing the same reason in multiple places.
- This weakens Stream D privacy guarantees around secret/private material even though the memory body itself may be encrypted or tombstoned.

**Minimal remediation:**

- Bound and classify the forget reason before any disk/event side effect.
- Either reject unsafe reason text and ask for a safe reason, or persist only a reason enum plus a hash/redacted descriptor.
- If the public event contract must keep a `reason` field, store a sanitized bounded reason and keep raw local-only state out of the synced repo/event log.
- Add a regression test with a secret canary in the forget reason and assert it does not appear under repo/runtime plaintext artifacts.

### Medium: `RealityCheckDue` notification scheduling exists only as a library helper, not daemon behavior

**Files:**

- `crates/memoryd/src/protocol.rs:17-21`
- `crates/memoryd/src/protocol.rs:275-284`
- `crates/memoryd/src/reality_check/scheduling.rs:65-75`
- `crates/memoryd/src/handlers.rs:60-68`
- `crates/memoryd/src/main.rs:35-39`

**What happens:**

Task 5 says the notification broadcast channel should be created and stored in `memoryd` shared state; Task 7 says scheduling checks run on daemon startup and hourly, firing `RealityCheckDue` when due. The current code defines `NotificationEvent` and a `RcScheduler::check_and_fire_if_due` helper, but `HandlerState` has only recall counters, and daemon startup only loads state/session files. There is no stored `broadcast::Sender<NotificationEvent>`, no startup due check, and no hourly scheduler loop.

**Exploitability:**

No adversary is needed; this is a contract gap. In production, due Reality Check notifications will not be emitted by the daemon path implemented in Tasks 4-7.

**Impact:**

- Review Gate B's event-emission invariant is not actually satisfied end to end.
- Task 8 notification dispatch will have no real channel/source to subscribe to unless it adds the missing Task 5/7 plumbing.

**Minimal remediation:**

- Add `notifications: broadcast::Sender<NotificationEvent>` to `HandlerState` or a daemon app state object.
- Create the channel at daemon startup using `NOTIFICATION_CHANNEL_CAPACITY`.
- Run `RcScheduler::check_and_fire_if_due` on startup and from an hourly task.
- Add an integration-style daemon test that observes `RealityCheckDue` through the real shared state/channel, not only a unit test with a locally constructed sender.

### Low: Encrypted Reality Check items are tested in scoring only, but `List`/`Run` exclude metadata-only encrypted rows

**Files:**

- `docs/specs/stream-g-observability-v0.1.md:912`
- `crates/memoryd/src/reality_check/session.rs:169-180`
- `crates/memory-substrate/src/index/query.rs:874-880`
- `crates/memoryd/src/reality_check/session.rs:220-224`
- `crates/memoryd/tests/scoring.rs:178-189`

**What happens:**

The spec says encrypted memories are scored using index-visible fields only and shown with no title/body. `score_memories_at` can score an encrypted fixture when handed a row directly, and `scored_item_to_wire` blanks the title when `item.encrypted` is true. But the real `List`/`Run` path obtains rows through `Substrate::query_recall_index`, and the recall-index query hard-filters `memories.metadata_only = 0`. Encrypted writes are normally metadata-only rows, so the admin Reality Check surface never sees them.

**Exploitability:**

Not a data leak in the current code; it fails closed by omission. The risk is contract drift: the encrypted-item reveal/deny semantics are not exercised in the real handler path.

**Impact:**

- Encrypted memories are silently absent from Reality Check despite the spec.
- The claim that encrypted items render as empty-title items is only unit-tested below the actual handler/query boundary.

**Minimal remediation:**

- Either adjust the spec to say encrypted metadata-only memories are out of scope for v1 Reality Check, or add a safe metadata-only recall-index query path for Reality Check.
- Add a handler-level test: write an encrypted memory with safe descriptors, call `RealityCheckRequest::List`/`Run`, and assert the item is present with `encrypted: true`, empty `title`, and no body/private fields.

### Low: `confirm` does not update the `observed_at` field described by the spec

**Files:**

- `docs/specs/stream-g-observability-v0.1.md:950-953`
- `crates/memoryd/src/handlers.rs:252-260`
- `crates/memory-substrate/src/index/query.rs:615-616`

**What happens:**

The spec says `confirm` sets `memory.observed_at = now`. The implementation updates `frontmatter.updated_at` and bumps confidence, but there is no typed `Frontmatter::observed_at`; the index writer still sets the SQLite `observed_at` column to `NULL`.

**Exploitability:**

No direct security exploit. This is a scoring/contract drift issue.

**Impact:**

- Future code reading `observed_at` directly will not see confirmations.
- Current scoring falls back to `updated_at`, so the happy path works indirectly, but the named contract is not satisfied.

**Minimal remediation:**

- Add a typed observed-at field or a clearly documented extras-backed bridge, then hydrate the index column from it.
- Update the response test to assert the actual persisted observed-at source, not only `updated_at`.

## Validations run

All requested and adjacent Gate B checks passed:

```bash
cargo test -p memoryd --test daemon_state_files --test responses --test scheduling --test protocol_contract --test notification_channel
```

Result: passed (`13 + 2 + 12 + 11 + 7` tests).

```bash
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
```

Result: passed.

Additional adjacent checks run because Review Gate B/Tasks 4 and 6 depend on them:

```bash
cargo test -p memoryd --test scoring --test doctor_mirror_health
```

Result: passed (`20 + 3` tests).

## Positive validations

- State files are rooted under `substrate.roots().runtime/state`, not the synced memory repo (`state.rs`, `session.rs`, `handlers.rs`).
- State file writes use write-temp, `sync_all`, rename, and directory fsync (`state.rs:200-216`).
- Missing/corrupt/version-mismatched daemon state falls back to defaults; corrupt session files are renamed and stale sessions older than 7 days are discarded (`state.rs:148-169`, `state.rs:220-227`).
- MCP forwarder rejects raw `RequestPayload::RealityCheck(_)` with `method_not_allowed_on_mcp` before socket I/O (`mcp.rs:222-234`).
- `not_relevant` sets `passive_recall = false` and tags the memory without intentionally tombstoning (`handlers.rs:355-374`).
- `forget` reason length validation happens before the governance forget call (`handlers.rs:221-230`).
- Notification event payload shapes currently do not carry memory bodies or Reality Check item content (`protocol.rs:275-284`).

## Residual risk and confidence

Residual risk is moderate because this review was code-and-test based, not an interactive concurrency proof with a newly added race test. Confidence is high for the reported issues: the line-level evidence shows concurrent request handling, no Reality Check mutation lock, unsanitized reason persistence, and missing notification-channel wiring.
