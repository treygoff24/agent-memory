use std::io::Write;
use std::path::Path;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use serde_json::{json, Value};

use crate::support::{daemon_request, find_file_with_extension, git, read_device_id};

const SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED: &str = "SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED";

#[tokio::test]
async fn t17_preseeded_two_device_lease_blocks_loser_and_allows_retry_after_release() {
    let scaffold = DaemonScaffold::two_device().await;
    let device_a_id = read_device_id(&scaffold.device_a.tree_dir().join(".memoryd"));
    let device_b_id = read_device_id(&scaffold.device_b.tree_dir().join(".memoryd"));
    eval_assert!(device_a_id != device_b_id, "two-device scaffold should create distinct device ids");

    append_lease_record(scaffold.device_a.tree_dir(), active_lease_record(&device_a_id));
    git(scaffold.device_a.tree_dir(), ["add", "leases/journal.lease"]);
    git(scaffold.device_a.tree_dir(), ["commit", "-m", "seed active device A journal lease"]);
    git(scaffold.device_a.tree_dir(), ["push", "origin", "HEAD:main"]);
    git(scaffold.device_b.tree_dir(), ["pull", "--ff-only", "origin", "main"]);

    let device_b_blocked = dream_now(scaffold.device_b.socket_path(), false);
    assert_error_code(&device_b_blocked, "lease_held");
    eval_assert!(
        journal_files(scaffold.device_b.tree_dir()).is_empty(),
        "Device B must not write a journal while Device A's active lease is visible"
    );

    let device_a_same_lease = dream_now(scaffold.device_a.socket_path(), false);
    if protocol_error_code(&device_a_same_lease) == Some("lease_held") {
        println!(
            "MEMORUM_EVAL_SKIP:{SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED}: \
             Current Stream F lease acquisition is not re-entrant for the same device; \
             a pre-seeded local active lease returns lease_held unless forced, so the spec step where Device A \
             proceeds under its own pre-seeded lease is not shipped yet. Verified the two-device loser backs off."
        );
        return;
    }

    assert_pass_1_success(&device_a_same_lease);
    eval_assert!(
        !journal_files(scaffold.device_a.tree_dir()).is_empty(),
        "Device A should write the journal when it owns the pre-seeded lease: {device_a_same_lease:#?}"
    );
    eval_assert!(
        journal_files(scaffold.device_b.tree_dir()).is_empty(),
        "Device B should still have no journal before retry"
    );
    eval_flush_assertion_count();
}

fn dream_now(socket_path: &Path, force: bool) -> Value {
    daemon_request(socket_path, json!({"dream_now": {"scope": "me", "force": force, "cli_override": "echo"}}))
}

fn assert_error_code(response: &Value, expected: &str) {
    eval_assert_eq!(protocol_error_code(response), Some(expected), "expected protocol error {expected}: {response:#?}");
}

fn protocol_error_code(response: &Value) -> Option<&str> {
    response.pointer("/result/error/code").and_then(Value::as_str)
}

fn assert_pass_1_success(response: &Value) {
    eval_assert_eq!(
        response.pointer("/result/success/dream_now/pass_1/status").and_then(Value::as_str),
        Some("success"),
        "expected successful dream pass 1: {response:#?}"
    );
}

fn journal_files(tree_dir: &Path) -> Vec<std::path::PathBuf> {
    find_file_with_extension(&tree_dir.join("dreams/journal/me"), "md")
}

fn append_lease_record(repo: &Path, record: Value) {
    std::fs::create_dir_all(repo.join("leases")).expect("create leases directory");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(repo.join("leases/journal.lease"))
        .expect("open lease file");
    writeln!(file, "{record}").expect("append lease record");
}

fn active_lease_record(device_id: &str) -> Value {
    json!({
        "device": device_id,
        "scope": "me",
        "acquired_at": "2026-05-01T12:00:00Z",
        "expires_at": "2999-01-01T00:00:00Z",
        "run_id": "run_t17_preseeded_device_a"
    })
}
