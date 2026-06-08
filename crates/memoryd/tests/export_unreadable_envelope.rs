//! B2 regression: unreadable memory envelopes must fail the export with exit 1
//! and a stderr diagnostic, not be silently dropped.
//!
//! The original implementation used `Err(_) => None` in `filter_map`, which
//! silently excluded corrupt files and shrunk `memory_count` without any
//! signal to the caller.

use std::process::Command;

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exportunreadable";

#[tokio::test]
async fn unreadable_envelope_fails_export_with_exit_1() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    let id = "mem_20260501_cccc00000000cccc_000001";
    write_plaintext(&substrate, make_plaintext_memory(id, "good body", "2026-05-01T10:00:00Z")).await;

    // Inject a corrupt file into the substrate's memory directory that the
    // iterator will attempt to parse as a MemoryEnvelope. The path must look
    // like a canonical memory path (agent/claims/*.md) so the iterator picks
    // it up. Writing non-YAML-frontmatter bytes causes a parse error.
    let corrupt_path = temp.path().join("repo").join("agent").join("claims").join("corrupt_mem.md");
    std::fs::write(&corrupt_path, b"this is not valid YAML frontmatter\x00\x01\x02").expect("write corrupt file");

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

    assert_eq!(
        exit_code, 1,
        "a corrupt memory file must cause exit 1 (B2 regression: original silently dropped it); stderr:\n{stderr}"
    );
    assert!(!output.status.success(), "export must not succeed when a memory envelope cannot be read");
    assert!(
        stderr.contains("failed to read memory envelope") || stderr.contains("error"),
        "stderr must contain a diagnostic about the read failure; got:\n{stderr}"
    );
}
