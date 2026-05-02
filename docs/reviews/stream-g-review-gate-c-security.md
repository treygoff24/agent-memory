# Stream G Review Gate C Security/Privacy Review

## Verdict: Changes requested

Tasks 8-12 pass the focused tests I ran, and the core privacy shape is mostly right: external notification message bodies do not include memory titles/bodies/entity names, SMTP config stores an env-var name rather than a password value, and server-side encrypted trust artifacts redact the target title and body. However, I found security/privacy issues in the notification failure path, passive notification availability, TUI stale/fabricated data handling, and trust-artifact git invocation hardening.

## Scope reviewed

- Plan: `docs/plans/2026-05-01-stream-g-observability.md` Review Gate C and Tasks 8-12.
- Spec: `docs/specs/stream-g-observability-v0.1.md` notification channels/config, external payload privacy, TUI daemon-boundary rules, and trust artifact rendering/data-source rules.
- Code paths:
  - `crates/memoryd/src/notifications/`
  - `crates/memoryd/src/server.rs` notification dispatcher startup
  - `crates/memoryd/src/protocol.rs` notification/status wire structs
  - `crates/memoryd/src/handlers.rs` shared daemon state for notification channel ownership
  - `crates/memoryd/src/trust_artifact.rs`
  - `crates/memoryd-tui/`
  - Focused static check of `crates/memoryd-web/` for direct substrate bypass.

## Findings

### Medium: Passive notification queue is always appended in tests but not retained or exposed by the daemon

**Files:**

- `crates/memoryd/src/server.rs:93-96`
- `crates/memoryd/src/handlers.rs:80-112`
- `crates/memoryd/src/notifications/passive.rs:6-25`
- `crates/memoryd/src/protocol.rs:303-310`

**What happens:**

`PassiveQueue` itself has no config switch and the dispatcher always appends before OS/external routing. That satisfies the narrow "config cannot disable passive" property at the unit level.

The daemon startup path, however, constructs a fresh `PassiveQueue::new()` inside `spawn_notification_dispatcher` and moves it into the spawned task. `HandlerState` retains only the broadcast sender, not the passive queue, and `StatusResponse` has no pending-notifications field. A focused `rg` found no production holder or drain path outside tests and the notification module.

**Exploitability:**

No adversary is required. With OS notifications disabled by default and external notifications disabled unless configured, a security-relevant event such as `LeakedSecretDetected` is appended to an in-memory queue that no production status/pending-attention path can read.

**Impact:**

- The passive fallback can be operationally silent despite being "always on."
- Failed external notifications fall back into the same unobservable queue.
- Users can miss security and sync-blocking alerts that the spec says must remain visible through passive status/pending-attention surfaces.

**Minimal remediation:**

- Store the passive queue in daemon/shared state, not only inside the dispatcher task.
- Add pending notifications to `StatusResponse` and/or the Stream E pending-attention assembly path.
- Add an integration test that starts the daemon path, fires a notification, calls status, and observes the passive item.

### Medium: Raw external delivery errors can expose Slack webhook secrets in logs/passive notifications

**Files:**

- `crates/memoryd/src/notifications/external.rs:135-138`
- `crates/memoryd/src/notifications/external.rs:229-249`

**What happens:**

Slack webhook delivery maps `reqwest` transport errors directly with `error.to_string()`. After retries are exhausted, the raw error is logged and appended to the passive queue as `External notification failed: {error}`.

Slack webhook URLs are bearer credentials. On transport, DNS, TLS, proxy, or timeout failures, `reqwest` error display commonly includes the request URL. That can put the full webhook URL into daemon logs and then into the passive queue/status surface once passive exposure is wired.

**Exploitability:**

This is easy to trigger accidentally by configuring an invalid proxy/network, using a webhook URL with a temporary DNS/TLS failure, or blocking outbound access. Anyone with access to daemon logs or pending passive notifications could recover the webhook secret.

**Impact:**

- Slack webhook credential disclosure.
- Possible unauthorized posting into the configured Slack channel until the webhook is rotated.
- Secret exposure is amplified if logs are later shipped to an external sink.

**Minimal remediation:**

- Do not persist or log raw external-client errors for secret-bearing endpoints.
- For reqwest, strip URLs before storing/displaying errors (`without_url()` where applicable) or map transport failures to a generic reason such as `slack delivery failed`.
- Keep detailed transport error kinds only in debug logs after sanitization.
- Add a regression test with a canary token in the webhook URL and a failing transport path; assert the canary is absent from logs/passive messages.

### Medium: The production TUI can show hard-coded sample memory content as live connected data

**Files:**

- `crates/memoryd-tui/src/app.rs:300-330`
- `crates/memoryd-tui/src/app.rs:722-743`
- `crates/memoryd-tui/src/app.rs:818-853`
- `crates/memoryd-tui/src/app.rs:993-1017`
- `crates/memoryd-tui/src/widgets/trust_artifact.rs:110-123`

**What happens:**

`App::new` starts with `DaemonSnapshot::loading`, which is built from `DaemonSnapshot::sample`. The sample snapshot contains memory-like titles, namespaces, Reality Check items, and a trust artifact body. On successful daemon polling, `poll_daemon` updates only overview status/counters and sets `socket_state = Connected`; it does not clear or replace the rest of the sample snapshot. Rendering hides content only when `SocketState::Unreachable`; connected rendering uses the stale/sample snapshot.

**Exploitability:**

No adversary is required. A user running the TUI against a reachable daemon can see fabricated memory titles/body/trust-artifact content as though it were live daemon data. If future fixture text copies real local memory content, this also becomes an accidental UI privacy leak.

**Impact:**

- Violates the Gate C requirement that TUI surfaces not show stale data as live data.
- Undermines trust in review/forget/correct actions because rows may not correspond to daemon state.
- Risks accidental display of fixture/private content in operator demos or screenshots.

**Minimal remediation:**

- Replace `DaemonSnapshot::loading` with an empty/loading snapshot, not `sample()`.
- Make `sample()` test-only or clearly fixture-gated.
- Until real daemon DTOs exist for panels, render "not loaded / endpoint not implemented" placeholders after a successful status poll rather than fixture memory content.
- Add a TUI test proving connected status-only polling does not render sample titles, bodies, entity names, or trust artifact content.

### Low: SMTP password is read from an env var, but the in-memory message type is debug-printable with the password

**Files:**

- `crates/memoryd/src/notifications/config.rs:57-65`
- `crates/memoryd/src/notifications/external.rs:32-42`
- `crates/memoryd/src/notifications/external.rs:156-164`
- `crates/memoryd/src/notifications/external.rs:340-350`

**What happens:**

The config model stores `smtp_password_env`, and delivery reads the actual password from `std::env::var`. I found no config field that stores a password value. That part is correct.

The resulting `EmailMessage`, however, is public and derives `Debug` while containing `pub password: String`. There is no current production log of `EmailMessage`, but any future debug logging, assertion failure, or tracing of the message can print the SMTP password.

**Exploitability:**

This is an accidental-exposure risk rather than a current direct leak. A developer or operator could add `?message` tracing while debugging email delivery, or a failing test could render the struct.

**Impact:**

- SMTP credential exposure in logs/test output if the message is debug-printed.

**Minimal remediation:**

- Use a secret wrapper such as `secrecy::SecretString`, or make the password private.
- Implement a manual `Debug` for `EmailMessage` that redacts the password.
- Keep the existing env-var-only config contract.

### Low: Trust artifact git status avoids shell injection but still needs pathspec/PATH/timeout hardening

**Files:**

- `crates/memoryd/src/trust_artifact.rs:396-405`

**What happens:**

`memory_path_git_status` invokes `git status --porcelain -- <repo_path>` using `Command` arguments, so this is not a shell-injection bug. The `--` separator also prevents simple option injection.

Remaining hardening gaps:

- `Command::new("git")` resolves through the daemon's `PATH`.
- The memory path is still a Git pathspec, not a literal pathspec; Git pathspec magic can have surprising behavior if an unchecked/historical path reaches this call.
- There is no timeout around the external process.

**Exploitability:**

Likely local/low. A malicious or corrupted repo/index path, unusual Git config, or attacker-controlled daemon environment could cause incorrect merge-status output, excess work, or execution of an unexpected `git` binary.

**Impact:**

- Trust artifact `merge_status` can be wrong or slow.
- In a badly controlled daemon environment, `PATH` hijack could execute an unexpected binary.

**Minimal remediation:**

- Prefer a library/status API if available, or run `git` through a hardened helper.
- Add `--literal-pathspecs` or equivalent literal pathspec handling.
- Validate `repo_path` again before invoking Git.
- Use a bounded timeout and a sanitized `PATH`/absolute binary strategy for daemon-side external commands.

## Positive validations

- External Slack/email payload builders use content-free summaries and ignore event fields that could include IDs/paths/scopes in the outgoing text (`external.rs:324-371`).
- Passive routing is not configurable in `NotificationConfig`; the only config branches are OS and external (`config.rs:3-7`).
- SMTP config stores only `smtp_password_env`, and the delivery path reads the actual secret from the named env var (`config.rs:57-65`, `external.rs:156-164`).
- Server-side trust artifacts redact the target encrypted memory title and body via `SafeContent::Encrypted` (`trust_artifact.rs:146-152`).
- Supersession titles are also redacted when linked memories are encrypted (`trust_artifact.rs:325-335`).
- Focused grep found no direct `memory_substrate`/`Substrate`/read/write substrate usage in `crates/memoryd-tui` or `crates/memoryd-web`; TUI status goes through `memoryd::client::request`.
- Web routes in current scope are placeholders returning JSON `501 Not Implemented`; I did not find direct substrate fetches in the web crate.
- TUI unreachable rendering hides stale panels behind the daemon-unreachable box while disconnected (`app.rs:323-326`), and the focused socket-unreachable tests pass.

## Commands run

```bash
cargo test -p memoryd --test dispatcher --test trust_artifact
```

Result: passed (`10 + 6` tests).

```bash
cargo test -p memoryd-tui --test socket_unreachable --test panel_render
```

Result: passed (`2 + 10` tests).

```bash
cargo test -p memoryd-tui --test keymap --test resize --test trust_artifact
```

Result: passed (`10 + 2 + 4` tests).

```bash
if rg -n "memory_substrate|memory-substrate|\bSubstrate\b|read_memory|write_memory|events_log" crates/memoryd-tui crates/memoryd-web; then :; else echo 'no direct substrate hits in memoryd-tui or memoryd-web'; fi
```

Result: no direct substrate hits in `memoryd-tui` or `memoryd-web`.

```bash
rg -n "slack_payload|email_message|external_summary|passive_message|smtp_password_env|password|NotificationConfig|PassiveQueue|NotificationEvent" crates/memoryd/src/notifications crates/memoryd/src/server.rs crates/memoryd/src/protocol.rs
rg -n "SafeContent|Encrypted|memory_path_git_status|Command::new|git|source =|read_memory_envelope|events_log|query_recall_stats|query_provenance|SupersessionLink" crates/memoryd/src/trust_artifact.rs crates/memoryd-tui/src/widgets/trust_artifact.rs
```

Result: used for the static evidence above.

## Residual risk and confidence

Residual risk is moderate because I did not run the full workspace clippy/doc/test gate, and several Stream G surfaces are still skeleton or placeholder flows. Confidence is high for the findings above: they are based on direct code paths and focused passing tests. Confidence is medium for absence-of-bypass claims because the grep was scoped to `memoryd-tui` and `memoryd-web`, not a full workspace data-flow audit.
