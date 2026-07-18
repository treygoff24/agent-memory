//! B4 regression: missing device config must fail the export with exit 1,
//! not emit `source_device_id: ""`.
//!
//! The original implementation used `.unwrap_or_default()` on the
//! `Ok(None)` case from `load_local_device_config`, which silently produced
//! an empty device id in the JSON output.

use std::process::Command;

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exportmissingdev";

#[tokio::test]
async fn missing_device_config_fails_export_with_exit_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    write_plaintext(
        &substrate,
        make_plaintext_memory("mem_20260501_dddd00000000dddd_000001", "body", "2026-05-01T10:00:00Z"),
    )
    .await;

    let device_config = temp.path().join("runtime").join("local-device.yaml");
    std::fs::remove_file(&device_config)
        .unwrap_or_else(|e| panic!("expected local-device.yaml at {}: {e}", device_config.display()));

    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");

    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
        ])
        .output()
        .expect("spawn memoryd export");

    let exit_code = output.status.code().expect("exit code present");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf-8");
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf-8");

    assert_eq!(
        exit_code, 1,
        "missing device config must exit 1 (B4 regression: original produced empty source_device_id); \
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // Must not emit any JSON to stdout (the error path must exit before writing output).
    assert!(
        stdout.is_empty(),
        "stdout must not contain a partial export when device config is missing; got:\n{stdout}"
    );
}

#[tokio::test]
async fn whitespace_device_id_fails_export_with_empty_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    write_plaintext(
        &substrate,
        make_plaintext_memory("mem_20260501_ffff00000000ffff_000001", "body", "2026-05-01T10:00:00Z"),
    )
    .await;

    let runtime = temp.path().join("runtime");
    let device_config = runtime.join("local-device.yaml");
    std::fs::write(
        &device_config,
        "schema_version: 1\ndevice:\n  id: \" dev_whitespace \"\n  name: \"dev whitespace\"\n  shard: \"devwhite\"\n",
    )
    .expect("write whitespace device config");

    let repo = temp.path().join("repo");
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args([
            "export",
            "--repo",
            repo.to_str().expect("repo utf8"),
            "--runtime",
            runtime.to_str().expect("runtime utf8"),
        ])
        .output()
        .expect("spawn memoryd export");

    assert_eq!(
        output.status.code(),
        Some(1),
        "whitespace source_device_id must exit 1; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "whitespace source_device_id error must leave stdout empty; got:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("source_device_id") && stderr.contains("non-empty"),
        "stderr must name the source_device_id validation failure; got:\n{stderr}"
    );
}
