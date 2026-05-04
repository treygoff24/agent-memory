use std::process::Command;

#[test]
fn inject_panic_flag_restores_before_default_hook() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd-tui"))
        .arg("--inject-panic")
        .output()
        .expect("spawn memoryd-tui --inject-panic");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("injected memoryd-tui panic"));
}

#[test]
fn inject_panic_flag_is_hidden_from_help() {
    let output =
        Command::new(env!("CARGO_BIN_EXE_memoryd-tui")).arg("--help").output().expect("spawn memoryd-tui --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("inject-panic"));
}
