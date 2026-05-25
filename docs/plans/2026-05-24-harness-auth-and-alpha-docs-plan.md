# Harness Auth and Alpha Docs Implementation Plan

> **⚠️ Historical / post-implementation document.** This plan was executed on commit `a7371d7`. Embedded code snippets and file-change descriptions describe the *implementation target* and may diverge from the final shipped code. Do not treat any code block in this file as current source of truth -- consult the actual files for the authoritative implementation.

**Goal:** Make `memoryd doctor`, dream harness selection, and alpha onboarding work on ordinary developer machines with current or slightly older Claude Code / Codex CLI installs.

**Architecture:** Replace hard-coded single auth probes with provider-specific ordered probe strategies that prefer current CLI surfaces and fall back only for unsupported older surfaces. Keep the public `HarnessCli` contract stable, preserve bounded/redacted diagnostics, then update the alpha docs and docs-validity gate so users follow one private socket/runtime path.

**Tech Stack:** Rust 2021, Tokio, clap, shell scripts, Markdown docs, existing `memoryd` harness/doctor tests.

---

## Current confirmed failure

- `ClaudeCodeCli::auth_probe` currently runs `claude config get auth.user` (`crates/memoryd/src/dream/harness.rs`), which can fail or hang on current Claude Code.
- `CodexCli::auth_probe` currently runs `codex auth status`, which fails on current Codex CLI; current Codex exposes `codex login status`.
- `memoryd doctor` reports a fresh install unhealthy when the installed local CLIs are actually authenticated.
- Onboarding docs still disagree on socket paths and command shape: README uses `~/memorum/.memoryd/memoryd.sock`; `docs/getting-started.md` and `docs/mcp-wiring.md` use `/tmp/memoryd.sock`; the dogfood runbook uses bare client commands after installer-based runtime setup.

## Non-goals

- Do not add new harness providers.
- Do not change prompt transport for Claude/Codex completion; completion remains stdin-only.
- Do not make unauthenticated CLIs look healthy. The goal is correct detection and actionable diagnostics.
- Do not edit historical review/plan docs unless the doc is part of current onboarding or live Stream F contract.

## Acceptance criteria

1. On a machine with current Codex CLI authenticated via ChatGPT/API key, `memoryd doctor` treats Codex as authenticated using `codex login status`.
2. On a machine with current Claude Code authenticated, `memoryd doctor` treats Claude as authenticated using `claude auth status`.
3. Older CLI surfaces are supported only when the preferred command is clearly unsupported:
   - Codex fallback: `codex auth status`.
   - Claude fallback: `claude config get auth.user`.
4. A real auth failure, timeout, or I/O error on the preferred supported command does not fall through into a legacy command that can hang or mask the real diagnostic.
5. Diagnostics stay bounded, redacted, and do not disclose the full daemon `PATH`.
6. `memoryd doctor` exits 0 when substrate is clean and at least one enabled harness is authenticated; exits 1 when no enabled harness is authenticated.
7. Current onboarding docs use a consistent private runtime socket and avoid `~` in JSON/TOML snippets that MCP clients will not expand.
8. `scripts/docs-command-validity.sh` catches stale `/tmp/memoryd.sock`, stale `cargo run -p memoryd --`, stale `codex auth status`, and stale `claude config get auth.user` in current onboarding/live-contract docs.
9. Dogfood and full gates include the docs-validity script.
10. Installer-emitted MCP snippets are pasteable on another machine: absolute socket paths only, no literal `~`, and no `/tmp/memoryd.sock`.

---

## Task 1: Add behavior tests for robust auth probe selection

**Parallel:** no
**Blocked by:** none
**Owned files:** `crates/memoryd/tests/dream_harness_cli.rs`
**Invariants:** Tests must exercise public `HarnessCli::auth_probe()` behavior through fake executable stubs on `PATH`; do not test private helper implementation details.
**Out of scope:** Production code changes.

**Files:**
- Modify: `crates/memoryd/tests/dream_harness_cli.rs`

**Step 1: Add Codex preferred-command test**

Add a test near the existing harness adapter tests:

```rust
#[test]
fn codex_auth_probe_prefers_login_status() {
    let _guard = SUBPROCESS_TEST_LOCK.lock().expect("subprocess test lock");
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'Logged in using ChatGPT\n'
  exit 0
fi
printf 'wrong command: %s\n' "$*" >&2
exit 64
"#,
                marker.display()
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(probe.is_ok(), "expected codex login status to authenticate, got {probe:?}");
        let calls = std::fs::read_to_string(marker).expect("called marker");
        assert_eq!(calls.trim(), "login status");
    });
}
```

**Step 2: Add Codex fallback-only-on-unsupported test**

```rust
#[test]
fn codex_auth_probe_falls_back_to_legacy_auth_status_only_when_login_status_is_unsupported() {
    let _guard = SUBPROCESS_TEST_LOCK.lock().expect("subprocess test lock");
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'error: unrecognized subcommand status\n' >&2
  exit 2
fi
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'authenticated\n'
  exit 0
fi
exit 64
"#,
                marker.display()
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(probe.is_ok(), "legacy codex auth status should authenticate after unsupported login status: {probe:?}");
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "login status\nauth status\n");
    });
}
```

**Step 3: Add Codex no-fallback-on-real-auth-failure test**

```rust
#[test]
fn codex_auth_probe_does_not_fallback_after_supported_login_status_auth_failure() {
    let _guard = SUBPROCESS_TEST_LOCK.lock().expect("subprocess test lock");
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> '{}'
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'not logged in; run codex login\n' >&2
  exit 1
fi
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'legacy command must not run\n' >&2
  exit 0
fi
exit 64
"#,
                marker.display()
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(!probe.is_ok(), "auth failure must remain unhealthy");
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "login status\n");
    });
}
```

**Step 4: Add exit-code-alone no-fallback regression tests**

Add two short regression tests proving fallback is marker-driven, not exit-code-driven:

- Codex: `login status` exits 2 or 64 with `not logged in; run codex login`; legacy `auth status` would exit 0 if called. Assert the probe is unhealthy and the marker file contains only `login status`.
- Claude: `auth status` exits 2 or 64 with `not authenticated; run claude auth login`; legacy `config get auth.user` would exit 0 if called. Assert the probe is unhealthy and the marker file contains only `auth status`.

This protects the key invariant: exit code 2/64 can mean command-line misuse or auth failure, so fallback is allowed only when stderr clearly says the preferred command is unsupported.

**Step 5: Add Claude preferred/fallback/no-fallback tests**

Mirror the Codex tests for Claude:

- Preferred succeeds with only `auth status` called.
- Fallback succeeds when `auth status` exits with an unsupported-command diagnostic and `config get auth.user` exits 0.
- No fallback when `auth status` exits with a real unauthenticated diagnostic.

Use stub command checks:

```sh
if [ "$1" = auth ] && [ "$2" = status ]; then ... fi
if [ "$1" = config ] && [ "$2" = get ] && [ "$3" = auth.user ]; then ... fi
```

**Step 6: Add all-known-surfaces-unsupported diagnostic test**

Add a test asserting the final `AuthProbeResult` is not `Ok`, includes a bounded operator message, and mentions that no supported auth-status command was accepted, without printing the full PATH.

**Step 7: Confirm red**

Run:

```bash
cargo test -p memoryd --test dream_harness_cli auth_probe -- --nocapture --test-threads=1
```

Expected before Task 2: at least the preferred-command tests fail because current production probes still call the old single command.

---

## Task 2: Implement ordered auth probe strategies

**Parallel:** no
**Blocked by:** Task 1
**Owned files:** `crates/memoryd/src/dream/harness.rs`
**Invariants:** Public completion command argv remains unchanged: Claude uses `claude --print`; Codex uses `codex exec [-|--json -]`; prompt bytes never enter argv; auth diagnostics remain redacted and bounded.
**Out of scope:** Doctor policy changes, docs, or new harness providers.

**Files:**
- Modify: `crates/memoryd/src/dream/harness.rs`

**Step 1: Add a small internal auth candidate type**

Near `HarnessCommandPlan`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthProbeCandidate {
    plan: HarnessCommandPlan,
    unsupported_markers: &'static [&'static str],
}
```

**Step 2: Add provider candidate builders**

Inside `impl ClaudeCodeCli`:

```rust
fn auth_probe_candidates(&self) -> Vec<AuthProbeCandidate> {
    vec![
        AuthProbeCandidate {
            plan: HarnessCommandPlan {
                program: "claude".to_owned(),
                args: vec!["auth".to_owned(), "status".to_owned()],
                prompt_transport: PromptTransport::Stdin,
            },
            unsupported_markers: &["unknown command", "unrecognized", "invalid command"],
        },
        AuthProbeCandidate {
            plan: HarnessCommandPlan {
                program: "claude".to_owned(),
                args: vec!["config".to_owned(), "get".to_owned(), "auth.user".to_owned()],
                prompt_transport: PromptTransport::Stdin,
            },
            unsupported_markers: &["unknown command", "unrecognized", "invalid command"],
        },
    ]
}
```

Inside `impl CodexCli`:

```rust
fn auth_probe_candidates(&self) -> Vec<AuthProbeCandidate> {
    vec![
        AuthProbeCandidate {
            plan: HarnessCommandPlan {
                program: "codex".to_owned(),
                args: vec!["login".to_owned(), "status".to_owned()],
                prompt_transport: PromptTransport::Stdin,
            },
            unsupported_markers: &["unrecognized subcommand", "unknown command", "invalid command"],
        },
        AuthProbeCandidate {
            plan: HarnessCommandPlan {
                program: "codex".to_owned(),
                args: vec!["auth".to_owned(), "status".to_owned()],
                prompt_transport: PromptTransport::Stdin,
            },
            unsupported_markers: &["unrecognized subcommand", "unknown command", "invalid command"],
        },
    ]
}
```

**Step 3: Replace single-command calls**

Change Claude/Codex `auth_probe()` bodies from `auth_probe(plan, ...)` to:

```rust
auth_probe_any(self.auth_probe_candidates(), self.path_env.clone(), CLAUDE_ENV_ALLOWLIST).await
```

and similarly for Codex with `CODEX_ENV_ALLOWLIST`.

**Step 4: Implement `auth_probe_any` with one testable loop**

Keep `auth_probe(...)` as the one-command primitive. Implement the candidate loop once in a runner-injected helper so timeout/error branches are easy to unit-test without slow subprocess sleeps:

```rust
async fn auth_probe_any_with_runner<F, Fut>(
    candidates: Vec<AuthProbeCandidate>,
    mut runner: F,
) -> AuthProbeResult
where
    F: FnMut(HarnessCommandPlan) -> Fut,
    Fut: std::future::Future<Output = AuthProbeResult>,
{
    let mut unsupported = Vec::new();
    for candidate in candidates {
        let AuthProbeCandidate { plan, unsupported_markers } = candidate;
        let command_label = command_label(&plan);
        match runner(plan).await {
            AuthProbeResult::Ok => return AuthProbeResult::Ok,
            AuthProbeResult::AuthFailed { stderr_tail, .. }
                if is_unsupported_auth_surface(&stderr_tail, unsupported_markers) =>
            {
                unsupported.push(format!("{command_label}: {stderr_tail}"));
                continue;
            }
            AuthProbeResult::AuthFailed { exit_code, stderr_tail } => {
                return AuthProbeResult::AuthFailed {
                    exit_code,
                    stderr_tail: format!("{command_label} failed: {stderr_tail}"),
                };
            }
            AuthProbeResult::Timeout => {
                return AuthProbeResult::Error { message: format!("{command_label} timed out") };
            }
            AuthProbeResult::Error { message } => {
                return AuthProbeResult::Error { message: format!("{command_label} error: {message}") };
            }
            AuthProbeResult::CliMissing { which, path } => return AuthProbeResult::CliMissing { which, path },
        }
    }

    AuthProbeResult::Error {
        message: format!(
            "no supported auth status command was accepted; tried {}",
            summarize_unsupported_attempts(&unsupported)
        ),
    }
}
```

Then make the production helper a thin wrapper:

```rust
async fn auth_probe_any(
    candidates: Vec<AuthProbeCandidate>,
    path_env: Option<OsString>,
    env_allowlist: &[&str],
) -> AuthProbeResult {
    auth_probe_any_with_runner(candidates, |plan| {
        auth_probe(plan, path_env.clone(), env_allowlist)
    })
    .await
}
```

**Step 5: Add helper functions**

Add private helpers:

```rust
const AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS: usize = 4096;

fn command_label(plan: &HarnessCommandPlan) -> String {
    if plan.args.is_empty() {
        plan.program.clone()
    } else {
        format!("{} {}", plan.program, plan.args.join(" "))
    }
}

fn is_unsupported_auth_surface(
    stderr_tail: &str,
    markers: &[&str],
) -> bool {
    let lower = stderr_tail.to_ascii_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

fn summarize_unsupported_attempts(attempts: &[String]) -> String {
    truncate_for_auth_diagnostic(&attempts.join("; "), AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS)
}

fn truncate_for_auth_diagnostic(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}
```

Use a char-counted summary cap; do not slice Rust strings by byte offsets for these joined diagnostics.

**Step 6: Add private loop tests for timeout and unicode diagnostics**

Tests call `auth_probe_any_with_runner(...)` with a fake closure. This avoids duplicated control flow while keeping subprocess-facing integration tests in Task 1.

Add unit tests in `harness.rs` for:

- Unsupported marker on the preferred command runs the legacy candidate.
- Exit 2/64 without an unsupported marker does not run the legacy candidate.
- Timeout on the preferred command does not run the legacy candidate.
- A long unsupported diagnostic containing multibyte Unicode is summarized without panic and remains bounded.

Keep the public integration tests from Task 1 as the primary contract; these unit tests only cover branches that are too slow or awkward to force through subprocesses.

**Step 7: Run focused tests**

```bash
cargo test -p memoryd --lib auth_probe_any -- --nocapture
cargo test -p memoryd --test dream_harness_cli auth_probe -- --nocapture --test-threads=1
cargo test -p memoryd --test dream_auth_diagnostic -- --nocapture
```

Expected: all pass.

---

## Task 3: Add doctor-level regression coverage with stub CLIs

**Parallel:** no
**Blocked by:** Task 2
**Owned files:** `crates/memoryd/tests/cli_contract.rs`
**Invariants:** `memoryd doctor` health rule stays: clean substrate plus at least one authenticated enabled harness is healthy; missing/unauthenticated secondary harness can be a warning but not a hard failure when another enabled harness works.
**Out of scope:** Changing doctor JSON shape beyond messages naturally produced by improved probes.

**Files:**
- Modify: `crates/memoryd/tests/cli_contract.rs`

**Step 1: Add helper to write executable stubs**

At bottom of the test file, add a Unix helper equivalent to the one in `dream_harness_cli.rs` if none exists:

```rust
#[cfg(unix)]
fn write_executable(path: impl AsRef<std::path::Path>, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path.as_ref(), contents).expect("write executable stub");
    let mut permissions = std::fs::metadata(path.as_ref()).expect("stub metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path.as_ref(), permissions).expect("mark stub executable");
}
```

**Step 2: Add a healthy-with-current-Codex-stub test**

```rust
#[test]
fn doctor_is_healthy_when_current_codex_login_status_is_authenticated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("bin dir");

    write_executable(
        bin_dir.join("codex"),
        r#"#!/bin/sh
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'Logged in using ChatGPT\n'
  exit 0
fi
printf 'unexpected codex args: %s\n' "$*" >&2
exit 64
"#,
    );

    init_test_substrate(&repo, &runtime, "dev_doctor_codex");

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["doctor", "--repo"])
        .arg(&repo)
        .arg("--runtime")
        .arg(&runtime)
        .env("PATH", &bin_dir)
        .output()
        .expect("run memoryd doctor");

    assert_eq!(output.status.code(), Some(0), "healthy doctor should exit 0: {}", String::from_utf8_lossy(&output.stdout));
    let stdout = String::from_utf8(output.stdout).expect("doctor stdout utf8");
    assert!(stdout.contains("\"healthy\": true"), "{stdout}");
    assert!(!stdout.contains("codex auth status"), "current Codex probe should not use stale auth command: {stdout}");
}
```

Refactor the existing substrate init block into `init_test_substrate(repo, runtime, device_id)` to avoid duplication.

**Step 3: Add a healthy-with-current-Claude-stub test**

Same shape with a `claude` stub that succeeds only for `auth status`. Assert exit 0 and no stale `config get auth.user` diagnostic.

**Step 4: Add unsupported fallback doctor test**

Use a `codex` stub where `login status` exits 2 with `unrecognized subcommand`, `auth status` exits 0. Assert doctor healthy.

**Step 5: Run focused doctor tests**

```bash
cargo test -p memoryd --test cli_contract doctor_ -- --nocapture --test-threads=1
cargo test -p memoryd --lib doctor_health -- --nocapture
```

Expected: all pass.

---

## Task 4: Verify live local CLI behavior without making CI require credentials

**Parallel:** no
**Blocked by:** Tasks 2, 3, and 5
**Owned files:** none
**Invariants:** Live smoke instructions must not require provider keys in CI; they are operator/local alpha checks only.
**Out of scope:** Adding credentials to tests or CI.

**Files:** none

**Step 1: Manual validation command for implementer**

After the auth implementation, doctor coverage, and dogfood runbook updates are complete, run locally when at least one CLI is authenticated:

```bash
tmp="$(mktemp -d -t memorum-auth-smoke.XXXXXX)"
repo="$tmp/repo"
runtime="$tmp/runtime"
socket="$runtime/memoryd.sock"
mkdir -p "$repo" "$runtime"
target/debug/memoryd serve --init --repo "$repo" --runtime "$runtime" --socket "$socket" >"$tmp/serve.log" 2>&1 & pid=$!
for _ in $(seq 1 50); do [ -S "$socket" ] && break; sleep 0.1; done
target/debug/memoryd doctor --repo "$repo" --runtime "$runtime"
code=$?
kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
rm -rf "$tmp"
exit "$code"
```

Expected on an authenticated local machine: exit 0. If no harness is authenticated, exit 1 with actionable diagnostics.

---

## Task 5: Update current onboarding docs to one socket/runtime story

**Parallel:** yes
**Blocked by:** none
**Owned files:** `README.md`, `docs/getting-started.md`, `docs/mcp-wiring.md`, `docs/runbooks/dogfooding-day-one.md`
**Invariants:** Canonical MCP bridge shape remains `memoryd mcp --socket <PATH>`; installer-emitted snippets should be treated as source of truth for alpha users.
**Out of scope:** Historical `docs/reviews/**`, historical `docs/plans/**`, and old stream v0.1/v0.2 specs unless explicitly marked current.

**Files:**
- Modify: `README.md`
- Modify: `docs/getting-started.md`
- Modify: `docs/mcp-wiring.md`
- Modify: `docs/runbooks/dogfooding-day-one.md`

**Step 1: README quickstart and MCP snippet**

Define the private runtime/socket variables before the quickstart commands:

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
```

Then replace README shell examples that use literal `~/memorum` socket paths:

```bash
mkdir -p "$MEMORUM_REPO"
memoryd serve --init --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --socket "$MEMORUM_SOCKET"
memoryd status --socket "$MEMORUM_SOCKET"
memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
memoryd mcp --socket "$MEMORUM_SOCKET"
memoryd recall startup-block --socket "$MEMORUM_SOCKET" --cwd "$PWD" --session-id smoke --harness codex
memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137
memoryd ui --socket "$MEMORUM_SOCKET"
```

Replace JSON snippets that contain `~` with an absolute placeholder and warning:

~~~markdown
Use the absolute socket path printed by `scripts/install-memorum.sh`; most MCP clients do not expand `~` inside JSON/TOML.

```json
{
  "mcpServers": {
    "memorum": {
      "command": "memoryd",
      "args": ["mcp", "--socket", "/Users/you/memorum/.memoryd/memoryd.sock"]
    }
  }
}
```
~~~

**Step 2: `docs/getting-started.md`**

- Replace `cargo run -p memoryd --` with `cargo run --bin memoryd --`.
- Define shell variables early:

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
```

- Use those variables in shell commands.
- Use `/Users/you/memorum/.memoryd/memoryd.sock` in JSON examples, with explicit instruction to replace it with `echo "$MEMORUM_SOCKET"` output.
- Update tool list to all ten current MCP tools or say “such as” and avoid claiming a complete list unless it is generated from tests.

**Step 3: `docs/mcp-wiring.md`**

- Replace every `/tmp/memoryd.sock` with the private runtime socket story.
- Keep command shape `memoryd mcp --socket <absolute path>`.
- Add the “no `~` in JSON/TOML” warning.
- For Codex TOML, show:

```toml
[mcp.memorum]
command = "memoryd"
args = ["mcp", "--socket", "/Users/you/memorum/.memoryd/memoryd.sock"]
```

**Step 4: Dogfood runbook**

After installer command, add:

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
```

Then make all CLI examples explicit:

```bash
memoryd status --socket "$MEMORUM_SOCKET"
memoryd write-note --socket "$MEMORUM_SOCKET" "I dogfooded Memorum on 2026-05-24."
memoryd search --socket "$MEMORUM_SOCKET" "dogfood"
memoryd web enable --socket "$MEMORUM_SOCKET"
memoryd reality-check run --socket "$MEMORUM_SOCKET"
memoryd ui --socket "$MEMORUM_SOCKET" --panel 9
```

For MCP config, prefer installer-printed `--socket` snippet over hand-written `--runtime` snippets.

Add a subsection under Troubleshooting or Gates:

~~~markdown
## Optional local harness-auth smoke

If `codex` or `claude` is installed and authenticated in your shell, `memoryd doctor --repo "$HOME/memorum" --runtime "$HOME/memorum/.memoryd"` should exit 0 when the substrate is otherwise clean.

Useful direct checks:

```bash
codex login status   # current Codex CLI
claude auth status   # current Claude Code CLI
```

Older CLIs may use legacy auth status commands; Memorum falls back only when the preferred current command is unsupported.
~~~

---

## Task 6: Update live Stream F/system docs for auth and bootstrap reality

**Parallel:** yes
**Blocked by:** Task 2
**Owned files:** `docs/specs/stream-f-dreaming-v0.3.md`, `docs/specs/system-v0.2.md`
**Invariants:** Do not rewrite historical specs wholesale; add/update focused notes so current readers do not implement stale auth probes or hunt for an absent `memoryd init` command.
**Out of scope:** Old stream-f v0.1/v0.2 docs unless explicitly linked as current.

**Files:**
- Modify: `docs/specs/stream-f-dreaming-v0.3.md`
- Modify: `docs/specs/system-v0.2.md`

**Step 1: Stream F harness auth table**

Update the auth probe rows:

| Adapter | Completion command | Auth probe |
|---|---|---|
| `ClaudeCodeCli` | `claude --print` with prompt on stdin | Prefer `claude auth status`; fallback to `claude config get auth.user` only when the preferred command is unsupported. |
| `CodexCli` | `codex exec --json -` or `codex exec -` with prompt on stdin | Prefer `codex login status`; fallback to `codex auth status` only when the preferred command is unsupported. |

Add invariant: auth failure/timeout on a supported preferred command must not fall through to legacy fallback.

Also update any nearby trait/prose/comment text that names `claude config get auth.user` or `codex auth status` as the primary auth probe. It should describe ordered provider-specific auth probes: preferred current command first, legacy command only when the preferred surface is unsupported, and no fallback on auth failure or timeout.

**Step 2: System bootstrap status note**

Where `system-v0.2.md` describes `memoryd init`, add a current implementation note:

```markdown
Implementation note as of 2026-05-24: alpha bootstrap is `memoryd serve --init` plus `scripts/install-memorum.sh`; the full interactive `memoryd init` wizard remains a release-shape target and is not the current alpha entrypoint.
```

Keep the release-shape content if it is intentionally aspirational, but make the current alpha path unambiguous.

---

## Task 7: Harden docs-validity and gate wiring

**Parallel:** no
**Blocked by:** Tasks 5, 6, and 8
**Owned files:** `scripts/docs-command-validity.sh`, `scripts/check-fast.sh`, `scripts/check.sh`
**Invariants:** The docs-validity script should scan current docs only; do not fail on intentionally historical reviews/plans. The dogfood gate should catch onboarding regressions before alpha installs.
**Out of scope:** Broad specgate ownership cleanup.

**Files:**
- Modify: `scripts/docs-command-validity.sh`
- Modify: `scripts/check-fast.sh`
- Modify: `scripts/check.sh`

**Step 1: Expand current-doc paths**

Change:

```bash
paths=(README.md docs/runbooks docs/api docs/dev)
```

to:

```bash
paths=(
  README.md
  docs/getting-started.md
  docs/mcp-wiring.md
  docs/runbooks
  docs/api
  docs/dev
  docs/specs/system-v0.2.md
  docs/specs/stream-f-dreaming-v0.3.md
)
```

**Step 2: Add stale-auth checks**

Add checks that fail only when the legacy command appears as the current/preferred path, while allowing lines that explicitly label it fallback/legacy/older-CLI support:

```bash
stale_codex="$(rg -n 'codex auth status' "${paths[@]}" 2>/dev/null | rg -vi 'fallback|legacy|older cli' || true)"
if [ -n "$stale_codex" ]; then
  printf '%s\n' "$stale_codex" >&2
  echo "docs contain stale Codex auth probe; use codex login status as preferred current command" >&2
  failed=1
fi
stale_claude="$(rg -n 'claude config get auth\.user' "${paths[@]}" 2>/dev/null | rg -vi 'fallback|legacy|older cli' || true)"
if [ -n "$stale_claude" ]; then
  printf '%s\n' "$stale_claude" >&2
  echo "docs contain stale Claude auth probe as preferred command; use claude auth status as preferred current command" >&2
  failed=1
fi
tilde_socket="$(
  {
    rg -n -- '--socket[[:space:]]+~/' "${paths[@]}" 2>/dev/null || true
    rg -n -- '"~/[^"]*memoryd\.sock"' "${paths[@]}" 2>/dev/null || true
    rg -n -- "'~/[^']*memoryd\.sock'" "${paths[@]}" 2>/dev/null || true
  } | sort -u
)"
if [ -n "$tilde_socket" ]; then
  printf '%s\n' "$tilde_socket" >&2
  echo "docs contain MCP config with unexpanded ~; use an absolute path placeholder" >&2
  failed=1
fi
```

Keep the matcher simple and explicit; if it becomes brittle, prefer moving legacy examples into prose that says fallback/legacy on the same line.

**Step 3: Wire docs-validity into gates**

In `scripts/check-fast.sh`, add after shell syntax:

```bash
phase "docs command validity"
./scripts/docs-command-validity.sh
```

In `scripts/check.sh`, add a parallel phase:

```bash
run_parallel docs-validity ./scripts/docs-command-validity.sh
```

**Step 4: Expand shell syntax check**

In `scripts/check-fast.sh`, include:

```bash
bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh scripts/install-launchd.sh scripts/docs-command-validity.sh scripts/install-memorum.test.sh scripts/install-launchd.test.sh
```

**Step 5: Coordinate with installer tests**

Task 8 owns installer test edits. This task should only run those tests to prove the gate wiring did not regress.

**Step 6: Run scripts**

```bash
bash -n scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh scripts/install-memorum.sh scripts/install-launchd.sh scripts/docs-command-validity.sh scripts/install-memorum.test.sh scripts/install-launchd.test.sh
bash scripts/docs-command-validity.sh
bash scripts/install-memorum.test.sh
bash scripts/install-launchd.test.sh
```

Expected: all pass.

---

## Task 8: Canonicalize installer paths for pasteable MCP snippets

**Parallel:** no
**Blocked by:** Task 5
**Owned files:** `scripts/install-memorum.sh`, `scripts/install-memorum.test.sh`
**Invariants:** Installer output should be directly pasteable into MCP configs. Dry-run should remain non-destructive except for temporary/test-owned directories if canonicalization requires path existence.
**Out of scope:** Replacing the installer with `memoryd init`.

**Files:**
- Modify: `scripts/install-memorum.sh`
- Modify: `scripts/install-memorum.test.sh`

**Step 1: Canonicalize repo/runtime/socket before printing snippets**

Add helper:

```bash
absolute_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$(pwd -P)" "$1" ;;
  esac
}
```

After defaulting `repo`, `runtime`, and `socket`, canonicalize enough to avoid relative/tilde snippets:

```bash
repo="$(absolute_path "$repo")"
runtime="$(absolute_path "$runtime")"
socket="$(absolute_path "$socket")"
```

Implementation caution: unexpanded literal `~` is just a normal character in shell variables; docs should pass `$HOME/...` or unquoted `~/...`. Before canonicalizing, the installer must reject any path containing literal `~` so it never prints non-pasteable MCP snippets:

```bash
reject_literal_tilde() {
  case "$1" in
    *'~'*)
      echo "error: literal ~ is not expanded here; pass \$HOME/... or an absolute path" >&2
      exit 2
      ;;
  esac
}

reject_literal_tilde "$repo"
reject_literal_tilde "$runtime"
reject_literal_tilde "$socket"
```

**Step 2: Test relative path output**

In `scripts/install-memorum.test.sh`, invoke a dry-run from a temp cwd with relative `--repo repo --runtime runtime` and assert the JSON snippet contains the absolute temp path.

**Step 3: Test literal tilde warning**

Capture stdout and stderr separately. Add a dry-run assertion that `--repo '~/memorum'` exits 2, stderr contains the literal-tilde error, and stdout contains neither the MCP snippet nor any socket path containing `~`.

---

## Final verification plan

Run in this order:

```bash
cargo test -p memoryd --lib auth_probe_any -- --nocapture
cargo test -p memoryd --test dream_harness_cli auth_probe -- --nocapture --test-threads=1
cargo test -p memoryd --test dream_auth_diagnostic -- --nocapture
cargo test -p memoryd --test cli_contract doctor_ -- --nocapture --test-threads=1
cargo test -p memoryd --lib doctor_health -- --nocapture
bash scripts/docs-command-validity.sh
bash scripts/install-memorum.test.sh
bash scripts/install-launchd.test.sh
cargo fmt --all -- --check
cargo clippy -p memoryd --all-targets --all-features -- -D warnings
bash scripts/check-dogfood.sh
bash scripts/check.sh
```

If `bash scripts/check.sh` exposes an unrelated pre-existing environmental blocker, record the exact failing phase and command in the implementation handoff; do not silently downgrade the gate.

Optional local authenticated smoke:

```bash
codex login status || true
claude auth status || true
# then run the temporary daemon doctor smoke from Task 4
```

Broader confidence before inviting alpha users:

```bash
cargo test --workspace --all-targets --all-features
cd crates/memoryd-web/frontend && pnpm run check:all
```

## Implementation sequencing

1. Task 1 red tests.
2. Task 2 implementation.
3. Task 3 doctor regression tests.
4. Task 5 docs cleanup.
5. Task 6 live-contract docs cleanup.
6. Task 8 installer canonicalization.
7. Task 7 docs-validity/gate wiring.
8. Task 4 manual live smoke.
9. Final verification.
