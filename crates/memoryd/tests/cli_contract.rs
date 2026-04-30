use std::process::Command;

use clap::Parser as _;
use memoryd::cli::Cli;

#[test]
fn cli_contract_help_exposes_daemon_and_agent_facing_client_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd")).arg("--help").output().expect("run memoryd --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is utf8");
    for command in ["serve", "status", "doctor", "search", "get", "write-note", "privacy", "privacy-filter", "device"] {
        assert!(stdout.contains(command), "help should list {command}");
    }
    for admin_only in ["rollback", "pin", "unpin", "policy"] {
        assert!(!stdout.contains(admin_only), "admin-only command leaked into initial CLI: {admin_only}");
    }
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

    // write-note must NOT accept --entity (flag was removed)
    assert!(
        Cli::try_parse_from(["memoryd", "write-note", "--entity", "Alice", "note text"]).is_err(),
        "--entity flag should be rejected after removal"
    );

    // serve
    Cli::try_parse_from(["memoryd", "serve", "--repo", "/tmp/repo", "--runtime", "/tmp/rt"]).expect("serve parses");
    Cli::try_parse_from(["memoryd", "serve", "--init"]).expect("serve --init parses");

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
