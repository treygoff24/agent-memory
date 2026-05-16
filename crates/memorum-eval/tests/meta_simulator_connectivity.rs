use std::time::Duration;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use tokio::time::timeout;

#[tokio::test]
async fn simulator_startup_receives_startup_response_from_daemon() {
    let scaffold = fresh_scaffold().await;
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let observations = agent.run_script([SimulatorAction::Startup { since_event_id: None }]).await;

    let startup_json = observations.last_startup_json.as_deref().expect("startup response should be captured");
    assert!(
        startup_json.contains(r#""startup""#),
        "expected ResponsePayload::Startup-shaped JSON, got: {startup_json}"
    );
    assert!(
        observations.last_startup_block.is_some(),
        "startup response should expose the rendered recall/startup block: {startup_json}"
    );
}

async fn fresh_scaffold() -> DaemonScaffold {
    timeout(Duration::from_secs(10), DaemonScaffold::fresh()).await.expect("fresh daemon scaffold should not hang")
}
