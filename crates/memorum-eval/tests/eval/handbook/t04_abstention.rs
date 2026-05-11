use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};

use serial_test::serial;

use crate::support::{assert_success_response, search_total, write_project_file, DEFAULT_PROJECT_ID};

const NOVEL_TOPIC: &str = "EVAL_NOVEL_TOPIC_ZK8T";

#[tokio::test]
#[serial]
async fn novel_topic_search_and_startup_abstain_without_error() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let observations = agent
        .run_script([
            SimulatorAction::Search { query: NOVEL_TOPIC.to_owned(), namespace: None },
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
        ])
        .await;

    let search_json = observations.last_search_json.as_deref().expect("search response captured");
    assert_success_response(search_json);
    eval_assert_eq!(search_total(search_json), 0, "novel topic should return zero search hits:\n{search_json}");

    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    eval_assert!(assert_xml_valid(recall_block).is_ok(), "empty-recall startup block is valid XML");
    eval_assert!(
        !recall_block.contains(NOVEL_TOPIC),
        "startup must not fabricate novel topic memories:\n{recall_block}"
    );

    eval_flush_assertion_count();
}
