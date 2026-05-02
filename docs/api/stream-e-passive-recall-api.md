# Stream E Passive Recall API

Stream E is the passive recall layer for `memoryd`. It adds startup and per-turn recall blocks without creating a second persistence layer: all durable memory state remains Stream A files plus the derived SQLite index, governance remains Stream C, and privacy/reveal authority remains Stream D.

## MCP `memory_startup`

`memory_startup` is exposed in the MCP manifest and forwards to the daemon. The legacy `{ "include_recent": true }` shape is removed; callers must provide binding context.

Request:

```json
{
  "cwd": "/Users/treygoff/Code/agent-memory",
  "session_id": "sess_abc123",
  "harness": "codex",
  "harness_version": "0.0.0",
  "include_recent": true,
  "since_event_id": null,
  "budget_tokens": 3600
}
```

Required fields: `cwd`, `session_id`, `harness`.

Defaults and validation:

- `include_recent` defaults to `true`.
- `budget_tokens` defaults to `3600` and must be `512..=8000`.
- `cwd` must be absolute and canonicalizable.
- `session_id`, `harness`, and `harness_version` are trimmed and bounded.
- Non-null/non-empty `since_event_id` returns `not_implemented`; event-based deltas are deferred.

Daemon protocol request:

```rust
RequestPayload::Startup(StartupRequest)
```

Daemon protocol response:

```rust
ResponsePayload::Startup(Box<StartupResponse>)
```

`StartupResponse` contains:

- `session_binding`
- `recall_block`
- `budget_used_tokens`
- `recall_explanation`
- `guidance`

## CLI recall commands

Recall hook commands route through the running daemon socket. There is no direct-substrate fallback in Stream E.

```bash
memoryd recall startup-block \
  --repo . \
  --runtime .memoryd \
  --cwd "$PWD" \
  --session-id sess_abc123 \
  --harness codex \
  --budget-tokens 3600

memoryd recall delta-block \
  --repo . \
  --runtime .memoryd \
  --cwd "$PWD" \
  --session-id sess_abc123 \
  --harness codex \
  --message "what changed?" \
  --budget-tokens 512
```

By default the socket path is `<runtime>/memoryd.sock`; `--socket` can override it. `--repo` is accepted for hook contract clarity but does not enable a direct-substrate fallback.

On success, stdout contains XML only. Diagnostics and typed errors go to stderr.

Exit codes:

| Code | Meaning                                   |
| ---- | ----------------------------------------- |
| 1    | `invalid_request`                         |
| 2    | `substrate_error` or `recall_unavailable` |
| 3    | `privacy_error`                           |
| 4    | `not_implemented`                         |

If no daemon socket is reachable, recall CLI commands fail fast with `recall_unavailable` and exit 2.

## Recall XML shape

Startup emits one stable frame:

```xml
<memory-recall version="stream-e-v0.5" harness="codex" session="sess_abc123">
  <identity>
  </identity>
  <project-state>
  </project-state>
  <entity-recall entities="">
  </entity-recall>
  <recent-memory>
  </recent-memory>
  <pending-attention>
  </pending-attention>
  <recall-explanation policy="stream-e-v0.5" budget-tokens="3600" used-tokens="123">
  </recall-explanation>
</memory-recall>
```

Section order is always: `identity`, `project-state`, `entity-recall`, `recent-memory`, `pending-attention`, `recall-explanation`.

Delta no-match emits exactly:

```xml
<memory-delta empty="true" />
```

## Ranking, budgeting, and explanations

Budgeting uses the deterministic estimator `ceil(utf8_byte_len / 4)`. Rendered summaries are capped at 240 UTF-8 bytes and snippets at 360 UTF-8 bytes. XML text and attributes are escaped before output.

`RecallExplanation` includes:

- `budget_tokens`
- `budget_used_tokens`
- `policy = "stream-e-v0.5"`
- `sections[]` with selected ids, matched entities, per-section token estimates, and omitted counts
- bounded `omitted[]`
- `omitted_truncated_count`

`RecallOmission.alias` and `RecallOmission.colliding_ids` are optional/additive fields for ambiguous alias collisions.

## Privacy and governance constraints

Passive recall is read-only. It never calls `memory_reveal`, never decrypts, and never persists durable last-recalled state. Stream F's dream-question hook maintains only a runtime-local recent surfaced-question hash ring for novelty-window suppression; that ring resets when the daemon process restarts.

Fact recall includes only active/pinned, passive-recall-enabled, review-safe rows within the row's `max_scope`. Rows that require review, are candidate/quarantined, are tombstoned/superseded/archived, disable passive recall, or are unsafe for body recall are omitted or counted as pending attention.

Candidate and quarantined rows can affect `<pending-attention>` counts but their claim text is not emitted. Encrypted or metadata-only rows may contribute only safe metadata already available in Stream A's recall index.

Stream F adds an additive Pass-3 dream-question hook without changing `policy = "stream-e-v0.5"`. During startup recall only, the daemon reads the most recent `dreams/questions/<scope_path>/<YYYY-MM-DD>.jsonl` file with date `<= today` for each namespace in `namespaces_in_scope`. Each JSONL record must contain explicit `entities: string[]` and `question: string` fields. The hook entity-gates on the structured `entities` field against the active recall seed set; it does not unmask question text, call an LLM, decrypt memory, or rerun Pass 3.

Matching dream questions render as pending-attention line items:

```text
- [<scope>] <question text>
```

Question text is classified with Stream D `safe_plaintext_fragment` before emission, capped to 240 UTF-8 bytes, and XML-escaped by the recall renderer. Empty `entities` records never surface. Non-empty `entities` records with no active seed overlap are omitted. The daemon suppresses any matching question whose normalized 240-byte question-text hash was surfaced from the same repo in the last 7 days. Caps are exactly 2 questions per scope and 6 total. When caps apply, surfacing order is deterministic: entity-overlap strength, file recency, novelty hash, then lexicographic `(scope, question)`.

Recent-window suppressions are not currently represented in `dream_question_omitted_total`: Stream F v0.2 enumerates the stable status keys below and does not define a dedicated `recent_window` reason. The suppression remains observable in the rendered `<pending-attention>` output and resets with the in-process ring on daemon restart.

## Stream G Reality Check pending attention

Stream G adds one fixed pending-attention item when the weekly Reality Check is due and not snoozed:

```xml
<pending-attention>
  <item kind="reality_check_due" count="1">Weekly Reality Check is ready — run `memoryd reality-check run` or open TUI panel 8.</item>
</pending-attention>
```

Shorthand references to `<pending-attention kind="reality_check_due">` mean this item inside the existing `<pending-attention>` block; the shipped XML shape does not add attributes to the block itself.

The text is fixed and contains no memory title, body, entity name, or score. It emits at most once per 7-day window and is suppressed while `memoryd reality-check snooze` is active for the current week.

The item counts against Stream E's existing pending-attention caps: at most 2 items per scope and 6 total. If the 6-total cap is already full with higher-priority pending-attention items, `reality_check_due` is omitted and counted in the section's omitted count; the `stream-e-v0.5` XML version string does not change.

## Status counters

`StatusResponse.recall` is always present on new daemon status responses and is additive for older clients.

Fields:

- `startup_invoked_total`
- `startup_failed_total: { code: count }`
- `delta_invoked_total`
- `delta_failed_total: { code: count }`
- `budget_exhausted_total: { section: count }`
- `dream_question_omitted_total: { reason: count }`

Counters are in-process and reset on daemon restart.

`dream_question_omitted_total` reason keys are exactly:

- `cap_section`
- `cap_total`
- `no_entity_match`
- `unsafe_fragment`
- `malformed_record`
