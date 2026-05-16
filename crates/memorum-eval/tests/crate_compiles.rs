use memorum_eval::{assertions, daemon_scaffold, harness_runner, orchestrator, simulator, EvalCli};

#[test]
fn crate_reexports_public_modules_and_binary_name_resolves() {
    let command = EvalCli::command();

    assert_eq!(command.get_name(), "memorum-eval");
    let binary_path = std::path::Path::new(env!("CARGO_BIN_EXE_memorum-eval"));
    assert_eq!(binary_path.file_name().and_then(|name| name.to_str()), Some("memorum-eval"));

    let _ = orchestrator::EvalOrchestrator;
    let _ = simulator::SimulatorAgent::new(simulator::SimulatorConfig::default());
    let _ = harness_runner::HarnessRunner::new(harness_runner::RealHarness::Codex);
    let _ = daemon_scaffold::DaemonScaffoldConfig::default();
    assertions::assert_xml_valid("<memory-recall></memory-recall>").expect("empty recall block should be valid XML");
}
