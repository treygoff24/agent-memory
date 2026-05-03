use std::collections::HashSet;

use memorum_eval::assertions::{assert_xml_valid, parse_recall_block};
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_flush_assertion_count};

use crate::support::{
    memory_file_body, promoted_project_meta, search_hits, write_id, write_project_file, DEFAULT_PROJECT_ID,
};

#[tokio::test]
async fn simulated_compaction_resumption_preserves_active_working_state_without_duplicates() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let mut first_round_ids = Vec::new();
    for index in 0..10 {
        let observations = agent
            .run_script([SimulatorAction::WriteWithMetaJson {
                body: format!("Working state round one {index}: auth migration investigation fact t10_key_{index}."),
                title: Some(format!("Working state round one {index}")),
                meta_json: promoted_project_meta(&format!("t10-round-one-{index}"), "claim"),
            }])
            .await;
        first_round_ids.push(write_id(&observations));
    }

    let first_resumption = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd.clone()), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "t10_key_3 auth migration investigation".to_owned(), namespace: None },
        ])
        .await;
    let first_block = first_resumption.last_startup_block.as_deref().expect("first startup recall block captured");
    eval_assert!(assert_xml_valid(first_block).is_ok(), "first recall block is valid XML");
    let recalled_first = first_round_ids.iter().filter(|id| first_block.contains(id.as_str())).count();
    eval_assert!(
        recalled_first >= 8,
        "first resumption should recall >=8 of 10 memories, got {recalled_first}:\n{first_block}"
    );
    let search_json = first_resumption.last_search_json.as_deref().expect("first search response captured");
    eval_assert!(
        search_hits(search_json).iter().any(|hit| hit
            .get("snippet")
            .and_then(|snippet| snippet.as_str())
            .is_some_and(|snippet| snippet.contains("t10_key_3"))),
        "search after first resumption should find pre-compaction key claim:\n{search_json}"
    );

    let mut second_round_ids = Vec::new();
    for index in 0..5 {
        let observations = agent
            .run_script([SimulatorAction::WriteWithMetaJson {
                body: format!("Working state round two {index}: deployment audit follow-up fact t10_followup_{index}."),
                title: Some(format!("Working state round two {index}")),
                meta_json: promoted_project_meta(&format!("t10-round-two-{index}"), "claim"),
            }])
            .await;
        second_round_ids.push(write_id(&observations));
    }

    let second_resumption = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "t10_followup_3 deployment audit".to_owned(), namespace: None },
        ])
        .await;
    let second_block = second_resumption.last_startup_block.as_deref().expect("second startup recall block captured");
    eval_assert!(assert_xml_valid(second_block).is_ok(), "second recall block is valid XML");
    eval_assert!(
        first_round_ids.iter().any(|id| second_block.contains(id.as_str())),
        "second resumption should include original working state:\n{second_block}"
    );
    eval_assert!(
        second_round_ids.iter().any(|id| second_block.contains(id.as_str())),
        "second resumption should include newer working state:\n{second_block}"
    );
    assert_no_duplicate_recall_ids(second_block);

    for id in first_round_ids.iter().chain(second_round_ids.iter()) {
        let file = memory_file_body(scaffold.tree_dir(), id);
        eval_assert!(file.contains("status: active"), "working state {id} should remain active:\n{file}");
    }

    eval_flush_assertion_count();
}

fn assert_no_duplicate_recall_ids(recall_block: &str) {
    let mut seen = HashSet::new();
    let block = parse_recall_block(recall_block).expect("recall XML was already validated");
    for memory in block.memories {
        eval_assert!(
            seen.insert(memory.ref_id.clone()),
            "duplicate recall id {} in block:\n{recall_block}",
            memory.ref_id
        );
    }
}
