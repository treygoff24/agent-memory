use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use serde_json::Value;

use crate::support::{governed_meta_json, payload, write_project_file, GovernedMetaJson, DEFAULT_PROJECT_ID};

const DISCOVERY: &str =
    "The auth service requires PKCE for public clients. Discovered during OAuth flow investigation.";

#[tokio::test]
async fn subagent_writeback_requires_a_spawn_registry_before_parent_recall() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");

    let mut parent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));
    parent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd.clone()), harness: Some("claude-code".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
        ])
        .await;

    let mut subagent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));
    let observations = subagent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: DISCOVERY.to_owned(),
            title: Some("Subagent PKCE discovery".to_owned()),
            meta_json: governed_meta_json(GovernedMetaJson {
                namespace: "project",
                memory_type: "claim",
                confidence: 0.85,
                source_kind: "subagent",
                source_ref: Some("session-spawn:memorum-eval-parent-session".to_owned()),
                explicit_user_context: true,
            }),
        }])
        .await;

    let write_json = observations.last_write_json.as_deref().expect("subagent write response captured");
    let write = payload(write_json, "governance_write");
    eval_assert_eq!(write.get("status").and_then(Value::as_str), Some("refused"), "{write_json}");
    eval_assert_eq!(write.get("reason").and_then(Value::as_str), Some("grounding"), "{write_json}");
    eval_assert!(
        write.get("id").is_none_or(Value::is_null),
        "refused subagent write must not allocate a memory id: {write_json}"
    );

    let observations = parent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("claude-code".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "PKCE auth public clients".to_owned(), namespace: None },
        ])
        .await;

    let recall_block = observations.last_startup_block.as_deref().expect("parent startup recall block captured");
    eval_assert!(
        !recall_block.contains(DISCOVERY),
        "ungrounded subagent discovery must not appear in parent recall until spawn refs are resolvable:\n{recall_block}"
    );
    let search_json = observations.last_search_json.as_deref().expect("parent search response captured");
    let search = payload(search_json, "search");
    eval_assert_eq!(search.get("total").and_then(Value::as_u64), Some(0), "{search_json}");

    eval_flush_assertion_count();
}
