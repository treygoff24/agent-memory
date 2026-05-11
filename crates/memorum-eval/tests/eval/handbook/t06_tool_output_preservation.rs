use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};

use serial_test::serial;

use crate::support::{
    memory_file_body, promoted_project_meta, search_hits, write_id, write_project_file, DEFAULT_PROJECT_ID,
};

const ARTIFACT_HANDLE: &str = "artifact://session_abc/migration-dry-run-2026-05-01.log";

#[tokio::test]
#[serial]
async fn artifact_memory_preserves_tool_output_handle_through_recall_search_and_get() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let artifact_body = format!(
        "Database migration dry-run output: 14 tables affected, 2 foreign key cycles detected. Full log at {ARTIFACT_HANDLE}"
    );
    let observations = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: artifact_body.clone(),
            title: Some("Migration dry-run artifact".to_owned()),
            meta_json: promoted_project_meta("t06-artifact", "artifact"),
        }])
        .await;
    eval_assert_eq!(
        observations.last_write_outcome.as_deref(),
        Some("promoted"),
        "artifact write should promote: {observations:#?}"
    );
    let artifact_id = write_id(&observations);

    let observations = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "database migration foreign key".to_owned(), namespace: None },
            SimulatorAction::Get { id: artifact_id.clone() },
        ])
        .await;

    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    eval_assert!(assert_xml_valid(recall_block).is_ok(), "startup recall block is valid XML");
    eval_assert!(
        recall_block.contains(&artifact_id),
        "startup recall should include artifact memory {artifact_id}:\n{recall_block}"
    );

    let search_json = observations.last_search_json.as_deref().expect("search response captured");
    let hits = search_hits(search_json);
    eval_assert!(
        hits.iter().any(|hit| {
            hit.get("id").and_then(|id| id.as_str()) == Some(artifact_id.as_str())
                && hit
                    .get("snippet")
                    .and_then(|snippet| snippet.as_str())
                    .is_some_and(|snippet| snippet.contains(ARTIFACT_HANDLE))
        }),
        "search should return artifact with handle intact:\n{search_json}"
    );

    let get_json = observations.last_get_json.as_deref().expect("get response captured");
    eval_assert!(get_json.contains(ARTIFACT_HANDLE), "memory_get should preserve artifact handle:\n{get_json}");

    let file = memory_file_body(scaffold.tree_dir(), &artifact_id);
    eval_assert!(file.contains("type: artifact"), "canonical file should persist artifact type:\n{file}");
    eval_assert!(file.contains(ARTIFACT_HANDLE), "canonical file should preserve artifact handle:\n{file}");

    eval_flush_assertion_count();
}
