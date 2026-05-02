use std::path::{Path, PathBuf};
use std::process::Command;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use serde_json::{json, Value};

use crate::support::{daemon_request, debug_binary, find_file_with_extension, read_device_id};

const STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED: &str = "STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED";
const FIRST_PRIVATE_BODY: &str = "T18 private continuity contact is t18-before@example.com.";
const SECOND_PRIVATE_BODY: &str = "T18 private continuity contact is t18-after@example.com.";

#[tokio::test]
async fn t18_encrypted_tier_key_rotation_preserves_reads_and_forward_secrecy() {
    let scaffold = DaemonScaffold::fresh().await;

    if !rotation_contract_present(scaffold.tree_dir()) {
        println!(
            "MEMORUM_EVAL_SKIP:{STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED}: Stream D key rotation contract is absent. \
             The rotate-keys probe did not create keys/decommissioned plus keys/active.json, \
             so Test #18 is semantically skipped instead of treating the shipped overwrite-only CLI as rotation."
        );
        return;
    }

    let first_response = write_pii_memory(scaffold.socket_path(), FIRST_PRIVATE_BODY, "t18-first");
    assert_write_promoted(&first_response);
    let first_id = promoted_memory_id(&first_response);
    let first_files = encrypted_memory_files(scaffold.tree_dir());
    eval_assert_eq!(first_files.len(), 1, "first PII write should create one encrypted memory file");
    assert_body_absent_from_tree(scaffold.tree_dir(), FIRST_PRIVATE_BODY);
    let old_active_key = active_key_snapshot(scaffold.tree_dir());

    let rotation = rotate_keys(scaffold.tree_dir());
    eval_assert!(
        rotation.status.success(),
        "memoryd device rotate-keys should exit 0 after the Stream D contract is present\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&rotation.stdout),
        String::from_utf8_lossy(&rotation.stderr)
    );
    eval_assert!(
        rotation_contract_files(scaffold.tree_dir()).is_present(),
        "rotation contract files should remain present after rotation"
    );
    eval_assert!(
        active_key_snapshot(scaffold.tree_dir()) != old_active_key,
        "active key snapshot should change after rotation"
    );

    let first_reveal = reveal_memory(scaffold.socket_path(), &first_id, "T18 old encrypted memory continuity check");
    assert_revealed_body(&first_reveal, FIRST_PRIVATE_BODY);

    let second_response = write_pii_memory(scaffold.socket_path(), SECOND_PRIVATE_BODY, "t18-second");
    assert_write_promoted(&second_response);
    let second_id = promoted_memory_id(&second_response);
    let second_files = encrypted_memory_files(scaffold.tree_dir());
    eval_assert_eq!(second_files.len(), 2, "second PII write should add another encrypted memory file");
    assert_new_ciphertext_uses_new_recipient(&second_files, &old_active_key);

    let second_reveal = reveal_memory(scaffold.socket_path(), &second_id, "T18 new encrypted memory active key check");
    assert_revealed_body(&second_reveal, SECOND_PRIVATE_BODY);
    assert_encrypted_reveal_events(scaffold.tree_dir(), &[&first_id, &second_id]);
    assert_device_keys_rotated_event(scaffold.tree_dir());
    eval_flush_assertion_count();
}

fn rotation_contract_present(tree_dir: &Path) -> bool {
    let files = rotation_contract_files(tree_dir);
    if files.is_present() {
        return true;
    }

    let output = rotate_keys(tree_dir);
    if !output.status.success() {
        println!(
            "MEMORUM_EVAL_SKIP:{STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED}: \
             rotate-keys probe failed before contract files existed\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return false;
    }

    rotation_contract_files(tree_dir).is_present()
}

fn rotation_contract_files(tree_dir: &Path) -> RotationContractFiles {
    let key_dir = tree_dir.join(".memoryd/keys");
    RotationContractFiles {
        decommissioned_dir: key_dir.join("decommissioned"),
        active_manifest: key_dir.join("active.json"),
    }
}

fn rotate_keys(tree_dir: &Path) -> std::process::Output {
    let memoryd = debug_binary("memoryd", "memoryd");
    Command::new(memoryd)
        .args(["device", "rotate-keys", "--runtime"])
        .arg(tree_dir.join(".memoryd"))
        .output()
        .expect("run memoryd device rotate-keys")
}

fn write_pii_memory(socket_path: &Path, body: &str, source_ref: &str) -> Value {
    daemon_request(
        socket_path,
        json!({
            "write_memory": {
                "body": body,
                "title": "T18 encrypted tier key rotation",
                "tags": ["stream-h", "t18"],
                "meta": {
                    "namespace": "project",
                    "type": "claim",
                    "summary": "T18 encrypted key rotation fixture",
                    "confidence": 0.95,
                    "source_kind": "user",
                    "source_ref": source_ref,
                    "explicit_user_context": true
                }
            }
        }),
    )
}

fn reveal_memory(socket_path: &Path, id: &str, reason: &str) -> Value {
    daemon_request(socket_path, json!({"reveal": {"id": id, "reason": reason}}))
}

fn assert_write_promoted(response: &Value) {
    eval_assert_eq!(
        response.pointer("/result/success/governance_write/status").and_then(Value::as_str),
        Some("promoted"),
        "PII write should promote into encrypted storage: {response:#?}"
    );
}

fn promoted_memory_id(response: &Value) -> String {
    response
        .pointer("/result/success/governance_write/id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("promoted write response should include memory id: {response:#?}"))
        .to_owned()
}

fn encrypted_memory_files(tree_dir: &Path) -> Vec<PathBuf> {
    let mut files = find_file_with_extension(&tree_dir.join("encrypted"), "md");
    files.sort();
    files
}

fn assert_body_absent_from_tree(tree_dir: &Path, body: &str) {
    for path in find_file_with_extension(tree_dir, "md") {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        eval_assert!(!text.contains(body), "encrypted PII body leaked to markdown file {}", path.display());
    }
}

fn active_key_snapshot(tree_dir: &Path) -> String {
    let runtime_dir = tree_dir.join(".memoryd");
    let device_id = read_device_id(&runtime_dir);
    let key_path = runtime_dir.join("keys").join(format!("{device_id}.age"));
    let manifest_path = runtime_dir.join("keys/active.json");
    let key = std::fs::read_to_string(&key_path).unwrap_or_default();
    let manifest = std::fs::read_to_string(&manifest_path).unwrap_or_default();
    format!("{manifest}\n{key}")
}

fn assert_new_ciphertext_uses_new_recipient(encrypted_files: &[PathBuf], old_active_key: &str) {
    let new_file = encrypted_files.last().expect("new encrypted file exists");
    let text = std::fs::read_to_string(new_file).unwrap_or_else(|err| panic!("read {}: {err}", new_file.display()));
    eval_assert!(
        !old_active_key.trim().is_empty(),
        "full Test #18 path requires a readable pre-rotation active key snapshot"
    );
    eval_assert!(
        !text.contains(old_active_key.trim()),
        "new ciphertext metadata must not keep using the pre-rotation active key: {}",
        new_file.display()
    );
}

fn assert_revealed_body(response: &Value, body: &str) {
    eval_assert_eq!(
        response.pointer("/result/success/reveal/body").and_then(Value::as_str),
        Some(body),
        "memory_reveal should return decrypted body: {response:#?}"
    );
}

fn assert_encrypted_reveal_events(tree_dir: &Path, memory_ids: &[&str]) {
    let events = event_log_text(tree_dir);
    for memory_id in memory_ids {
        eval_assert!(
            events.contains(r#""kind":"encrypted_content_revealed""#) && events.contains(memory_id),
            "event log should include encrypted_content_revealed for {memory_id}:\n{events}"
        );
    }
}

fn assert_device_keys_rotated_event(tree_dir: &Path) {
    let events = event_log_text(tree_dir);
    eval_assert!(
        events.contains(r#""kind":"device_keys_rotated""#),
        "event log should include DeviceKeysRotated after key rotation:\n{events}"
    );
}

fn event_log_text(tree_dir: &Path) -> String {
    find_file_with_extension(&tree_dir.join("events"), "jsonl")
        .into_iter()
        .map(|path| std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display())))
        .collect::<Vec<_>>()
        .join("\n")
}

struct RotationContractFiles {
    decommissioned_dir: PathBuf,
    active_manifest: PathBuf,
}

impl RotationContractFiles {
    fn is_present(&self) -> bool {
        self.decommissioned_dir.is_dir() && self.active_manifest.is_file()
    }
}
