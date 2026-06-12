use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_flush_assertion_count};

use serial_test::serial;

use crate::support::{promoted_project_meta, write_id, write_project_file, DEFAULT_PROJECT_ID};

#[tokio::test]
#[serial]
async fn project_binding_filters_project_memory_from_other_project_recall() {
    let scaffold = DaemonScaffold::fresh().await;
    let alpha_cwd = scaffold.tree_dir().join("proj-alpha");
    let beta_cwd = scaffold.tree_dir().join("proj-beta");
    write_project_file(&alpha_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    write_project_file(&beta_cwd, "proj_beta", "Project Beta");

    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()).with_cwd(&alpha_cwd));
    let alpha = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: "The API uses JWT authentication. Entity ent_api_auth belongs to proj_alpha.".to_owned(),
            title: Some("Project Alpha API uses JWT authentication".to_owned()),
            meta_json: promoted_project_meta("t03-alpha-jwt", "claim"),
        }])
        .await;
    let alpha_id = write_id(&alpha);

    let alpha_recall = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(alpha_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
        ])
        .await;
    let alpha_block = alpha_recall.last_startup_block.as_deref().expect("alpha startup recall block captured");
    eval_assert!(assert_xml_valid(alpha_block).is_ok(), "alpha recall block is valid XML");
    eval_assert!(alpha_block.contains(&alpha_id), "alpha project should recall JWT memory {alpha_id}:\n{alpha_block}");
    eval_assert!(alpha_block.contains("JWT authentication"), "alpha recall should include JWT fact:\n{alpha_block}");

    let beta_recall = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(beta_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
        ])
        .await;
    let beta_block = beta_recall.last_startup_block.as_deref().expect("beta startup recall block captured");
    eval_assert!(assert_xml_valid(beta_block).is_ok(), "beta recall block is valid XML");
    eval_assert!(
        beta_block.contains("namespace: project:proj_beta"),
        "beta binding should resolve to proj_beta:\n{beta_block}"
    );
    eval_assert!(
        !beta_block.contains(&alpha_id),
        "beta project must not recall alpha JWT memory {alpha_id}:\n{beta_block}"
    );
    eval_assert!(
        !beta_block.contains("JWT authentication"),
        "beta recall should not leak alpha JWT fact:\n{beta_block}"
    );

    eval_flush_assertion_count();
}
