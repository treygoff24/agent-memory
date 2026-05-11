use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};

use serial_test::serial;

use crate::support::{
    promoted_project_meta, search_hits, startup_invoked_total, write_id, write_project_file, DEFAULT_PROJECT_ID,
};

const SENTINEL: &str = "EVAL_SENTINEL_XF7Q9";

#[tokio::test]
#[serial]
async fn exact_identifier_survives_startup_recall_and_search() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");

    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));
    let sentinel_body = format!("id: mem_test_001 exact recall sentinel {SENTINEL}");
    let observations = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: sentinel_body.clone(),
            title: Some(format!("Exact identifier recall sentinel {SENTINEL}")),
            meta_json: promoted_project_meta("t01-sentinel", "claim"),
        }])
        .await;
    let sentinel_id = write_id(&observations);

    let competition = (0..20).map(|index| SimulatorAction::WriteWithMetaJson {
        body: format!("Recall competition memory {index}: unrelated operational note."),
        title: Some(format!("Recall competition {index}")),
        meta_json: promoted_project_meta(&format!("t01-competition-{index}"), "claim"),
    });
    agent.run_script(competition).await;

    let observations = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: SENTINEL.to_owned(), namespace: None },
            SimulatorAction::Status,
        ])
        .await;

    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    eval_assert!(assert_xml_valid(recall_block).is_ok(), "startup recall block is valid XML");
    eval_assert!(
        recall_block.contains(&sentinel_id),
        "recall block should include sentinel id {sentinel_id}:\n{recall_block}"
    );
    eval_assert!(recall_block.contains(SENTINEL), "recall block should include sentinel body text:\n{recall_block}");

    let search_json = observations.last_search_json.as_deref().expect("search response captured");
    let hits = search_hits(search_json);
    let top = hits.first().unwrap_or_else(|| panic!("sentinel search returned no hits:\n{search_json}"));
    eval_assert_eq!(top.get("id").and_then(|id| id.as_str()), Some(sentinel_id.as_str()));
    eval_assert!(
        top.get("snippet").and_then(|snippet| snippet.as_str()).is_some_and(|snippet| snippet.contains(SENTINEL)),
        "top search hit should preserve sentinel body text: {top:#?}"
    );
    eval_assert_eq!(
        hits.iter().filter(|hit| hit.get("id").and_then(|id| id.as_str()) == Some(sentinel_id.as_str())).count(),
        1
    );

    let status_json = observations.last_status_json.as_deref().expect("status response captured");
    eval_assert!(startup_invoked_total(status_json) >= 1, "startup counter should increment: {status_json}");

    // Print assertion count for orchestrator JSON output accuracy. (H-B3)
    eval_flush_assertion_count();
}
