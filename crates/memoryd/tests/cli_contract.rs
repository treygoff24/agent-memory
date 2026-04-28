use std::process::Command;

#[test]
fn cli_contract_help_exposes_daemon_and_agent_facing_client_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd")).arg("--help").output().expect("run memoryd --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is utf8");
    for command in ["serve", "status", "doctor", "search", "get", "write-note"] {
        assert!(stdout.contains(command), "help should list {command}");
    }
    for admin_only in ["rollback", "pin", "unpin", "policy"] {
        assert!(!stdout.contains(admin_only), "admin-only command leaked into initial CLI: {admin_only}");
    }
}

#[test]
fn cli_contract_client_commands_parse_minimal_inputs() {
    for args in [
        &["status"][..],
        &["search", "needle"][..],
        &["get", "mem_20260428_0123456789abcdef_000001"][..],
        &["write-note", "observed a useful pattern"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_memoryd")).args(args).output().expect("run memoryd command");
        assert!(output.status.success(), "command failed: {args:?}");
    }
}
