use std::time::Duration;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{GovernanceMeta, SimulatorAction, SimulatorAgent, SimulatorConfig};
use tokio::time::timeout;

#[tokio::test]
async fn privacy_filter_rejects_luhn_valid_card_number() {
    let scaffold = fresh_scaffold().await;
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let observations = agent
        .run_script([SimulatorAction::Write {
            body: "meta privacy fixture contains 4111111111111111 and must not persist".to_owned(),
            title: None,
            meta: GovernanceMeta {
                confidence: 0.95,
                source_kind: "agent_primary".to_owned(),
                source_ref: Some("meta_privacy_filter_connectivity".to_owned()),
            },
        }])
        .await;

    let response = observations.last_write_json.as_deref().expect("write response should be captured");
    let lower = response.to_ascii_lowercase();
    assert!(
        observations.last_write_outcome.as_deref() == Some("refused")
            || lower.contains(r#""code":"privacy_error""#)
            || lower.contains("privacy refused")
            || lower.contains("policy"),
        "expected privacy/policy refusal shape, got: {response}"
    );
}

async fn fresh_scaffold() -> DaemonScaffold {
    timeout(Duration::from_secs(10), DaemonScaffold::fresh()).await.expect("fresh daemon scaffold should not hang")
}
