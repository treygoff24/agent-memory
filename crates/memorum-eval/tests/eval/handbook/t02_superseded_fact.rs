use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{GovernanceMeta, SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};

use serial_test::serial;

use crate::support::{
    memory_file_body, promoted_project_meta, search_hits, supersede_new_id, write_id, write_project_file,
    DEFAULT_PROJECT_ID,
};

#[tokio::test]
#[serial]
async fn superseded_fact_loses_to_replacement_in_search_and_recall() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()).with_cwd(&project_cwd));

    let original = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: "The primary database is PostgreSQL 14.".to_owned(),
            title: Some("Primary database old version".to_owned()),
            meta_json: promoted_project_meta("t02-original", "claim"),
        }])
        .await;
    let old_id = write_id(&original);

    let superseded = agent
        .run_script([SimulatorAction::Supersede {
            old_id: old_id.clone(),
            new_body: "The primary database is PostgreSQL 16.".to_owned(),
            reason: "version correction from deployment inventory".to_owned(),
            meta: GovernanceMeta {
                confidence: 0.96,
                source_kind: "agent_primary".to_owned(),
                source_ref: Some("t02-supersession".to_owned()),
            },
        }])
        .await;
    eval_assert_eq!(
        superseded.last_supersede_outcome.as_deref(),
        Some("promoted"),
        "supersede should promote: {superseded:#?}"
    );
    let new_id = supersede_new_id(&superseded);

    let observations = agent
        .run_script([
            SimulatorAction::Search { query: "primary database PostgreSQL".to_owned(), namespace: None },
            SimulatorAction::Get { id: old_id.clone() },
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
        ])
        .await;

    let search_json = observations.last_search_json.as_deref().expect("search response captured");
    let hits = search_hits(search_json);
    eval_assert!(!hits.is_empty(), "search should find the replacement claim:\n{search_json}");
    eval_assert_eq!(
        hits[0].get("id").and_then(|id| id.as_str()),
        Some(new_id.as_str()),
        "PostgreSQL 16 should rank first: {hits:#?}"
    );
    eval_assert!(hits[0]
        .get("snippet")
        .and_then(|snippet| snippet.as_str())
        .is_some_and(|snippet| snippet.contains("PostgreSQL 16")));

    let old_file = memory_file_body(scaffold.tree_dir(), &old_id);
    eval_assert!(old_file.contains("status: superseded"), "old memory should be marked superseded:\n{old_file}");
    eval_assert!(old_file.contains(&new_id), "old memory should point at replacement {new_id}:\n{old_file}");

    let new_file = memory_file_body(scaffold.tree_dir(), &new_id);
    eval_assert!(new_file.contains("PostgreSQL 16"));
    eval_assert!(new_file.contains(&old_id), "replacement should record supersedes {old_id}:\n{new_file}");

    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    eval_assert!(assert_xml_valid(recall_block).is_ok(), "startup recall block is valid XML");
    eval_assert!(recall_block.contains(&new_id), "recall should include replacement {new_id}:\n{recall_block}");
    eval_assert!(
        !recall_block.contains(&old_id),
        "recall should not include superseded old id {old_id}:\n{recall_block}"
    );
    eval_assert!(!recall_block.contains("PostgreSQL 14"), "recall should not include old body:\n{recall_block}");

    eval_flush_assertion_count();
}
