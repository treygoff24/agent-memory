use std::fs;

use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use serde_json::Value;

use crate::support::{
    governed_meta_json, memory_file_body, payload, promoted_meta, search_total, write_id, write_project_file,
    GovernedMetaJson, DEFAULT_PROJECT_ID,
};

const FORGOTTEN_BODY: &str = "The fallback queue uses Redis 6. Entity ent_fallback_queue.";

#[tokio::test]
async fn forgotten_agent_memory_is_tombstoned_hidden_and_blocks_reinsertion() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let observations = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: FORGOTTEN_BODY.to_owned(),
            title: Some("Fallback queue tombstone fixture".to_owned()),
            meta_json: promoted_meta("agent", "t08-agent-fallback-queue", "claim"),
        }])
        .await;
    assert_eq!(observations.last_write_outcome.as_deref(), Some("promoted"), "{observations:#?}");
    let memory_id = write_id(&observations);

    let observations = agent
        .run_script([
            SimulatorAction::Forget { id: memory_id.clone(), reason: "user requested deletion in eval".to_owned() },
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "fallback queue Redis".to_owned(), namespace: None },
            SimulatorAction::Get { id: memory_id.clone() },
        ])
        .await;

    let forget_json = observations.last_forget_json.as_deref().expect("forget response captured");
    let forget = payload(forget_json, "governance_forget");
    assert_eq!(forget.get("status").and_then(Value::as_str), Some("tombstoned"), "{forget_json}");

    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    assert_xml_valid(recall_block).expect("startup recall block is valid XML");
    assert!(!recall_block.contains(&memory_id), "tombstoned memory must not be recalled:\n{recall_block}");
    assert!(!recall_block.contains(FORGOTTEN_BODY), "tombstoned body must not be recalled:\n{recall_block}");

    let search_json = observations.last_search_json.as_deref().expect("search response captured");
    assert_eq!(search_total(search_json), 0, "default search must not surface tombstoned memory:\n{search_json}");

    let tombstoned_file = memory_file_body(scaffold.tree_dir(), &memory_id);
    assert!(
        tombstoned_file.contains("status: tombstoned"),
        "canonical memory file should expose audit status:\n{tombstoned_file}"
    );

    let tombstone_file = scaffold.tree_dir().join("tombstones").join("memoryd-forget.jsonl");
    let tombstones = fs::read_to_string(&tombstone_file)
        .unwrap_or_else(|err| panic!("read tombstone file {}: {err}", tombstone_file.display()));
    assert!(tombstones.contains(&memory_id), "tombstone file should reference forgotten id:\n{tombstones}");

    let reinsertion = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: FORGOTTEN_BODY.to_owned(),
            title: Some("Fallback queue reinsertion attempt".to_owned()),
            meta_json: governed_meta_json(GovernedMetaJson {
                namespace: "agent",
                memory_type: "claim",
                confidence: 0.95,
                source_kind: "agent_primary",
                source_ref: Some(crate::support::grounding_source_ref("t08-reinsert")),
                explicit_user_context: true,
            }),
        }])
        .await;
    let write_json = reinsertion.last_write_json.as_deref().expect("reinsertion response captured");
    let write = payload(write_json, "governance_write");
    assert_eq!(write.get("status").and_then(Value::as_str), Some("refused"), "{write_json}");
    assert_eq!(write.get("reason").and_then(Value::as_str), Some("tombstone"), "{write_json}");
}
