# Stream G Review Gate C Security/Privacy Rerun

## Verdict: Approved

The Gate C security/privacy remediation is approved. I found no remaining blocking findings in the scoped prior issues. The prior medium/low issues are materially remediated: passive notifications are retained in shared daemon state and exposed through status, external failure reporting is sanitized, the production TUI starts from empty/loading data rather than sample memories, `EmailMessage` debug output redacts the password, and trust-artifact git status uses argument-based invocation with literal pathspec handling and path validation.

## Scope reviewed

- Prior review: `docs/reviews/stream-g-review-gate-c-security.md`
- Code paths:
  - `crates/memoryd/src/notifications/`
  - `crates/memoryd/src/server.rs`
  - `crates/memoryd/src/handlers.rs`
  - `crates/memoryd/src/protocol.rs`
  - `crates/memoryd/src/trust_artifact.rs`
  - `crates/memoryd-tui/src/app.rs`
  - `crates/memoryd-tui/src/widgets/trust_artifact.rs`
- Regression tests:
  - `crates/memoryd/tests/dispatcher.rs`
  - `crates/memoryd/tests/handler_contract.rs`
  - `crates/memoryd/tests/protocol_contract.rs`
  - `crates/memoryd/tests/trust_artifact.rs`
  - `crates/memoryd-tui/tests/socket_unreachable.rs`
  - `crates/memoryd-tui/tests/panel_render.rs`
  - `crates/memoryd-tui/tests/keymap.rs`
  - `crates/memoryd-tui/tests/resize.rs`
  - `crates/memoryd-tui/tests/trust_artifact.rs`

Note: the worktree was already heavily modified/untracked before this rerun. I treated that as the current remediation state and only wrote this review artifact.

## Findings

None.

## Prior findings verification

### 1. Passive queue retained/exposed by daemon status and no longer silent

**Status: Verified remediated.**

Evidence:

- `HandlerState` now owns a shared `PassiveQueue` at `crates/memoryd/src/handlers.rs:86-127`.
- The substrate-backed daemon passes that shared queue into the production dispatcher at `crates/memoryd/src/server.rs:93-96`.
- `StatusResponse` serializes `passive_notifications` at `crates/memoryd/src/protocol.rs:303-318`.
- `status_response` populates `passive_notifications` from the shared queue at `crates/memoryd/src/handlers.rs:415-427`.
- Regression coverage: `status_response_surfaces_shared_passive_notifications` at `crates/memoryd/tests/handler_contract.rs:517-535`.

This addresses the previous silent-fallback concern: dispatcher-generated passive events are retained in daemon state and surfaced through status rather than living only inside an unreachable task-local queue.

### 2. External delivery fallback/log path sanitizes raw errors and does not expose Slack webhook URL/token

**Status: Verified remediated.**

Evidence:

- `ExternalDeliveryError::sanitized_reason()` returns a constant content-free message at `crates/memoryd/src/notifications/external.rs:61-74`.
- The final retry failure log and passive fallback use only `error.sanitized_reason()` at `crates/memoryd/src/notifications/external.rs:156-159`.
- Reqwest transport errors are stripped with `without_url()` before being stored as raw delivery errors at `crates/memoryd/src/notifications/external.rs:250-270`.
- Regression coverage injects a canary Slack URL/token and asserts the passive fallback omits both `SECRET_CANARY_TOKEN` and `hooks.slack.com` at `crates/memoryd/tests/dispatcher.rs:126-145`.

The current fallback/log path does not print the raw Slack webhook URL or token. The raw reason still exists inside `ExternalDeliveryError`, but the reviewed production failure path does not expose it.

### 3. Production TUI loading/connected status-only path no longer shows hard-coded sample memory content as live data

**Status: Verified remediated.**

Evidence:

- Production startup constructs `App::new(config)` at `crates/memoryd-tui/src/app.rs:503-512`.
- `App::new` initializes with `DaemonSnapshot::loading(&config.socket_path)` at `crates/memoryd-tui/src/app.rs:185-204`.
- `DaemonSnapshot::loading` now derives from `DaemonSnapshot::empty()`, not `DaemonSnapshot::sample()`, at `crates/memoryd-tui/src/app.rs:722-743`.
- `poll_daemon` only updates status counters/state and marks the socket connected at `crates/memoryd-tui/src/app.rs:300-309`; because production starts from the empty/loading snapshot, status-only polling does not preserve sample memory rows.
- Regression coverage: `test_loading_snapshot_does_not_render_sample_memory_content` asserts the initial production-style loading snapshot omits prior sample strings at `crates/memoryd-tui/tests/socket_unreachable.rs:43-53`.
- Focused grep found remaining sample memory strings in `DaemonSnapshot::sample()` and tests only; production `run` uses `App::new` rather than `App::with_snapshot(DaemonSnapshot::sample())`.

The fixture/sample snapshot remains in source for tests and explicit fixture construction, but I did not find a production loading or status-only connected path that renders it as live daemon data.

### 4. `EmailMessage` debug redacts password

**Status: Verified remediated.**

Evidence:

- `EmailMessage` no longer derives `Debug`; it has a manual `fmt::Debug` implementation at `crates/memoryd/src/notifications/external.rs:33-59`.
- The manual formatter prints `password` as `[redacted]` at `crates/memoryd/src/notifications/external.rs:45-58`.
- Regression coverage: `test_email_message_debug_redacts_password` asserts the debug string contains `[redacted]` and omits `smtp-secret-canary` at `crates/memoryd/tests/dispatcher.rs:217-235`.

### 5. Trust artifact git status path handling hardened enough for Gate C

**Status: Verified remediated for Gate C.**

Evidence:

- Encrypted/metadata-only trust artifacts are redacted before sync-state construction at `crates/memoryd/src/trust_artifact.rs:149-157`.
- `memory_path_git_status` rejects invalid `RepoPath` values and pathspec-magic prefixes beginning with `:` at `crates/memoryd/src/trust_artifact.rs:426-429`.
- `RepoPath::try_new` rejects empty/NUL, absolute paths, `.`/`..`, and paths outside the allow-listed repository tiers at `crates/memory-substrate/src/model.rs:1018-1058` and `crates/memory-substrate/src/model.rs:1100-1128`.
- The git call uses `std::process::Command` arguments, not a shell, and includes `--literal-pathspecs`, `--`, and the validated repo-relative path as a separate argument at `crates/memoryd/src/trust_artifact.rs:430-439`.
- `git_binary()` prefers `/usr/bin/git` when present and falls back to `git` only otherwise at `crates/memoryd/src/trust_artifact.rs:449-455`.

This satisfies the Gate C requirement for no shell injection, literal pathspec handling, safe path validation, and absolute git where available. A timeout around the git child process would still be a reasonable future hardening improvement, but I do not consider it a Gate C blocker.

## Additional privacy validations

### Encrypted trust artifacts still redact title/body

**Status: Verified.**

Evidence:

- Server-side trust artifacts map encrypted or metadata-only content to `SafeContent::Encrypted` at `crates/memoryd/src/trust_artifact.rs:149-157`.
- Supersession links redact linked encrypted-memory titles at `crates/memoryd/src/trust_artifact.rs:331-340`.
- Server-side regression coverage: `encrypted_memory_shows_content_redacted_but_keeps_other_sections` at `crates/memoryd/tests/trust_artifact.rs:40-56`.
- TUI-side widget rendering uses the encrypted placeholder instead of title/body values at `crates/memoryd-tui/src/widgets/trust_artifact.rs:28-40` and `crates/memoryd-tui/src/widgets/trust_artifact.rs:188-260`.
- TUI-side regression coverage: `test_encrypted_memory_shows_content_redacted_without_leaking_private_text` at `crates/memoryd-tui/tests/trust_artifact.rs:93-109`.

### Notification payloads contain no memory content

**Status: Verified.**

Evidence:

- Passive notification text is generated from content-free summaries and intentionally ignores `memory_id`, `path`, and `scope` fields at `crates/memoryd/src/notifications/dispatcher.rs:50-72`.
- External Slack/email summaries are also content-free and ignore event fields that could carry memory identifiers, repo paths, or scopes at `crates/memoryd/src/notifications/external.rs:345-392`.
- Slack payload regression coverage asserts the outgoing body omits canary title/entity/body strings at `crates/memoryd/tests/dispatcher.rs:147-169`.

## Commands run

```bash
cargo test -p memoryd --test dispatcher --test trust_artifact --test handler_contract --test protocol_contract
```

Result: passed.

- `dispatcher`: 12 passed
- `handler_contract`: 14 passed
- `protocol_contract`: 12 passed
- `trust_artifact`: 8 passed

```bash
cargo test -p memoryd-tui --test socket_unreachable --test panel_render --test keymap --test resize --test trust_artifact
```

Result: passed.

- `keymap`: 10 passed
- `panel_render`: 10 passed
- `resize`: 2 passed
- `socket_unreachable`: 3 passed
- `trust_artifact`: 4 passed

```bash
cargo clippy -p memoryd -p memoryd-tui --all-targets --all-features -- -D warnings
```

Result: passed.

Focused static checks used:

```bash
rg -n "ExternalDeliveryError|external notification failed|External notification failed|without_url|webhook_url|hooks\.slack|SECRET_CANARY|tracing::(warn|error|debug|info)!|format!\(.*error|error\.to_string\(\)" crates/memoryd/src/notifications crates/memoryd/tests/dispatcher.rs
rg -n "pub struct RepoPath|impl RepoPath|fn try_new|starts_with\(':|literal-pathspecs|git_binary|Command::new|\.arg\(repo_path\)|without_url|sanitized_reason|passive_notifications|DaemonSnapshot::loading|DaemonSnapshot::sample|External notification failed|smtp-secret-canary|SECRET_CANARY_TOKEN|Prefer CITEXT|Deploy target is production ECS|encrypted - use memoryd reveal" crates/memory-substrate crates/memoryd crates/memoryd-tui -g '!target'
rg -n "NotificationEvent::|NotificationEvent\b|passive_message|external_summary|slack_payload|email_message|os_notification|notify\(" crates/memoryd/src crates/memoryd/tests -g '!target'
rg -n "git status|literal|pathspec|RepoPath|merge_status|starts_with\(':|evil|injection|absolute|unknown|modified|clean" crates/memoryd/tests/trust_artifact.rs crates/memoryd/src/trust_artifact.rs crates/memory-substrate/tests -g '!target'
```

Result: no scoped regressions found beyond the residual notes above.

## Residual risk and confidence

Residual risk is low for the prior Gate C findings. I did not run a full-workspace test/clippy audit or review every unrelated `Command::new` call in the repository. The TUI fixture snapshot still exists in production source, but the production startup/status-only path no longer uses it. The trust-artifact git path is safe against shell injection and pathspec surprises in the reviewed path; adding a child-process timeout would further harden availability.

Confidence is high for the prior-finding remediation verdict because it is backed by direct code review, focused grep/static checks, the requested targeted tests, and the requested clippy gate.
