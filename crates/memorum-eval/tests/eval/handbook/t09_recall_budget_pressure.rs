use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};
use memorum_eval::{eval_assert, eval_assert_eq, eval_flush_assertion_count};
use serde_json::Value;

use serial_test::serial;

use crate::support::{
    payload, promoted_project_meta, write_id_or_materialized_file, write_project_file, DEFAULT_PROJECT_ID,
};

const GOLD_SENTINEL: &str = "EVAL_GOLD_BUDGET_SENTINEL";

#[tokio::test]
#[serial]
async fn recall_budget_pressure_keeps_high_value_gold_memory_and_reports_omissions() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    for index in 0..20 {
        let body = format!(
            "Budget pressure competitor {index:02} ent_budget_test. This deliberately long summary competes for recall budget and should be droppable under pressure. {}",
            "lower priority context ".repeat(8)
        );
        let observations = agent
            .run_script([SimulatorAction::WriteWithMetaJson {
                body,
                title: Some(format!("Budget competitor {index:02} {}", "context ".repeat(12))),
                meta_json: promoted_project_meta(&format!("t09-competitor-{index}"), "claim"),
            }])
            .await;
        eval_assert_eq!(observations.last_write_outcome.as_deref(), Some("promoted"), "{observations:#?}");
    }

    let gold_body = format!("{GOLD_SENTINEL}: ent_budget_test production incident owner is release engineering.");
    let gold = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: gold_body,
            title: Some(format!("{GOLD_SENTINEL} gold budget memory")),
            meta_json: crate::support::governed_meta_json(crate::support::GovernedMetaJson {
                namespace: "project",
                memory_type: "claim",
                confidence: 0.99,
                source_kind: "agent_primary",
                source_ref: Some(crate::support::grounding_source_ref("t09-gold")),
                explicit_user_context: true,
            }),
        }])
        .await;
    let gold_id = write_id_or_materialized_file(&gold, scaffold.tree_dir(), GOLD_SENTINEL);

    let mut newer_competitor_ids = Vec::new();
    for index in 20..39 {
        let marker = format!("EVAL_NEWER_BUDGET_COMPETITOR_{index:02}");
        let body = format!(
            "Budget pressure competitor {index:02} {marker} ent_budget_test. This deliberately long summary competes for recall budget and should be droppable under pressure. {}",
            "lower priority context ".repeat(8)
        );
        let observations = agent
            .run_script([SimulatorAction::WriteWithMetaJson {
                body,
                title: Some(format!("Budget competitor {index:02} {}", "context ".repeat(12))),
                meta_json: promoted_project_meta(&format!("t09-competitor-{index}"), "claim"),
            }])
            .await;
        eval_assert_eq!(observations.last_write_outcome.as_deref(), Some("promoted"), "{observations:#?}");
        newer_competitor_ids.push(write_id_or_materialized_file(&observations, scaffold.tree_dir(), &marker));
    }

    let observations = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::StartupWithBudget { since_event_id: None, budget_tokens: 512 },
        ])
        .await;

    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    eval_assert!(assert_xml_valid(recall_block).is_ok(), "startup recall block is valid XML");
    eval_assert!(recall_block.contains(&gold_id), "gold memory should survive budget pressure:\n{recall_block}");
    eval_assert!(recall_block.contains(GOLD_SENTINEL), "gold sentinel should be visible in recall:\n{recall_block}");

    let startup_json = observations.last_startup_json.as_deref().expect("startup response captured");
    let startup = payload(startup_json, "startup");
    let explanation = startup.get("recall_explanation").expect("recall_explanation present");
    let omitted = explanation.get("omitted").and_then(Value::as_array).expect("omitted array present");
    eval_assert!(!omitted.is_empty(), "budget pressure should omit at least one memory:\n{startup_json}");
    eval_assert!(
        omitted.iter().all(|item| item.get("id").and_then(Value::as_str) != Some(gold_id.as_str())),
        "gold memory must not be omitted: {omitted:#?}"
    );
    eval_assert!(
        newer_competitor_ids.iter().any(|competitor_id| {
            omitted.iter().any(|item| item.get("id").and_then(Value::as_str) == Some(competitor_id.as_str()))
        }),
        "budget pressure should omit at least one newer low-priority competitor; newer ids={newer_competitor_ids:?}, omitted={omitted:#?}"
    );
    let recent_section = explanation
        .get("sections")
        .and_then(Value::as_array)
        .and_then(|sections| {
            sections.iter().find(|section| section.get("name").and_then(Value::as_str) == Some("recent-memory"))
        })
        .expect("recent-memory explanation present");
    eval_assert!(
        recent_section.get("omitted_count").and_then(Value::as_u64).is_some_and(|count| count > 0),
        "recent-memory omitted_count should reflect budget drops: {recent_section:#?}"
    );
    eval_assert_eq!(
        explanation.get("budget_tokens").and_then(Value::as_u64),
        Some(512),
        "startup explanation should preserve configured budget:\n{startup_json}"
    );

    eval_flush_assertion_count();
}
