use std::time::Duration;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{AssertionSpec, GovernanceMeta, SimulatorAction, SimulatorAgent, SimulatorConfig};
use serde_json::Value;
use tokio::time::timeout;

#[tokio::test]
async fn simulator_agent_runs_startup_search_write_script_against_daemon() {
    let scaffold = fresh_scaffold().await;
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let observations = agent
        .run_script([
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "test".to_owned(), namespace: None },
            SimulatorAction::Write {
                body: "hello eval world".to_owned(),
                title: None,
                meta: GovernanceMeta {
                    confidence: 0.95,
                    source_kind: "agent_primary".to_owned(),
                    source_ref: Some("eval_test_1".to_owned()),
                },
            },
            SimulatorAction::Assert { condition: AssertionSpec::LastWriteStatusIsNotRefused },
        ])
        .await;

    let startup =
        success_payload(observations.last_startup_json.as_deref().expect("startup response captured"), "startup");
    assert!(startup["recall_block"].is_string(), "startup response must include recall_block: {startup:#}");
    let search = success_payload(observations.last_search_json.as_deref().expect("search response captured"), "search");
    assert!(search["total"].as_u64().is_some(), "search response must include total: {search:#}");

    let write =
        success_payload(observations.last_write_json.as_deref().expect("write response captured"), "governance_write");
    assert!(
        matches!(write["status"].as_str(), Some("promoted" | "candidate")),
        "write status should be a successful substrate write: {write:#}"
    );
    assert!(write["id"].as_str().is_some(), "successful write should return an id: {write:#}");
    assert_eq!(
        observations.last_write_outcome.as_deref(),
        write["status"].as_str(),
        "extracted last_write_outcome should match parsed response"
    );
}

fn success_payload(response_json: &str, payload_key: &str) -> Value {
    let response: Value = serde_json::from_str(response_json).expect("response is valid JSON");
    response
        .get("result")
        .and_then(|result| result.get("success"))
        .and_then(|success| success.get(payload_key))
        .cloned()
        .unwrap_or_else(|| panic!("{payload_key} response was not successful: {response:#}"))
}

async fn fresh_scaffold() -> DaemonScaffold {
    timeout(Duration::from_secs(10), DaemonScaffold::fresh()).await.expect("fresh daemon scaffold should not hang")
}
