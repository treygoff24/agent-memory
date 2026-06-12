use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use serde_json::{json, Value};

use serial_test::serial;

use crate::support::{grounding_source_ref, promoted_project_meta, write_id, write_project_file, DEFAULT_PROJECT_ID};

const FRESH: &str = "The production database is PostgreSQL 16.";

#[tokio::test]
#[serial]
async fn temporal_validity_fields_are_not_silently_ignored_and_fresh_memory_is_currently_recalled() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()).with_cwd(&project_cwd));

    let expired_meta = json!({
        "namespace": "project",
        "type": "claim",
        "confidence": 0.95,
        "source_kind": "agent_primary",
        "source_ref": grounding_source_ref("t12-expired"),
        "explicit_user_context": true,
        "valid_until": "2025-01-01"
    })
    .to_string();
    let expired = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: "The production database is PostgreSQL 13.".to_owned(),
            title: Some("Expired database version".to_owned()),
            meta_json: expired_meta,
        }])
        .await;
    assert_temporal_field_rejected(
        expired.last_write_json.as_deref().expect("expired response captured"),
        "valid_until",
    );

    let future_meta = json!({
        "namespace": "project",
        "type": "claim",
        "confidence": 0.95,
        "source_kind": "agent_primary",
        "source_ref": grounding_source_ref("t12-future"),
        "explicit_user_context": true,
        "valid_from": "2030-01-01"
    })
    .to_string();
    let future = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: "The production database is PostgreSQL 18.".to_owned(),
            title: Some("Future database version".to_owned()),
            meta_json: future_meta,
        }])
        .await;
    assert_temporal_field_rejected(future.last_write_json.as_deref().expect("future response captured"), "valid_from");

    let fresh = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: FRESH.to_owned(),
            title: Some("Fresh database version".to_owned()),
            meta_json: promoted_project_meta("t12-fresh", "claim"),
        }])
        .await;
    eval_assert_eq!(fresh.last_write_outcome.as_deref(), Some("promoted"), "{fresh:#?}");
    let fresh_id = write_id(&fresh);

    let observations = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "production database PostgreSQL version".to_owned(), namespace: None },
            SimulatorAction::Get { id: fresh_id.clone() },
        ])
        .await;
    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    eval_assert!(assert_xml_valid(recall_block).is_ok(), "startup recall block is valid XML");
    eval_assert!(recall_block.contains(&fresh_id), "fresh memory should be recalled:\n{recall_block}");
    eval_assert!(
        recall_block.contains("Fresh database version"),
        "fresh memory summary should be recalled:\n{recall_block}"
    );
    eval_assert!(
        !recall_block.contains("PostgreSQL 13"),
        "rejected expired memory must not be recalled:\n{recall_block}"
    );
    eval_assert!(
        !recall_block.contains("PostgreSQL 18"),
        "rejected future memory must not be recalled:\n{recall_block}"
    );

    let get_json = observations.last_get_json.as_deref().expect("get response captured");
    eval_assert!(get_json.contains(FRESH), "memory_get should return the fresh body:\n{get_json}");

    eval_flush_assertion_count();
}

fn assert_temporal_field_rejected(response_json: &str, field: &str) {
    let parsed: Value = serde_json::from_str(response_json).expect("daemon response is JSON");
    let error = parsed
        .pointer("/result/error")
        .unwrap_or_else(|| panic!("expected invalid-request error for {field}, got:\n{response_json}"));
    eval_assert_eq!(error.get("code").and_then(Value::as_str), Some("invalid_request"), "{response_json}");
    eval_assert!(
        error.get("message").and_then(Value::as_str).is_some_and(|message| message.contains(field)),
        "error should name rejected field {field}:\n{response_json}"
    );
    eval_assert!(payload_is_absent(response_json, "governance_write"));
}

fn payload_is_absent(response_json: &str, name: &str) -> bool {
    let parsed: Value = serde_json::from_str(response_json).expect("daemon response is JSON");
    parsed.pointer(&format!("/result/success/{name}")).is_none()
}
