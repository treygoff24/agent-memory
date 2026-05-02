use std::process::Command;

#[test]
fn list_outputs_the_19_test_catalog() {
    let output =
        Command::new(env!("CARGO_BIN_EXE_memorum-eval")).arg("--list").output().expect("spawn memorum-eval --list");

    assert!(
        output.status.success(),
        "memorum-eval --list should exit 0\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("catalog output is utf-8");
    let entry_lines: Vec<&str> = stdout.lines().filter(|line| line.starts_with('#')).collect();

    assert_eq!(entry_lines.len(), 19, "catalog should contain exactly 19 test entries:\n{stdout}");
}
