use std::process::Command;

use chrono::{TimeZone, Utc};
use clap::{CommandFactory as _, Parser as _};
use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::cli::{
    reality_check_request_payload, validate_snooze_until, validate_ui_stdin, web_request_payload, Cli,
    Command as CliCommand, RealityCheckCommand, WebCommand,
};
use memoryd::protocol::{RealityCheckRequest, RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};
use serde_json::Value;

#[test]
fn cli_contract_help_exposes_daemon_and_agent_facing_client_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd")).arg("--help").output().expect("run memoryd --help");

    assert!(output.status.success());
    let command_names = top_level_command_names();
    assert_command_names_include(
        &command_names,
        &[
            "serve",
            "mcp",
            "status",
            "doctor",
            "search",
            "get",
            "write-note",
            "source",
            "ui",
            "web",
            "reality-check",
            "privacy",
            "privacy-filter",
            "device",
            "dream",
        ],
    );
    assert_command_names_exclude(&command_names, &["rollback", "pin", "unpin", "policy"]);
}

#[test]
fn cli_contract_client_commands_expose_help_without_requiring_daemon() {
    for args in
        [&["status", "--help"][..], &["search", "--help"][..], &["get", "--help"][..], &["write-note", "--help"][..]]
    {
        let output = Command::new(env!("CARGO_BIN_EXE_memoryd")).args(args).output().expect("run memoryd command");
        assert!(output.status.success(), "command failed: {args:?}");
    }
}

#[test]
fn doctor_unhealthy_exit_is_nonzero_when_no_harness_is_authenticated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let empty_path = temp.path().join("empty-path");
    std::fs::create_dir_all(&empty_path).expect("empty path dir");

    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime").block_on(async {
        Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_doctorcli".to_owned()) },
        )
        .await
        .expect("init substrate");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["doctor", "--repo"])
        .arg(&repo)
        .arg("--runtime")
        .arg(&runtime)
        .env("PATH", &empty_path)
        .output()
        .expect("run memoryd doctor");

    assert_eq!(output.status.code(), Some(1), "unhealthy doctor should exit 1");
    let stdout = String::from_utf8(output.stdout).expect("doctor stdout is utf8");
    let response: ResponseEnvelope = serde_json::from_str(&stdout).expect("doctor stdout is response JSON");
    let ResponseResult::Success(ResponsePayload::Doctor(doctor)) = response.result else {
        panic!("doctor should return a successful doctor response: {stdout}");
    };
    assert!(!doctor.healthy, "doctor should report unhealthy when no harness is available: {stdout}");
    let stdout_json: Value = serde_json::from_str(&stdout).expect("doctor stdout is json");
    assert!(
        !json_strings_contain(&stdout_json, "daemon PATH="),
        "doctor output should not disclose the full daemon PATH: {stdout}"
    );
}

/// Parsing-coverage test using clap's in-process parser — does not spawn the binary
/// and does not require a running daemon.
#[test]
fn cli_contract_clap_parses_all_subcommands() {
    // status
    Cli::try_parse_from(["memoryd", "status"]).expect("status parses");
    Cli::try_parse_from(["memoryd", "status", "--socket", "/tmp/test.sock"]).expect("status with socket parses");

    // search
    Cli::try_parse_from(["memoryd", "search", "hello world"]).expect("search parses");
    Cli::try_parse_from(["memoryd", "search", "--limit", "5", "--include-body", "query text"])
        .expect("search with flags parses");

    // get
    Cli::try_parse_from(["memoryd", "get", "mem-0001"]).expect("get parses");
    Cli::try_parse_from(["memoryd", "get", "--include-provenance", "mem-0002"])
        .expect("get with provenance flag parses");

    // write-note
    Cli::try_parse_from(["memoryd", "write-note", "a quick note"]).expect("write-note parses");
    Cli::try_parse_from([
        "memoryd",
        "source",
        "capture",
        "--url",
        "https://example.com/report",
        "--excerpt",
        "exact quote",
    ])
    .expect("source capture parses");

    // write-note must NOT accept --entity (flag was removed)
    assert!(
        Cli::try_parse_from(["memoryd", "write-note", "--entity", "Alice", "note text"]).is_err(),
        "--entity flag should be rejected after removal"
    );

    // serve
    Cli::try_parse_from(["memoryd", "serve", "--repo", "/tmp/repo", "--runtime", "/tmp/rt"]).expect("serve parses");
    Cli::try_parse_from(["memoryd", "serve", "--init"]).expect("serve --init parses");
    Cli::try_parse_from(["memoryd", "mcp"]).expect("mcp parses");
    Cli::try_parse_from(["memoryd", "mcp", "--socket", "/tmp/test.sock"]).expect("mcp with socket parses");

    // doctor
    Cli::try_parse_from(["memoryd", "doctor"]).expect("doctor parses");

    Cli::try_parse_from([
        "memoryd",
        "recall",
        "startup-block",
        "--cwd",
        "/tmp",
        "--session-id",
        "sess",
        "--harness",
        "codex",
        "--no-include-recent",
    ])
    .expect("recall startup --no-include-recent parses");

    // Stream D admin commands
    Cli::try_parse_from(["memoryd", "privacy", "status"]).expect("privacy status parses");
    Cli::try_parse_from(["memoryd", "privacy", "scan", "--text", "hello"]).expect("privacy scan text parses");
    Cli::try_parse_from(["memoryd", "privacy", "scan-delta", "--repo", "."]).expect("privacy scan-delta parses");
    Cli::try_parse_from(["memoryd", "privacy-filter", "status"]).expect("privacy-filter status parses");
    Cli::try_parse_from(["memoryd", "privacy-filter", "install"]).expect("privacy-filter install parses");
    Cli::try_parse_from(["memoryd", "device", "onboard"]).expect("device onboard parses");
    Cli::try_parse_from(["memoryd", "device", "rotate-keys"]).expect("device rotate parses");
    Cli::try_parse_from(["memoryd", "device", "revoke", "dev_a1b2"]).expect("device revoke parses");
}

#[test]
fn cli_contract_clap_parses_all_dream_subcommands_and_help_exposes_them() {
    Cli::try_parse_from(["memoryd", "dream", "status"]).expect("dream status parses");
    Cli::try_parse_from(["memoryd", "dream", "status", "--repo", "/tmp/repo", "--runtime", "/tmp/rt", "--json"])
        .expect("dream status flags parse");
    Cli::try_parse_from([
        "memoryd",
        "dream",
        "now",
        "--repo",
        "/tmp/repo",
        "--runtime",
        "/tmp/rt",
        "--scope",
        "me",
        "--force",
        "--cli",
        "echo",
        "--json",
    ])
    .expect("dream now flags parse");
    Cli::try_parse_from([
        "memoryd",
        "dream",
        "review",
        "--repo",
        "/tmp/repo",
        "--runtime",
        "/tmp/rt",
        "--since",
        "7d",
        "--scope",
        "project:proj_abc",
    ])
    .expect("dream review flags parse");
    Cli::try_parse_from(["memoryd", "dream", "enable", "--runtime", "/tmp/rt"]).expect("dream enable parses");
    Cli::try_parse_from(["memoryd", "dream", "disable", "--runtime", "/tmp/rt"]).expect("dream disable parses");

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd")).args(["dream", "--help"]).output().expect("dream help");
    assert!(output.status.success());
    let dream_command_names = subcommand_names("dream");
    assert_command_names_include(&dream_command_names, &["status", "now", "review", "enable", "disable"]);
}

#[test]
fn test_clap_rejects_panel_flag() {
    assert!(
        Cli::try_parse_from(["memoryd", "ui", "--panel", "3"]).is_err(),
        "--panel was forwarded to memoryd-tui which does not accept it; the flag has been removed"
    );
}

#[test]
fn test_ui_subprocess_args_does_not_forward_panel() {
    use memoryd::cli::{ui_subprocess_args, UiArgs};
    use std::ffi::OsString;
    use std::path::PathBuf;

    let args = UiArgs { socket: Some(PathBuf::from("/tmp/memoryd.sock")) };
    let forwarded = ui_subprocess_args(&args);
    assert!(
        !forwarded.iter().any(|arg| arg == &OsString::from("--panel")),
        "ui_subprocess_args must not forward --panel; got {forwarded:?}"
    );
}

#[test]
fn test_clap_parses_web_enable_with_port() {
    let cli = Cli::try_parse_from(["memoryd", "web", "enable", "--port", "7138"]).expect("web enable parses");

    match &cli.command {
        CliCommand::Web(web) => match &web.command {
            WebCommand::Enable(args) => assert_eq!(args.port, 7138),
            other => panic!("expected web enable, got {other:?}"),
        },
        other => panic!("expected web command, got {other:?}"),
    }
    let CliCommand::Web(web) = cli.command else {
        panic!("expected web command");
    };
    assert_eq!(
        web_request_payload(&web.command),
        RequestPayload::WebEnable {
            port: 7138,
            socket_path: memoryd::socket::resolve_socket_path(&memoryd::socket::default_runtime_root())
                .to_string_lossy()
                .into_owned()
        }
    );
}

#[test]
fn test_clap_parses_web_disable() {
    let cli = Cli::try_parse_from(["memoryd", "web", "disable"]).expect("web disable parses");

    match cli.command {
        CliCommand::Web(web) => assert!(matches!(web.command, WebCommand::Disable(_))),
        other => panic!("expected web command, got {other:?}"),
    }
}

#[test]
fn test_clap_parses_web_status_json_flag() {
    let cli = Cli::try_parse_from(["memoryd", "web", "status", "--json"]).expect("web status parses");

    match cli.command {
        CliCommand::Web(web) => match web.command {
            WebCommand::Status(args) => assert!(args.json),
            other => panic!("expected web status, got {other:?}"),
        },
        other => panic!("expected web command, got {other:?}"),
    }
}

#[test]
fn test_clap_parses_reality_check_run() {
    let cli = Cli::try_parse_from(["memoryd", "reality-check", "run", "--top-n", "5", "--namespace", "me"])
        .expect("reality-check run parses");

    match cli.command {
        CliCommand::RealityCheck(args) => match args.command {
            RealityCheckCommand::Run(run) => {
                assert_eq!(run.top_n, Some(5));
                assert_eq!(run.namespace.as_deref(), Some("me"));
                assert!(!run.json);
                assert!(!run.tui);
            }
            other => panic!("expected reality-check run, got {other:?}"),
        },
        other => panic!("expected reality-check command, got {other:?}"),
    }
}

#[test]
fn test_clap_parses_reality_check_skip() {
    let cli = Cli::try_parse_from(["memoryd", "reality-check", "skip"]).expect("reality-check skip parses");

    match cli.command {
        CliCommand::RealityCheck(args) => assert!(matches!(args.command, RealityCheckCommand::Skip(_))),
        other => panic!("expected reality-check command, got {other:?}"),
    }
}

#[test]
fn test_clap_parses_reality_check_snooze_until() {
    let cli = Cli::try_parse_from(["memoryd", "reality-check", "snooze", "--until", "2026-05-10"])
        .expect("reality-check snooze parses");

    match cli.command {
        CliCommand::RealityCheck(args) => match args.command {
            RealityCheckCommand::Snooze(snooze) => assert_eq!(snooze.until.as_deref(), Some("2026-05-10")),
            other => panic!("expected reality-check snooze, got {other:?}"),
        },
        other => panic!("expected reality-check command, got {other:?}"),
    }
}

#[test]
fn test_memoryd_ui_rejects_non_tty() {
    let error = validate_ui_stdin(false).expect_err("non-TTY stdin should be rejected");

    assert_eq!(error.exit_code(), 2);
    assert_eq!(error.message(), "memoryd ui requires an interactive terminal.");
}

#[test]
fn test_memoryd_web_enable_delegates_to_daemon() {
    let cli = Cli::try_parse_from(["memoryd", "web", "enable"]).expect("web enable parses");

    let CliCommand::Web(web) = cli.command else {
        panic!("expected web command");
    };

    assert_eq!(
        web_request_payload(&web.command),
        RequestPayload::WebEnable {
            port: 7137,
            socket_path: memoryd::socket::resolve_socket_path(&memoryd::socket::default_runtime_root())
                .to_string_lossy()
                .into_owned()
        }
    );
}

#[test]
fn test_memoryd_reality_check_run_json_exits_without_interactive() {
    let cli = Cli::try_parse_from(["memoryd", "reality-check", "run", "--json", "--top-n", "5", "--namespace", "me"])
        .expect("reality-check run --json parses");

    let CliCommand::RealityCheck(args) = cli.command else {
        panic!("expected reality-check command");
    };

    assert_eq!(
        reality_check_request_payload(&args.command).expect("json run maps to request"),
        RequestPayload::RealityCheck(RealityCheckRequest::List { namespace: Some("me".to_string()), limit: Some(5) })
    );
}

#[test]
fn test_memoryd_reality_check_run_interactive_forwards_top_n() {
    let cli = Cli::try_parse_from(["memoryd", "reality-check", "run", "--top-n", "5", "--namespace", "me"])
        .expect("reality-check run parses");

    let CliCommand::RealityCheck(args) = cli.command else {
        panic!("expected reality-check command");
    };

    assert_eq!(
        reality_check_request_payload(&args.command).expect("interactive run maps to request"),
        RequestPayload::RealityCheck(RealityCheckRequest::Run {
            session_id: None,
            namespace: Some("me".to_string()),
            limit: Some(5)
        })
    );
}

#[test]
fn test_memoryd_reality_check_snooze_until_reaches_daemon_request() {
    let cli = Cli::try_parse_from(["memoryd", "reality-check", "snooze", "--until", "2026-05-10"])
        .expect("reality-check snooze parses");

    let CliCommand::RealityCheck(args) = cli.command else {
        panic!("expected reality-check command");
    };

    assert_eq!(
        reality_check_request_payload(&args.command).expect("snooze maps to request"),
        RequestPayload::RealityCheck(RealityCheckRequest::Snooze {
            until: Some(Utc.with_ymd_and_hms(2026, 5, 10, 0, 0, 0).unwrap())
        })
    );
}

#[test]
fn test_memoryd_reality_check_snooze_invalid_date_exits_1() {
    assert_eq!(validate_snooze_until(Some("next-week")), Err(1));
}

fn top_level_command_names() -> Vec<String> {
    Cli::command().get_subcommands().map(|command| command.get_name().to_string()).collect()
}

fn subcommand_names(parent: &str) -> Vec<String> {
    let command = Cli::command();
    let parent = command.find_subcommand(parent).unwrap_or_else(|| panic!("missing {parent} command"));
    parent.get_subcommands().map(|command| command.get_name().to_string()).collect()
}

fn assert_command_names_include(actual: &[String], expected: &[&str]) {
    for command in expected {
        assert!(actual.iter().any(|actual| actual == command), "command metadata should list {command}: {actual:?}");
    }
}

fn assert_command_names_exclude(actual: &[String], expected: &[&str]) {
    for command in expected {
        assert!(
            !actual.iter().any(|actual| actual == command),
            "command metadata should not list {command}: {actual:?}"
        );
    }
}

fn json_strings_contain(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text.contains(needle),
        Value::Array(values) => values.iter().any(|value| json_strings_contain(value, needle)),
        Value::Object(values) => values.values().any(|value| json_strings_contain(value, needle)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}
