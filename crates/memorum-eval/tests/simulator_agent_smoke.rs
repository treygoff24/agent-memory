use memorum_eval::block_on;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{GovernanceMeta, LastWriteNotRefused, SimulatorAction, SimulatorAgent, SimulatorConfig};

#[test]
fn simulator_agent_runs_startup_search_write_script_against_daemon() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
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
                SimulatorAction::Assert { condition: LastWriteNotRefused },
            ])
            .await;

        assert_ne!(
            observations.last_write_outcome.as_deref(),
            Some("refused"),
            "write should not be refused: {observations:#?}"
        );
    });
}
