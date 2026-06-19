use std::time::Duration;

use memoryd::dream::error::HarnessCliError;
#[cfg(feature = "dev-fixtures")]
use memoryd::dream::harness::EchoCli;
use memoryd::dream::harness::{
    run_hardened_command, ClaudeCodeCli, CodexCli, HardenedCommand, HarnessCli, MinimalEnvironment,
    CLAUDE_ENV_ALLOWLIST, CODEX_ENV_ALLOWLIST,
};
use memoryd::dream::registry::HarnessCliRegistry;
use memoryd::protocol::PromptTransport;

static SUBPROCESS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire the process-global subprocess-test serialization guard, recovering
/// from poison.
///
/// These tests mutate process-wide state (cwd, PATH) and so must run one at a
/// time. Recovering from poison keeps one panicking test from cascading into a
/// wall of spurious acquire-panics in every later test. The guarded value is `()`
/// (the mutex carries no data a panic could leave inconsistent), so recovering
/// the inner guard via `into_inner()` is always safe.
fn lock_subprocess_test() -> std::sync::MutexGuard<'static, ()> {
    SUBPROCESS_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Scoped process-env override, restoring prior values on drop. Callers must
/// hold [`lock_subprocess_test`] since this mutates process-global state.
struct EnvGuard {
    saved: Vec<(String, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn apply(vars: Vec<(&str, Option<std::ffi::OsString>)>) -> Self {
        let mut saved = Vec::new();
        for (key, value) in vars {
            saved.push((key.to_owned(), std::env::var_os(key)));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }
    }
}

#[tokio::test]
#[cfg(feature = "dev-fixtures")]
async fn echo_cli_replays_canned_outputs_deterministically() {
    let prompt = "masked dream prompt for pass 1";
    let echo = EchoCli::from_prompt_outputs([(prompt, "canned journal")]);

    let first = echo.complete(prompt, false, Duration::from_secs(1)).await.expect("echo fixture returns canned output");
    let second = echo.complete(prompt, false, Duration::from_secs(1)).await.expect("echo fixture is deterministic");

    assert_eq!(first, "canned journal");
    assert_eq!(second, first);
}

#[test]
fn claude_adapter_detects_stub_binary_on_path() {
    let bin_dir = tempfile::tempdir().expect("stub bin dir");
    write_executable(bin_dir.path().join("claude"), "#!/bin/sh\nexit 0\n");

    let cli = ClaudeCodeCli::with_path_env(bin_dir.path().as_os_str().to_owned());

    assert!(cli.is_installed(), "stub claude on PATH should be detected");
}

#[test]
fn adapter_path_lookup_ignores_empty_path_components() {
    let _guard = lock_subprocess_test();
    let temp = tempfile::tempdir().expect("cwd tempdir");
    write_executable(temp.path().join("claude"), "#!/bin/sh\nexit 0\n");
    let original_cwd = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(temp.path()).expect("enter temp cwd");

    let is_installed = ClaudeCodeCli::with_path_env(std::ffi::OsString::from("")).is_installed();

    std::env::set_current_dir(original_cwd).expect("restore cwd");
    assert!(!is_installed, "empty PATH components must not search the daemon cwd");
}

#[test]
fn claude_and_codex_adapter_argv_never_contains_prompt() {
    let prompt = "MASKED_SECRET_PROMPT_SHOULD_NOT_BE_IN_ARGV";

    let claude_args = ClaudeCodeCli::new().command(false).args;
    let codex_text_args = CodexCli::new().command(false).args;
    let codex_json_args = CodexCli::new().command(true).args;

    assert_eq!(claude_args, ["--print"]);
    assert_eq!(codex_text_args, ["exec", "-"]);
    assert_eq!(codex_json_args, ["exec", "--json", "-"]);

    for args in [&claude_args, &codex_text_args, &codex_json_args] {
        assert!(args.iter().all(|arg| !arg.contains(prompt)), "adapter argv must never include prompt bytes: {args:?}");
    }
}

#[test]
fn hardened_subprocess_sends_prompt_only_on_stdin_with_minimal_env_and_scratch_cwd() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let temp = tempfile::tempdir().expect("recorder tempdir");
        let recorder = temp.path().join("recorder");
        let record_prefix = temp.path().join("record");
        let scratch_root = temp.path().join("scratch");
        let parent_cwd = std::env::current_dir().expect("parent cwd");
        let prompt = "MASKED_PROMPT_ONLY_STDIN";

        write_executable(
            &recorder,
            r#"#!/bin/sh
record="$1"
cat > "${record}.stdin"
printf '%s\n' "$PWD" > "${record}.cwd"
printf '%s\n' "$@" > "${record}.argv"
if [ "${SHOULD_NOT_LEAK+x}" = x ]; then printf leak > "${record}.env"; else printf ok > "${record}.env"; fi
printf 'stderr without prompt\n' >&2
printf 'stdout without prompt\n'
"#,
        );

        let mut env = MinimalEnvironment::from_pairs([
            ("PATH", std::env::var("PATH").expect("PATH is set")),
            ("HOME", temp.path().display().to_string()),
            ("TERM", "xterm-256color".to_string()),
            ("ANTHROPIC_API_KEY", "test-auth".to_string()),
            ("SHOULD_NOT_LEAK", "1".to_string()),
        ]);

        let output = run_hardened_command(
            HardenedCommand {
                program: recorder,
                args: vec![record_prefix.display().to_string()],
                prompt_transport: PromptTransport::Stdin,
                expect_json: false,
                timeout: Duration::from_secs(2),
                kill_grace: Duration::from_millis(500),
                scratch_root: scratch_root.clone(),
                environment: env.clone(),
                redact_stderr: true,
            },
            prompt,
        )
        .await
        .expect("recorder subprocess succeeds");

        assert_eq!(std::fs::read_to_string(record_prefix.with_extension("stdin")).expect("stdin record"), prompt);
        assert!(!std::fs::read_to_string(record_prefix.with_extension("argv")).expect("argv record").contains(prompt));
        assert_eq!(std::fs::read_to_string(record_prefix.with_extension("env")).expect("env record"), "ok");
        assert!(!output.stderr_tail.contains(prompt));
        assert!(!output.stdout.contains(prompt));

        let child_cwd = std::fs::read_to_string(record_prefix.with_extension("cwd")).expect("cwd record");
        let child_cwd = std::path::PathBuf::from(child_cwd.trim());
        assert_ne!(child_cwd, parent_cwd, "harness must not run in repo/project cwd");
        let scratch_root = std::fs::canonicalize(&scratch_root).expect("canonical scratch root");
        assert!(child_cwd.starts_with(&scratch_root), "harness cwd should be under scratch root: {child_cwd:?}");

        env.retain_documented_keys_only();
        assert_eq!(
            env.keys().collect::<Vec<_>>(),
            ["ANTHROPIC_API_KEY", "HOME", "PATH", "TERM"],
            "minimal env builder should retain only documented keys and force TERM into the allowlist"
        );
    });
}

#[test]
fn adapter_environment_allowlists_do_not_cross_provider_credentials() {
    let mut claude_env = MinimalEnvironment::from_pairs([
        ("PATH", "/bin".to_string()),
        ("HOME", "/tmp".to_string()),
        ("ANTHROPIC_API_KEY", "anthropic".to_string()),
        ("OPENAI_API_KEY", "openai".to_string()),
        ("CODEX_HOME", "/tmp/codex".to_string()),
        ("GEMINI_API_KEY", "gemini".to_string()),
    ]);
    claude_env.retain_keys(CLAUDE_ENV_ALLOWLIST);
    assert_eq!(claude_env.keys().collect::<Vec<_>>(), ["ANTHROPIC_API_KEY", "HOME", "PATH", "TERM"]);

    let mut codex_env = MinimalEnvironment::from_pairs([
        ("PATH", "/bin".to_string()),
        ("HOME", "/tmp".to_string()),
        ("ANTHROPIC_API_KEY", "anthropic".to_string()),
        ("CLAUDE_CONFIG_DIR", "/tmp/claude".to_string()),
        ("OPENAI_API_KEY", "openai".to_string()),
        ("CODEX_HOME", "/tmp/codex".to_string()),
        ("GEMINI_API_KEY", "gemini".to_string()),
    ]);
    codex_env.retain_keys(CODEX_ENV_ALLOWLIST);
    assert_eq!(codex_env.keys().collect::<Vec<_>>(), ["CODEX_HOME", "HOME", "OPENAI_API_KEY", "PATH", "TERM"]);
}

#[test]
fn claude_allowlist_forwards_user_for_macos_keychain_but_codex_does_not() {
    // Claude's claude.ai token is in the macOS keychain, whose lookup needs USER;
    // without it `claude auth status` returns loggedIn:false under the hardened
    // env. USER must reach the Claude subprocess but stays out of Codex (which
    // uses file-based auth), and LOGNAME is not forwarded (USER alone suffices).
    let mut claude_env = MinimalEnvironment::from_pairs([
        ("PATH", "/bin".to_string()),
        ("HOME", "/tmp".to_string()),
        ("USER", "treygoff".to_string()),
        ("LOGNAME", "treygoff".to_string()),
    ]);
    claude_env.retain_keys(CLAUDE_ENV_ALLOWLIST);
    assert_eq!(claude_env.keys().collect::<Vec<_>>(), ["HOME", "PATH", "TERM", "USER"]);

    let mut codex_env = MinimalEnvironment::from_pairs([
        ("PATH", "/bin".to_string()),
        ("HOME", "/tmp".to_string()),
        ("USER", "treygoff".to_string()),
        ("CODEX_HOME", "/tmp/codex".to_string()),
    ]);
    codex_env.retain_keys(CODEX_ENV_ALLOWLIST);
    assert_eq!(codex_env.keys().collect::<Vec<_>>(), ["CODEX_HOME", "HOME", "PATH", "TERM"]);
}

#[test]
fn hardened_subprocess_timeout_terminates_child() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let temp = tempfile::tempdir().expect("timeout tempdir");
        let sleeper = temp.path().join("sleeper");
        let record_prefix = temp.path().join("timeout");
        let scratch_root = temp.path().join("scratch");

        write_executable(
            &sleeper,
            r#"#!/bin/sh
record="$1"
trap 'printf term > "${record}.term"' TERM
printf '%s' "$$" > "${record}.pid"
while :; do sleep 1; done
"#,
        );

        let error = run_hardened_command(
            HardenedCommand {
                program: sleeper,
                args: vec![record_prefix.display().to_string()],
                prompt_transport: PromptTransport::Stdin,
                expect_json: false,
                timeout: Duration::from_millis(500),
                kill_grace: Duration::from_millis(500),
                scratch_root,
                environment: MinimalEnvironment::from_pairs([
                    ("PATH", std::env::var("PATH").expect("PATH is set")),
                    ("HOME", temp.path().display().to_string()),
                ]),
                redact_stderr: true,
            },
            "prompt sent before timeout",
        )
        .await
        .expect_err("subprocess should time out");

        assert!(matches!(error, HarnessCliError::Timeout { .. }));
        let pid = std::fs::read_to_string(record_prefix.with_extension("pid"))
            .expect("pid marker")
            .parse::<u32>()
            .expect("pid parses");
        assert!(!process_is_alive(pid), "timed-out child should not remain alive");
    });
}

#[test]
fn hardened_subprocess_timeout_covers_non_reading_child_with_large_prompt() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let temp = tempfile::tempdir().expect("non-reader tempdir");
        let sleeper = temp.path().join("non-reader");
        let record_prefix = temp.path().join("non-reader");
        let scratch_root = temp.path().join("scratch");

        write_executable(
            &sleeper,
            r#"#!/bin/sh
record="$1"
trap 'printf term > "${record}.term"' TERM
printf '%s' "$$" > "${record}.pid"
printf ready > "${record}.ready"
while :; do sleep 1; done
"#,
        );

        let prompt = "MASKED_LARGE_PROMPT_LINE\n".repeat(512 * 1024);
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            run_hardened_command(
                HardenedCommand {
                    program: sleeper,
                    args: vec![record_prefix.display().to_string()],
                    prompt_transport: PromptTransport::Stdin,
                    expect_json: false,
                    timeout: Duration::from_millis(500),
                    kill_grace: Duration::from_millis(500),
                    scratch_root,
                    environment: MinimalEnvironment::from_pairs([
                        ("PATH", std::env::var("PATH").expect("PATH is set")),
                        ("HOME", temp.path().display().to_string()),
                    ]),
                    redact_stderr: true,
                },
                &prompt,
            ),
        )
        .await
        .expect("harness timeout should cover blocked stdin writes")
        .expect_err("non-reading child should time out");

        // Boundedness is enforced by the outer 3 s tokio timeout; the Timeout
        // error proves the 500 ms harness timeout fired rather than blocking on
        // the non-reading child. A tighter wall-clock assertion here (formerly
        // < 2 s) flaked under parallel-test load, where scheduler delay alone
        // could exceed the slack above the 500 ms + 500 ms kill-grace budget.
        assert!(matches!(error, HarnessCliError::Timeout { .. }));

        let pid = std::fs::read_to_string(record_prefix.with_extension("pid"))
            .expect("pid marker")
            .parse::<u32>()
            .expect("pid parses");
        assert!(!process_is_alive(pid), "timed-out non-reader child should not remain alive");
    });
}

#[test]
fn hardened_subprocess_ignores_broken_pipe_after_successful_stdout() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let temp = tempfile::tempdir().expect("epipe tempdir");
        let early_exit = temp.path().join("early-exit");

        write_executable(
            &early_exit,
            r#"#!/bin/sh
exec 0<&-
printf '{"ok":true}\n'
exit 0
"#,
        );

        let output = run_hardened_command(
            HardenedCommand {
                program: early_exit,
                args: Vec::new(),
                prompt_transport: PromptTransport::Stdin,
                expect_json: true,
                timeout: Duration::from_secs(2),
                kill_grace: Duration::from_millis(250),
                scratch_root: temp.path().join("scratch"),
                environment: MinimalEnvironment::from_pairs([
                    ("PATH", std::env::var("PATH").expect("PATH is set")),
                    ("HOME", temp.path().display().to_string()),
                ]),
                redact_stderr: true,
            },
            &"large masked prompt\n".repeat(64 * 1024),
        )
        .await
        .expect("successful child stdout should win over stdin BrokenPipe");

        assert_eq!(output.stdout, "{\"ok\":true}\n");
    });
}

#[test]
fn hardened_subprocess_redacts_partial_prompt_echo_from_stderr_error() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let temp = tempfile::tempdir().expect("stderr tempdir");
        let echoer = temp.path().join("partial-stderr");
        let scratch_root = temp.path().join("scratch");
        let prompt = "MASKED_PROMPT_ALPHA\nMASKED_PROMPT_BETA\nMASKED_PROMPT_GAMMA";

        write_executable(
            &echoer,
            r#"#!/bin/sh
printf 'diagnostic: MASKED_PROMPT_BETA\n' >&2
exit 23
"#,
        );

        let error = run_hardened_command(
            HardenedCommand {
                program: echoer,
                args: Vec::new(),
                prompt_transport: PromptTransport::Stdin,
                expect_json: false,
                timeout: Duration::from_secs(2),
                kill_grace: Duration::from_millis(250),
                scratch_root,
                environment: MinimalEnvironment::from_pairs([
                    ("PATH", std::env::var("PATH").expect("PATH is set")),
                    ("HOME", temp.path().display().to_string()),
                ]),
                redact_stderr: true,
            },
            prompt,
        )
        .await
        .expect_err("failing subprocess should return an error");

        let error_text = error.to_string();
        assert!(!error_text.contains("MASKED_PROMPT_ALPHA"));
        assert!(!error_text.contains("MASKED_PROMPT_BETA"));
        assert!(!error_text.contains("MASKED_PROMPT_GAMMA"));
        match error {
            HarnessCliError::SubprocessExit { stderr_tail, .. } => {
                assert!(!stderr_tail.contains("MASKED_PROMPT_ALPHA"));
                assert!(!stderr_tail.contains("MASKED_PROMPT_BETA"));
                assert!(!stderr_tail.contains("MASKED_PROMPT_GAMMA"));
            }
            other => panic!("expected subprocess exit, got {other:?}"),
        }
    });
}

#[test]
fn auth_probe_mode_preserves_stderr_tail_for_operator_diagnostics() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let temp = tempfile::tempdir().expect("auth stderr tempdir");
        let auth_probe = temp.path().join("auth-probe");

        write_executable(
            &auth_probe,
            r#"#!/bin/sh
printf 'not logged in: run cli auth login\n' >&2
exit 1
"#,
        );

        let error = run_hardened_command(
            HardenedCommand {
                program: auth_probe,
                args: Vec::new(),
                prompt_transport: PromptTransport::Stdin,
                expect_json: false,
                timeout: Duration::from_secs(2),
                kill_grace: Duration::from_millis(250),
                scratch_root: temp.path().join("scratch"),
                environment: MinimalEnvironment::from_pairs([
                    ("PATH", std::env::var("PATH").expect("PATH is set")),
                    ("HOME", temp.path().display().to_string()),
                ]),
                redact_stderr: false,
            },
            "",
        )
        .await
        .expect_err("auth probe should fail");

        match error {
            HarnessCliError::SubprocessExit { stderr_tail, .. } => {
                assert!(stderr_tail.contains("run cli auth login"), "{stderr_tail}");
                assert!(!stderr_tail.contains("[stderr redacted"), "{stderr_tail}");
            }
            other => panic!("expected subprocess exit, got {other:?}"),
        }
    });
}

#[test]
fn v0_2_registry_declares_no_argv_prompt_transport() {
    let registry = HarnessCliRegistry::builtin_v0_2();

    let adapters = registry.adapters().map(|(name, adapter)| (name, adapter.prompt_transport())).collect::<Vec<_>>();

    assert_eq!(adapters, [("claude", PromptTransport::Stdin), ("codex", PromptTransport::Stdin)]);
    assert!(adapters.iter().all(|(_, transport)| *transport != PromptTransport::Argv));

    let gemini = registry.disabled_adapters().find(|adapter| adapter.name == "gemini").expect("gemini disabled status");
    assert!(!gemini.is_installed, "Gemini remains disabled until stdin support is proven");
    assert_eq!(gemini.prompt_transport, PromptTransport::Stdin, "disabled Gemini must not introduce argv fallback");
}

#[test]
fn codex_auth_probe_prefers_login_status() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'Logged in using ChatGPT\n'
  exit 0
fi
printf 'wrong command: %s\n' "$*" >&2
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(probe.is_ok(), "expected codex login status to authenticate, got {probe:?}");
        let calls = std::fs::read_to_string(marker).expect("called marker");
        assert_eq!(calls.trim(), "login status");
    });
}

#[test]
fn codex_auth_probe_falls_back_to_legacy_auth_status_only_when_login_status_is_unsupported() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
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
                shell_quote(&marker)
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(
            probe.is_ok(),
            "legacy codex auth status should authenticate after unsupported login status: {probe:?}"
        );
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "login status\nauth status\n");
    });
}

#[test]
fn codex_auth_probe_falls_back_when_unsupported_diagnostic_is_stdout_only() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'error: unrecognized subcommand status\n'
  exit 2
fi
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'authenticated\n'
  exit 0
fi
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(
            probe.is_ok(),
            "legacy codex auth status should authenticate after stdout-only unsupported diagnostic: {probe:?}"
        );
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "login status\nauth status\n");
    });
}

#[test]
fn codex_auth_probe_does_not_fallback_after_supported_login_status_auth_failure() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
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
                shell_quote(&marker)
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(!probe.is_ok(), "auth failure must remain unhealthy");
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "login status\n");
    });
}

#[test]
fn codex_auth_probe_does_not_fallback_on_exit_code_alone() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("codex"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = login ] && [ "$2" = status ]; then
  printf 'not logged in; run codex login\n' >&2
  exit 2
fi
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'legacy must not be called\n' >&2
  exit 64
fi
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = CodexCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(!probe.is_ok(), "exit code 2 with auth-failure message must not trigger legacy fallback");
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "login status\n");
    });
}

#[test]
fn claude_auth_probe_prefers_auth_status() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("claude"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'authenticated\n'
  exit 0
fi
printf 'wrong command: %s\n' "$*" >&2
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = ClaudeCodeCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(probe.is_ok(), "expected claude auth status to authenticate, got {probe:?}");
        let calls = std::fs::read_to_string(marker).expect("called marker");
        assert_eq!(calls.trim(), "auth status");
    });
}

#[test]
fn claude_auth_probe_falls_back_to_legacy_config_get_only_when_auth_status_is_unsupported() {
    let _guard = lock_subprocess_test();
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("claude"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'error: unknown command status\n' >&2
  exit 2
fi
if [ "$1" = config ] && [ "$2" = get ] && [ "$3" = auth.user ]; then
  printf 'some-user@example.com\n'
  exit 0
fi
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = ClaudeCodeCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(
            probe.is_ok(),
            "legacy claude config get auth.user should authenticate after unsupported auth status: {probe:?}"
        );
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "auth status\nconfig get auth.user\n");
    });
}

#[test]
fn claude_auth_probe_does_not_fallback_after_supported_auth_status_auth_failure() {
    let _guard = lock_subprocess_test();
    // Pin HOME to an empty dir so profile resolution finds no sibling
    // `~/.claude-*` profiles to scan: this test asserts the per-directory command
    // fallback, not multi-profile resolution.
    let home = tempfile::tempdir().expect("home dir");
    let _env = EnvGuard::apply(vec![("HOME", Some(home.path().as_os_str().to_owned())), ("CLAUDE_CONFIG_DIR", None)]);
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("claude"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'not authenticated; run claude auth login\n' >&2
  exit 1
fi
if [ "$1" = config ] && [ "$2" = get ] && [ "$3" = auth.user ]; then
  printf 'legacy command must not run\n' >&2
  exit 64
fi
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = ClaudeCodeCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(!probe.is_ok(), "auth failure must remain unhealthy");
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "auth status\n");
    });
}

#[test]
fn claude_auth_probe_does_not_fallback_on_exit_code_alone() {
    let _guard = lock_subprocess_test();
    // See sibling test above: pin HOME so no `~/.claude-*` profiles are scanned.
    let home = tempfile::tempdir().expect("home dir");
    let _env = EnvGuard::apply(vec![("HOME", Some(home.path().as_os_str().to_owned())), ("CLAUDE_CONFIG_DIR", None)]);
    run_async(async {
        let bin_dir = tempfile::tempdir().expect("stub bin dir");
        let marker = bin_dir.path().join("called");
        write_executable(
            bin_dir.path().join("claude"),
            &format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> {}
if [ "$1" = auth ] && [ "$2" = status ]; then
  printf 'not authenticated; run claude auth login\n' >&2
  exit 2
fi
if [ "$1" = config ] && [ "$2" = get ] && [ "$3" = auth.user ]; then
  printf 'legacy must not be called\n' >&2
  exit 64
fi
exit 64
"#,
                shell_quote(&marker)
            ),
        );

        let cli = ClaudeCodeCli::with_path_env(bin_dir.path().as_os_str().to_owned());
        let probe = cli.auth_probe().await;

        assert!(!probe.is_ok(), "exit code 2 with auth-failure message must not trigger legacy fallback");
        assert_eq!(std::fs::read_to_string(marker).expect("called marker"), "auth status\n");
    });
}

#[cfg(unix)]
fn write_executable(path: impl AsRef<std::path::Path>, contents: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path.as_ref(), contents).expect("write executable stub");
    let mut permissions = std::fs::metadata(path.as_ref()).expect("stub metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path.as_ref(), permissions).expect("mark stub executable");
}

fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("test runtime").block_on(future)
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
