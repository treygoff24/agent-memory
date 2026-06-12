//! Wright item `export-encrypted-default-03`: integration test for the
//! encrypted-memory handling contract in `memoryd export` v0.1.
//!
//! Spec §6 + §8.3: the export routes each memory by its
//! `MemoryContent` variant; ciphertext bodies emit as
//! `body=null, body_marker="encrypted"` with NO reveal-flow side effects.

use std::process::Command;

use memory_privacy::FileKeyProvider;
use memory_substrate::events::EventKind;
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

#[path = "export_fixture/mod.rs"]
mod export_fixture;

use export_fixture::{init_substrate, make_plaintext_memory, write_plaintext};

const DEVICE_ID: &str = "dev_exportenc03";
const PLAINTEXT_BODY: &str = "This is the plaintext body the export must include verbatim.";
const ENCRYPTED_PLAINTEXT: &str = "SECRET-encrypted-content-202-555-0199-keep-out-of-stdout";
const FIXTURE_TS: &str = "2026-05-01T10:00:00Z";
const TEST_PROJECT_CANONICAL_ID: &str = "proj_export_encrypted_default";
const TEST_PROJECT_ALIAS: &str = "export-encrypted-default";

#[tokio::test]
async fn encrypted_bodies_are_never_emitted_no_reveal_event() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp, DEVICE_ID).await;

    // ------------------------------------------------------------------
    // Plaintext memory.
    // ------------------------------------------------------------------
    let plain_id = "mem_20260501_aabbccdd00112233_000001";
    write_plaintext(&substrate, make_plaintext_memory(plain_id, PLAINTEXT_BODY, FIXTURE_TS)).await;

    // ------------------------------------------------------------------
    // Ciphertext memory (encrypts ENCRYPTED_PLAINTEXT through governance).
    // ------------------------------------------------------------------
    FileKeyProvider::runtime_default(&temp.path().join("runtime"))
        .onboard_local_file()
        .expect("onboard local key for encrypted fixture");

    let enc_response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "export-enc-fixture",
            RequestPayload::WriteMemory {
                body: ENCRYPTED_PLAINTEXT.to_string(),
                title: Some("encrypted export fixture".to_string()),
                tags: vec!["export-test".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "encrypted export fixture",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.85,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(enc_write)) = enc_response.result else {
        panic!("expected encrypted governance write success, got {:?}", enc_response.result);
    };
    let enc_id = enc_write.id.expect("encrypted write id");

    // Snapshot the events log immediately after the writes so the
    // post-export comparison only includes deltas the export caused.
    let events_before: Vec<_> = substrate.events().expect("events before").into_iter().collect();
    let reveal_count_before =
        events_before.iter().filter(|e| matches!(e.kind, EventKind::EncryptedContentRevealed { .. })).count();

    // ------------------------------------------------------------------
    // Spawn memoryd export.
    // ------------------------------------------------------------------
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
    let stdout = output.stdout.clone();
    let stdout_str = String::from_utf8(stdout.clone()).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");
    assert!(output.status.success(), "memoryd export failed\nstderr: {stderr}");

    let value: serde_json::Value = serde_json::from_str(&stdout_str)
        .unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\nstdout: {stdout_str}"));
    let memories = value["memories"].as_array().expect("memories array");

    // Locate the plaintext and ciphertext rows by id.
    let plain_row = memories
        .iter()
        .find(|m| m["id"].as_str() == Some(plain_id))
        .unwrap_or_else(|| panic!("plaintext row missing\nmemories: {memories:#?}"));
    let enc_row = memories
        .iter()
        .find(|m| m["id"].as_str() == Some(enc_id.as_str()))
        .unwrap_or_else(|| panic!("encrypted row missing\nmemories: {memories:#?}"));

    // Plaintext: body equals the original, body_marker is null.
    assert_eq!(plain_row["body"].as_str(), Some(PLAINTEXT_BODY), "plaintext body must match original");
    assert!(plain_row["body_marker"].is_null(), "plaintext body_marker must be null");

    // Ciphertext: body is null, body_marker == "encrypted".
    assert!(enc_row["body"].is_null(), "encrypted body must be null");
    assert_eq!(enc_row["body_marker"].as_str(), Some("encrypted"), "encrypted body_marker must be `encrypted`");

    // ------------------------------------------------------------------
    // No EncryptedContentRevealed event appeared after the export ran.
    // ------------------------------------------------------------------
    let events_after: Vec<_> = substrate.events().expect("events after").into_iter().collect();
    let reveal_count_after =
        events_after.iter().filter(|e| matches!(e.kind, EventKind::EncryptedContentRevealed { .. })).count();
    assert_eq!(
        reveal_count_after, reveal_count_before,
        "memoryd export must not invoke any reveal-flow code path; \
         EncryptedContentRevealed events before={reveal_count_before} after={reveal_count_after}"
    );

    // ------------------------------------------------------------------
    // Defense-in-depth: the ciphertext bytes on disk do NOT appear in
    // stdout. Walk the `encrypted/` subtree and check each file's bytes
    // are absent from the export output.
    // ------------------------------------------------------------------
    let encrypted_root = repo.join("encrypted");
    if encrypted_root.is_dir() {
        for entry in walkdir_simple(&encrypted_root) {
            let bytes = match std::fs::read(&entry) {
                Ok(b) if !b.is_empty() => b,
                _ => continue,
            };
            // Use a short prefix to avoid pathological collisions if the
            // file happens to start with a JSON-shaped header — 32 bytes
            // is well past any plausible coincidence.
            let probe_len = bytes.len().min(64);
            let probe = &bytes[..probe_len];
            assert!(
                !window_contains(&stdout, probe),
                "ciphertext prefix from {} appeared in export stdout (length {} probe)",
                entry.display(),
                probe.len()
            );
        }
    }

    // And the original plaintext we encrypted MUST NOT appear in stdout
    // either — the production code path is forbidden from decrypting,
    // so this should hold by construction.
    assert!(!stdout_str.contains(ENCRYPTED_PLAINTEXT), "encrypted plaintext leaked into stdout");
}

/// Minimal recursive file walker — avoids pulling in the `walkdir`
/// dep just for this assertion path.
fn walkdir_simple(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    out
}

fn window_contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}
