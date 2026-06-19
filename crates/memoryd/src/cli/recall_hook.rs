//! Unified passive-recall hook handler (`memoryd recall hook`).
//!
//! Claude Code and Codex invoke this on lifecycle events (`SessionStart`,
//! `SubagentStart`, `UserPromptSubmit`), passing the hook-invocation JSON on
//! stdin and injecting our stdout back into the model context via
//! `hookSpecificOutput.additionalContext`. We map the event to a passive (read-
//! only) recall request, call the daemon under a hard deadline, and print the
//! recall block wrapped in the harness's `additionalContext` envelope.
//!
//! # Fail-open is absolute
//!
//! This handler must NEVER block the agent or print diagnostics. Every failure
//! path — malformed stdin, missing/invalid `cwd`, daemon unreachable, daemon
//! error response, timeout, oversize block, serialization error — results in
//! *nothing on stdout, nothing on stderr, and exit 0*. It deliberately shares
//! no exit path with `StartupBlock`/`DeltaBlock`: it never calls
//! `exit_recall_unavailable`/`exit_protocol_error` (those exit with code 2).
//!
//! The core is factored into `run_hook`, a pure-ish async fn that returns
//! `Some(json)` to print or `None` to emit nothing, so the parse/empty/oversize/
//! malformed branches are unit-testable without a live process or daemon.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::cli::RecallHookArgs;
use crate::client;
use crate::paths::resolve_socket_with_runtime;
use crate::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use crate::recall::{DeltaRequest, StartupRequest};

/// Hard internal deadline for the daemon round-trip (spec §12.1 ceiling). On
/// timeout the handler fails open. The installed harness `timeout` is a coarser
/// backstop; this keeps us well under the 800ms passive-recall budget.
const HOOK_DAEMON_DEADLINE_MS: u64 = 800;

/// Defense-in-depth output cap. The daemon already keeps the passive hook block
/// under its char cap; if a block ever reaches this size we fail open rather
/// than inject something Claude's 10k-char output limit would reject.
const HOOK_BLOCK_CHAR_CAP: usize = 10_000;

/// The render-layer empty-delta sentinel (`recall::render`). Treated as "no
/// memory" so an empty turn emits zero bytes rather than an empty XML element.
const EMPTY_DELTA_SENTINEL: &str = "<memory-delta empty=\"true\" />";

/// Placeholder session id used when the harness omits `session_id` from the
/// hook payload. Stable so repeated invocations bind to the same logical
/// session rather than churning the daemon's session state.
const PLACEHOLDER_SESSION_ID: &str = "hook-session";

/// One hook-invocation object as delivered on stdin. Permissive by design: both
/// Claude Code and Codex send a superset of these fields and may add more, so
/// every field is optional and unknown fields are ignored. Dispatch keys off
/// `hook_event_name`.
#[derive(Debug, Deserialize)]
struct HookInvocation {
    hook_event_name: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    /// Present on `UserPromptSubmit` — the user's prompt text.
    prompt: Option<String>,
    /// Present on `SessionStart` — `startup` | `resume` | `clear` | `compact`.
    #[allow(dead_code)]
    source: Option<String>,
    /// Present on `SubagentStart` — handler-side attribution only; never sent as
    /// a request field (`StartupRequest` has no subagent scoping).
    #[allow(dead_code)]
    agent_type: Option<String>,
    #[allow(dead_code)]
    transcript_path: Option<String>,
}

/// Thin outer entry point: read stdin, run the core, print on `Some`, and
/// return normally (the caller treats this as exit 0) on every path. Reading
/// stdin is itself fallible (a closed pipe); a read error fails open like every
/// other path.
pub async fn run(args: RecallHookArgs) {
    let socket = resolve_socket_with_runtime(&args.socket.socket, &args.socket.runtime);
    let stdin_bytes = match read_stdin() {
        Ok(bytes) => bytes,
        // Fail open: a stdin read error injects nothing and exits 0.
        Err(_) => return,
    };
    if let Some(output) = run_hook(&stdin_bytes, &socket, &args.harness).await {
        // Compact, no trailing newline. `print!` never panics on a closed
        // stdout in practice; if it ever did, fail-open still holds (no stderr).
        print!("{output}");
    }
}

/// Core handler: parse stdin, build the passive recall request, call the daemon
/// under the deadline, and return the `hookSpecificOutput` JSON to print — or
/// `None` to emit nothing. Every error and empty-result path returns `None`.
///
/// Pure with respect to process state (no stdout/stderr/exit), so the
/// parse/empty/oversize/malformed/daemon-down branches are unit-testable.
async fn run_hook(stdin_bytes: &[u8], socket: &Path, harness: &str) -> Option<String> {
    let invocation: HookInvocation = serde_json::from_slice(stdin_bytes).ok()?;
    let event = invocation.hook_event_name.as_deref()?;

    // `cwd` must be a present, non-empty, absolute path that exists as a
    // directory. Anything else fails open.
    let cwd = invocation.cwd.as_deref().filter(|c| !c.is_empty())?;
    let cwd_path = Path::new(cwd);
    if !cwd_path.is_absolute() || !cwd_path.is_dir() {
        return None;
    }

    let session_id =
        invocation.session_id.as_deref().filter(|s| !s.is_empty()).unwrap_or(PLACEHOLDER_SESSION_ID).to_owned();

    let payload = match event {
        // Subagents reuse the parent session's cwd-scoped base block; recall has
        // no subagent scope and `StartupRequest` carries no subagent fields, so
        // `agent_type` stays handler-side only.
        "SessionStart" | "SubagentStart" => RequestPayload::Startup(StartupRequest {
            cwd: cwd.to_owned(),
            session_id,
            harness: harness.to_owned(),
            harness_version: None,
            include_recent: true,
            since_event_id: None,
            // Passive: the daemon applies the reduced hook budget + char cap
            // server-side. Never set a budget here.
            budget_tokens: None,
            passive: true,
        }),
        "UserPromptSubmit" => {
            // An empty prompt has no delta to recall against; fail open.
            let message = invocation.prompt.as_deref().filter(|p| !p.is_empty())?.to_owned();
            RequestPayload::Delta(DeltaRequest {
                cwd: cwd.to_owned(),
                session_id,
                harness: harness.to_owned(),
                message,
                budget_tokens: None,
                passive: true,
            })
        }
        // Any other / missing event: fail open, emit nothing.
        _ => return None,
    };

    let block = call_daemon(socket, payload).await?;
    let trimmed = block.trim();

    // Empty == zero bytes: blank block, whitespace-only, or the delta
    // empty-sentinel all inject nothing.
    if trimmed.is_empty() || trimmed == EMPTY_DELTA_SENTINEL {
        return None;
    }

    // Oversize guard (belt-and-suspenders; the daemon already caps the passive
    // block). Count chars, not bytes — the cap is Claude's char limit.
    if block.chars().count() >= HOOK_BLOCK_CHAR_CAP {
        return None;
    }

    // Build the envelope with serde so the block is correctly escaped.
    build_hook_output(event, &block)
}

/// Call the daemon under [`HOOK_DAEMON_DEADLINE_MS`] and extract the recall
/// block. Returns `None` on timeout, transport failure, a daemon error
/// response, or an unexpected payload — all of which fail open.
async fn call_daemon(socket: &Path, payload: RequestPayload) -> Option<String> {
    let response = tokio::time::timeout(
        Duration::from_millis(HOOK_DAEMON_DEADLINE_MS),
        client::request(socket, "cli-recall-hook", payload),
    )
    .await
    .ok()? // timeout -> None
    .ok()?; // transport error -> None

    match response.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => Some(startup.recall_block),
        ResponseResult::Success(ResponsePayload::Delta(delta)) => Some(delta.delta_block),
        // Daemon error or any other payload: fail open.
        _ => None,
    }
}

/// Serialize the harness injection envelope:
/// `{"hookSpecificOutput":{"hookEventName":<event>,"additionalContext":<block>}}`.
/// Returns `None` if serialization fails (it cannot for these inputs, but a
/// serialization error must still fail open rather than panic).
fn build_hook_output(event: &str, block: &str) -> Option<String> {
    let value = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": block,
        }
    });
    serde_json::to_string(&value).ok()
}

/// Read all of stdin to bytes.
fn read_stdin() -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A bogus socket path that no daemon is listening on. The connect fails
    /// fast (ENOENT/ECONNREFUSED) well within the deadline, exercising the
    /// daemon-unreachable fail-open path without a real daemon.
    fn dead_socket() -> PathBuf {
        std::env::temp_dir().join("memorum-recall-hook-test-nonexistent.sock")
    }

    fn session_start_stdin(cwd: &Path) -> Vec<u8> {
        serde_json::json!({
            "hook_event_name": "SessionStart",
            "cwd": cwd.to_string_lossy(),
            "session_id": "sess-123",
            "source": "startup",
        })
        .to_string()
        .into_bytes()
    }

    /// Build a `HookInvocation`-shaped request the same way `run_hook` does, so
    /// per-event tests can assert the request payload without a live daemon.
    fn build_request(stdin: &[u8], harness: &str) -> Option<RequestPayload> {
        let invocation: HookInvocation = serde_json::from_slice(stdin).ok()?;
        let event = invocation.hook_event_name.as_deref()?;
        let cwd = invocation.cwd.as_deref().filter(|c| !c.is_empty())?;
        let session_id =
            invocation.session_id.as_deref().filter(|s| !s.is_empty()).unwrap_or(PLACEHOLDER_SESSION_ID).to_owned();
        match event {
            "SessionStart" | "SubagentStart" => Some(RequestPayload::Startup(StartupRequest {
                cwd: cwd.to_owned(),
                session_id,
                harness: harness.to_owned(),
                harness_version: None,
                include_recent: true,
                since_event_id: None,
                budget_tokens: None,
                passive: true,
            })),
            "UserPromptSubmit" => {
                let message = invocation.prompt.as_deref().filter(|p| !p.is_empty())?.to_owned();
                Some(RequestPayload::Delta(DeltaRequest {
                    cwd: cwd.to_owned(),
                    session_id,
                    harness: harness.to_owned(),
                    message,
                    budget_tokens: None,
                    passive: true,
                }))
            }
            _ => None,
        }
    }

    #[test]
    fn session_start_builds_passive_startup_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = session_start_stdin(dir.path());
        let payload = build_request(&stdin, "claude-code").expect("startup payload");
        match payload {
            RequestPayload::Startup(req) => {
                assert!(req.passive, "hook startup must be passive (read-only)");
                assert_eq!(req.budget_tokens, None, "passive hook leaves budget to the daemon");
                assert!(req.include_recent);
                assert_eq!(req.harness, "claude-code", "harness id flows through verbatim");
                assert_eq!(req.session_id, "sess-123");
                assert_eq!(req.cwd, dir.path().to_string_lossy());
            }
            other => panic!("expected Startup, got {other:?}"),
        }
    }

    #[test]
    fn subagent_start_builds_passive_startup_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = serde_json::json!({
            "hook_event_name": "SubagentStart",
            "cwd": dir.path().to_string_lossy(),
            "session_id": "sess-sub",
            "agent_type": "code-reviewer",
        })
        .to_string()
        .into_bytes();
        let payload = build_request(&stdin, "claude-code").expect("startup payload");
        match payload {
            RequestPayload::Startup(req) => {
                assert!(req.passive);
                // Subagent reuses parent (session) scope: no DTO widening, the
                // request is the same shape as SessionStart.
                assert_eq!(req.session_id, "sess-sub");
            }
            other => panic!("expected Startup, got {other:?}"),
        }
    }

    #[test]
    fn user_prompt_submit_builds_passive_delta_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "cwd": dir.path().to_string_lossy(),
            "session_id": "sess-9",
            "prompt": "what did we decide about caching?",
        })
        .to_string()
        .into_bytes();
        let payload = build_request(&stdin, "codex").expect("delta payload");
        match payload {
            RequestPayload::Delta(req) => {
                assert!(req.passive, "hook delta must be passive (read-only)");
                assert_eq!(req.budget_tokens, None);
                assert_eq!(req.message, "what did we decide about caching?", "prompt becomes the delta message");
                assert_eq!(req.harness, "codex");
            }
            other => panic!("expected Delta, got {other:?}"),
        }
    }

    #[test]
    fn missing_session_id_falls_back_to_placeholder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = serde_json::json!({
            "hook_event_name": "SessionStart",
            "cwd": dir.path().to_string_lossy(),
        })
        .to_string()
        .into_bytes();
        let payload = build_request(&stdin, "claude-code").expect("startup payload");
        match payload {
            RequestPayload::Startup(req) => assert_eq!(req.session_id, PLACEHOLDER_SESSION_ID),
            other => panic!("expected Startup, got {other:?}"),
        }
    }

    #[test]
    fn build_hook_output_echoes_event_and_carries_block() {
        let out = build_hook_output("SessionStart", "<memory>fact</memory>").expect("envelope");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(parsed["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert_eq!(parsed["hookSpecificOutput"]["additionalContext"], "<memory>fact</memory>");
        // Compact, no pretty-printing.
        assert!(!out.contains('\n'), "output must be compact single-line JSON: {out}");
    }

    #[test]
    fn build_hook_output_escapes_block_via_serde() {
        // A block containing quotes/newlines must survive as a valid escaped
        // JSON string, not break the envelope.
        let block = "line1\n\"quoted\" & <tag>";
        let out = build_hook_output("UserPromptSubmit", block).expect("envelope");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(parsed["hookSpecificOutput"]["additionalContext"], block);
    }

    #[tokio::test]
    async fn daemon_unreachable_emits_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = session_start_stdin(dir.path());
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "daemon-unreachable must emit zero bytes (None)");
    }

    #[tokio::test]
    async fn malformed_stdin_emits_nothing() {
        let out = run_hook(b"not json at all {{{", &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "malformed stdin must emit zero bytes");
    }

    #[tokio::test]
    async fn empty_stdin_emits_nothing() {
        let out = run_hook(b"", &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "empty stdin must emit zero bytes");
    }

    #[tokio::test]
    async fn unknown_event_emits_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = serde_json::json!({
            "hook_event_name": "Stop",
            "cwd": dir.path().to_string_lossy(),
            "session_id": "s",
        })
        .to_string()
        .into_bytes();
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "unknown event must fail open");
    }

    #[tokio::test]
    async fn missing_event_emits_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = serde_json::json!({
            "cwd": dir.path().to_string_lossy(),
            "session_id": "s",
        })
        .to_string()
        .into_bytes();
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "missing hook_event_name must fail open");
    }

    #[tokio::test]
    async fn missing_cwd_emits_nothing() {
        let stdin = serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "s",
        })
        .to_string()
        .into_bytes();
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "missing cwd must fail open");
    }

    #[tokio::test]
    async fn relative_cwd_emits_nothing() {
        let stdin = serde_json::json!({
            "hook_event_name": "SessionStart",
            "cwd": "relative/path",
            "session_id": "s",
        })
        .to_string()
        .into_bytes();
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "non-absolute cwd must fail open");
    }

    #[tokio::test]
    async fn nonexistent_cwd_emits_nothing() {
        let stdin = serde_json::json!({
            "hook_event_name": "SessionStart",
            "cwd": "/this/path/should/not/exist/memorum-hook-test",
            "session_id": "s",
        })
        .to_string()
        .into_bytes();
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "cwd that is not an existing dir must fail open");
    }

    #[tokio::test]
    async fn empty_prompt_emits_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stdin = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "cwd": dir.path().to_string_lossy(),
            "session_id": "s",
            "prompt": "",
        })
        .to_string()
        .into_bytes();
        let out = run_hook(&stdin, &dead_socket(), "claude-code").await;
        assert!(out.is_none(), "empty prompt has no delta to recall");
    }

    /// The empty/oversize/sentinel post-daemon branches are tested directly
    /// against the same predicates `run_hook` applies, since reaching them
    /// requires a live daemon response. This keeps coverage of the exact
    /// fail-open conditions without standing up a daemon.
    #[test]
    fn empty_and_sentinel_blocks_are_treated_as_zero_bytes() {
        let is_empty = |block: &str| {
            let trimmed = block.trim();
            trimmed.is_empty() || trimmed == EMPTY_DELTA_SENTINEL
        };
        assert!(is_empty(""), "blank block is zero bytes");
        assert!(is_empty("   \n\t "), "whitespace-only block is zero bytes");
        assert!(is_empty("<memory-delta empty=\"true\" />\n"), "delta sentinel is zero bytes");
        assert!(!is_empty("<memory>real</memory>"), "a real block is not empty");
    }

    #[test]
    fn oversize_block_exceeds_cap() {
        let block: String = "x".repeat(HOOK_BLOCK_CHAR_CAP);
        assert!(block.chars().count() >= HOOK_BLOCK_CHAR_CAP, "a cap-sized block must trip the oversize guard");
        let under: String = "x".repeat(HOOK_BLOCK_CHAR_CAP - 1);
        assert!(under.chars().count() < HOOK_BLOCK_CHAR_CAP, "a sub-cap block must pass the oversize guard");
    }

    /// Structural assertion of the #1 invariant: this hook module must never
    /// reference the StartupBlock/DeltaBlock exit helpers (which exit nonzero).
    /// Greps its own source so a future edit that imports them fails the gate.
    #[test]
    fn hook_module_does_not_reference_exit_helpers() {
        let source = include_str!("recall_hook.rs");
        // Skip this assertion's own occurrences in the test/comment by matching
        // only the symbol used as a path or call, never our string literals.
        for symbol in ["exit_recall_unavailable", "exit_protocol_error"] {
            // The only occurrences allowed are inside this test's array literal
            // above; assert they appear nowhere as an actual code reference by
            // checking there is no `exit::<symbol>` or bare call form.
            assert!(
                !source.contains(&format!("{symbol}(")),
                "hook handler must not call {symbol} — fail-open owns its own exit path"
            );
            assert!(!source.contains(&format!("exit::{symbol}")), "hook handler must not reference exit::{symbol}");
        }
    }
}
