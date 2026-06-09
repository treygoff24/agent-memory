//! I7 regression: --format must be enforced at clap parse time via ValueEnum,
//! not by a post-parse runtime check.  The original implementation accepted
//! `--format yaml` through clap and then returned ExportError::Argument; the
//! ValueEnum form makes clap reject the value with exit 2 and a clean message
//! listing the possible values.
//!
//! No substrate needed — argparse rejection happens before any substrate work.

use std::process::Command;

#[test]
fn format_yaml_rejected_at_argparse_with_exit_2() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["export", "--repo", "/nonexistent", "--runtime", "/nonexistent", "--format", "yaml"])
        .output()
        .expect("spawn memoryd export");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--format yaml must exit 2 (clap argparse rejection); stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "--format errors must not emit partial JSON on stdout; got:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = String::from_utf8(output.stderr).expect("stderr utf-8");
    assert!(stderr.contains("yaml"), "stderr must name the offending value 'yaml': {stderr}");
    assert!(stderr.contains("json"), "stderr must list 'json' as a possible value: {stderr}");
}
