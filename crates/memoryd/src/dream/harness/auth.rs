use std::{ffi::OsString, path::PathBuf, time::Duration};

use crate::protocol::PromptTransport;

use super::super::error::HarnessCliError;
use super::env::{AdapterEnv, MinimalEnvironment};
use super::process::{
    default_scratch_root, path_display, run_hardened_command, HardenedCommand, HarnessCommandPlan, DEFAULT_KILL_GRACE,
};

const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthProbeResult {
    Ok,
    CliMissing { which: &'static str, path: String },
    AuthFailed { exit_code: Option<i32>, stderr_tail: String },
    Timeout,
    Error { message: String },
}

impl AuthProbeResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn operator_message(&self, which: &'static str) -> String {
        match self {
            Self::Ok => format!("{which} CLI: ✓ authenticated"),
            Self::CliMissing { .. } => {
                format!("{which} CLI: ✗ not on PATH (dreams disabled for {which}); try `which {which}` in the daemon environment")
            }
            Self::AuthFailed { exit_code, stderr_tail } => {
                format!("{which} CLI: ✗ auth probe failed (exit={exit_code:?}): {stderr_tail}")
            }
            Self::Timeout => format!("{which} CLI: ✗ auth probe timed out"),
            Self::Error { message } => format!("{which} CLI: ✗ auth probe error: {message}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AuthProbeCandidate {
    pub(super) plan: HarnessCommandPlan,
    pub(super) unsupported_markers: &'static [&'static str],
}

const AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS: usize = 4096;
const AUTH_UNSUPPORTED_COMMAND_MARKERS: &[&str] = &[
    "unknown command",
    "unknown subcommand",
    "unrecognized command",
    "unrecognized subcommand",
    "invalid command",
    "invalid subcommand",
    "unsupported command",
    "unsupported subcommand",
];
const AUTH_FAILURE_MARKERS: &[&str] = &[
    "auth failed",
    "invalid credential",
    "invalid key",
    "invalid token",
    "not authenticated",
    "not logged in",
    "session expired",
    "unrecognized account",
    "unrecognized token",
];

pub(super) fn auth_probe_candidate(program: &str, args: &[&str]) -> AuthProbeCandidate {
    AuthProbeCandidate {
        plan: HarnessCommandPlan {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            prompt_transport: PromptTransport::Stdin,
        },
        unsupported_markers: AUTH_UNSUPPORTED_COMMAND_MARKERS,
    }
}

async fn auth_probe(
    plan: HarnessCommandPlan,
    path_env: Option<OsString>,
    env_allowlist: &[&str],
    config_dir: Option<PathBuf>,
) -> AuthProbeResult {
    let environment = MinimalEnvironment::for_adapter_with_optional_config_dir(path_env, env_allowlist, config_dir);
    let result = run_hardened_command(
        HardenedCommand {
            program: PathBuf::from(plan.program),
            args: plan.args,
            prompt_transport: plan.prompt_transport,
            expect_json: false,
            timeout: AUTH_PROBE_TIMEOUT,
            kill_grace: DEFAULT_KILL_GRACE,
            scratch_root: default_scratch_root(),
            environment,
            redact_stderr: false,
        },
        "",
    )
    .await;

    match result {
        Ok(_) => AuthProbeResult::Ok,
        Err(HarnessCliError::SubprocessExit { code, stderr_tail }) => {
            AuthProbeResult::AuthFailed { exit_code: code, stderr_tail }
        }
        Err(HarnessCliError::Timeout { .. }) => AuthProbeResult::Timeout,
        Err(error) => AuthProbeResult::Error { message: error.to_string() },
    }
}

/// Shared `HarnessCli::auth_probe` body for real external adapters: short-circuit
/// with `CliMissing` when the binary is absent, otherwise race the adapter's auth
/// candidates. `which` is the binary name surfaced in the missing diagnostic.
pub(super) async fn probe_external_auth(
    which: &'static str,
    env: AdapterEnv,
    candidates: Vec<AuthProbeCandidate>,
) -> AuthProbeResult {
    if !env.installed {
        return AuthProbeResult::CliMissing { which, path: path_display(env.path_env.as_deref()) };
    }
    auth_probe_any(candidates, env.path_env, env.allowlist, None).await
}

pub(super) async fn auth_probe_any(
    candidates: Vec<AuthProbeCandidate>,
    path_env: Option<OsString>,
    env_allowlist: &[&str],
    config_dir: Option<PathBuf>,
) -> AuthProbeResult {
    auth_probe_any_with_runner(candidates, |plan| {
        let path_env = path_env.clone();
        let config_dir = config_dir.clone();
        async move { auth_probe(plan, path_env, env_allowlist, config_dir).await }
    })
    .await
}

/// Prefer the current auth command, and invoke legacy candidates only when the
/// previous command failed because that command surface is unsupported.
pub(super) async fn auth_probe_any_with_runner<F, Fut>(
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
                return AuthProbeResult::Timeout;
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

fn command_label(plan: &HarnessCommandPlan) -> String {
    if plan.args.is_empty() {
        plan.program.clone()
    } else {
        format!("{} {}", plan.program, plan.args.join(" "))
    }
}

fn is_unsupported_auth_surface(stderr_tail: &str, markers: &[&str]) -> bool {
    let lower = stderr_tail.to_ascii_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
        && !AUTH_FAILURE_MARKERS.iter().any(|marker| lower.contains(marker))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan(program: &str, args: &[&str]) -> HarnessCommandPlan {
        HarnessCommandPlan {
            program: program.to_owned(),
            args: args.iter().map(|s| s.to_string()).collect(),
            prompt_transport: PromptTransport::Stdin,
        }
    }

    fn make_candidate(program: &str, args: &[&str], markers: &'static [&'static str]) -> AuthProbeCandidate {
        AuthProbeCandidate { plan: make_plan(program, args), unsupported_markers: markers }
    }

    #[test]
    fn unsupported_auth_surface_requires_command_surface_marker_without_auth_failure_marker() {
        for diagnostic in [
            "error: unknown command status",
            "error: unknown subcommand status",
            "error: unrecognized command status",
            "error: unrecognized subcommand status",
            "error: invalid command status",
            "error: invalid subcommand status",
            "error: unsupported command status",
            "error: unsupported subcommand status",
            "stdout: error: unsupported subcommand status\nstderr:",
        ] {
            assert!(
                is_unsupported_auth_surface(diagnostic, AUTH_UNSUPPORTED_COMMAND_MARKERS),
                "expected unsupported diagnostic: {diagnostic}"
            );
        }

        for diagnostic in [
            "not logged in; run codex login",
            "not authenticated; run claude auth login",
            "auth failed: unrecognized account token",
            "invalid token; run login again",
            "session expired: unknown command permissions",
        ] {
            assert!(
                !is_unsupported_auth_surface(diagnostic, AUTH_UNSUPPORTED_COMMAND_MARKERS),
                "expected auth-failure diagnostic: {diagnostic}"
            );
        }
    }

    #[tokio::test]
    async fn auth_probe_any_runs_legacy_after_unsupported_marker() {
        let candidates = vec![
            make_candidate("codex", &["login", "status"], &["unrecognized subcommand"]),
            make_candidate("codex", &["auth", "status"], &["unrecognized subcommand"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            let call_index = calls.len();
            calls.push(plan.clone());
            async move {
                if call_index == 0 {
                    AuthProbeResult::AuthFailed {
                        exit_code: Some(2),
                        stderr_tail: "error: unrecognized subcommand status\n".to_owned(),
                    }
                } else {
                    AuthProbeResult::Ok
                }
            }
        })
        .await;

        assert!(result.is_ok(), "legacy candidate should succeed after unsupported preferred: {result:?}");
        assert_eq!(calls.len(), 2, "should call both candidates");
        assert_eq!(calls[0].args, ["login", "status"]);
        assert_eq!(calls[1].args, ["auth", "status"]);
    }

    #[tokio::test]
    async fn auth_probe_any_does_not_fallback_on_exit_code_without_unsupported_marker() {
        let candidates = vec![
            make_candidate("codex", &["login", "status"], &["unrecognized subcommand"]),
            make_candidate("codex", &["auth", "status"], &["unrecognized subcommand"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move {
                AuthProbeResult::AuthFailed {
                    exit_code: Some(2),
                    stderr_tail: "not logged in; run codex login\n".to_owned(),
                }
            }
        })
        .await;

        assert!(!result.is_ok(), "exit 2 without unsupported marker should not fall back");
        assert_eq!(calls.len(), 1, "should only call preferred candidate");
        assert_eq!(calls[0].args, ["login", "status"]);
    }

    #[tokio::test]
    async fn auth_probe_any_does_not_fallback_on_preferred_timeout() {
        let candidates = vec![
            make_candidate("claude", &["auth", "status"], &["unknown command"]),
            make_candidate("claude", &["config", "get", "auth.user"], &["unknown command"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move { AuthProbeResult::Timeout }
        })
        .await;

        assert!(!result.is_ok(), "timeout on preferred must not fall back to legacy");
        assert!(matches!(result, AuthProbeResult::Timeout), "timeout should keep typed semantics: {result:?}");
        assert_eq!(calls.len(), 1, "should only call preferred candidate before timeout");
        assert_eq!(calls[0].args, ["auth", "status"]);
    }

    #[tokio::test]
    async fn auth_probe_any_truncates_multibyte_unicode_diagnostic_without_panic() {
        // Build a candidate list where every candidate is "unsupported," producing
        // a long diagnostic summary that includes multibyte Unicode characters.
        // The all-unsupported path exercises `summarize_unsupported_attempts` and
        // `truncate_for_auth_diagnostic`.
        let mut candidates = Vec::new();
        for i in 0..100 {
            candidates.push(make_candidate("codex", &[&format!("cmd{i}")], &["unsupported"]));
        }

        let mut call_index = 0;
        let result = auth_probe_any_with_runner(candidates, |_plan| {
            let i = call_index;
            call_index += 1;
            async move {
                // Use multibyte Unicode in the stderr tail
                AuthProbeResult::AuthFailed {
                    exit_code: Some(2),
                    stderr_tail: format!("error: unsupported command «テスト🧪» ({i})"),
                }
            }
        })
        .await;

        match &result {
            AuthProbeResult::Error { message } => {
                assert!(
                    message.contains("no supported auth status command was accepted"),
                    "all-unsupported path should produce clear diagnostic, got: {message}"
                );
                // The message includes the multi-byte Unicode from the stderr tails
                assert!(message.contains('«'), "diagnostic should include multibyte chars without crashing: {message}");
                // Check char count (not byte length) because multibyte chars inflate byte count.
                let char_count = message.chars().count();
                assert!(
                    char_count <= AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS + 300,
                    "diagnostic summary should be bounded by char count, got {char_count} chars ({} bytes)",
                    message.len()
                );
            }
            other => panic!("expected Error after all unsupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn auth_probe_any_does_not_fallback_on_preferred_error() {
        let candidates = vec![
            make_candidate("codex", &["login", "status"], &["unrecognized subcommand"]),
            make_candidate("codex", &["auth", "status"], &["unrecognized subcommand"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move { AuthProbeResult::Error { message: "I/O error".to_owned() } }
        })
        .await;

        assert!(!result.is_ok(), "I/O error on preferred must not fall back");
        assert_eq!(calls.len(), 1, "should only call preferred candidate");
        assert_eq!(calls[0].args, ["login", "status"]);
    }
}
