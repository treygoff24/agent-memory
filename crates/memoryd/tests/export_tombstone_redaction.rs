//! Regression coverage for the default export behavior on tombstoned memories:
//! even when the canonical file still contains plaintext, export emits no body
//! and marks the record as tombstoned.

use std::process::Command;

use memory_substrate::{MemoryId, TombstoneRequest};

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exporttombstone";
const TOMBSTONED_BODY: &str = "SECRET tombstoned plaintext body must not be exported";

#[tokio::test]
async fn tombstoned_plaintext_body_is_redacted_by_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    let id = "mem_20260501_abcd00000000abcd_000001";
    write_plaintext(&substrate, make_plaintext_memory(id, TOMBSTONED_BODY, "2026-05-01T10:00:00Z")).await;
    substrate
        .tombstone_memory(TombstoneRequest { id: MemoryId::new(id), reason: "export redaction regression".to_string() })
        .await
        .expect("tombstone memory");

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

    assert!(output.status.success(), "export failed\nstderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout.clone()).expect("stdout utf8");
    assert!(!stdout.contains(TOMBSTONED_BODY), "tombstoned plaintext body must not appear in export stdout:\n{stdout}");

    let value: serde_json::Value = serde_json::from_str(&stdout).expect("export stdout json");
    let memories = value["memories"].as_array().expect("memories array");
    let row = memories
        .iter()
        .find(|memory| memory["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("tombstoned memory row missing; memories: {memories:#?}"));
    assert_eq!(row["status"].as_str(), Some("tombstoned"));
    assert!(row["body"].is_null(), "tombstoned export body must be null");
    assert_eq!(row["body_marker"].as_str(), Some("tombstoned"));
}
