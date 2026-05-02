use memorum_eval::assertions::assert_xml_valid;
use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{GovernanceMeta, SimulatorAction, SimulatorAgent, SimulatorConfig};
use serde_json::Value;

use crate::support::{
    governed_meta_json, payload, search_hits, supersede_new_id, write_id, write_project_file, GovernedMetaJson,
    DEFAULT_PROJECT_ID,
};

const WRONG_CLAIM: &str = "The authentication flow uses RS256 JWT tokens.";
const CORRECT_CLAIM: &str = "The authentication flow uses ES256 JWT tokens.";

#[tokio::test]
async fn self_poisoned_candidate_cannot_ground_its_own_confidence_escalation() {
    let scaffold = DaemonScaffold::fresh().await;
    let project_cwd = scaffold.tree_dir().join("proj-alpha");
    write_project_file(&project_cwd, DEFAULT_PROJECT_ID, "Project Alpha");
    let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

    let candidate = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: WRONG_CLAIM.to_owned(),
            title: Some("Tentative wrong auth claim".to_owned()),
            meta_json: governed_meta_json(GovernedMetaJson {
                namespace: "project",
                memory_type: "claim",
                confidence: 0.50,
                source_kind: "agent_primary",
                source_ref: Some(crate::support::grounding_source_ref("t11-low-confidence")),
                explicit_user_context: true,
            }),
        }])
        .await;
    let candidate_json = candidate.last_write_json.as_deref().expect("candidate write response captured");
    let candidate_write = payload(candidate_json, "governance_write");
    assert_eq!(candidate_write.get("status").and_then(Value::as_str), Some("candidate"), "{candidate_json}");
    let candidate_id = write_id(&candidate);

    let observations = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd.clone()), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
        ])
        .await;
    let recall_block = observations.last_startup_block.as_deref().expect("startup recall block captured");
    assert_xml_valid(recall_block).expect("startup recall block is valid XML");
    assert!(!recall_block.contains(&candidate_id), "candidate must not appear as factual recall:\n{recall_block}");
    assert!(
        !recall_block.contains(WRONG_CLAIM),
        "wrong candidate body must not appear in factual recall:\n{recall_block}"
    );

    let escalation = agent
        .run_script([SimulatorAction::WriteWithMetaJson {
            body: WRONG_CLAIM.to_owned(),
            title: Some("Self-grounded wrong auth claim".to_owned()),
            meta_json: governed_meta_json(GovernedMetaJson {
                namespace: "project",
                memory_type: "claim",
                confidence: 0.90,
                source_kind: "agent_primary",
                source_ref: Some(format!("memory:{candidate_id}")),
                explicit_user_context: false,
            }),
        }])
        .await;
    let escalation_json = escalation.last_write_json.as_deref().expect("escalation response captured");
    let escalation_write = payload(escalation_json, "governance_write");
    assert_eq!(escalation_write.get("status").and_then(Value::as_str), Some("refused"), "{escalation_json}");
    assert_eq!(escalation_write.get("reason").and_then(Value::as_str), Some("grounding"), "{escalation_json}");

    let superseded = agent
        .run_script([SimulatorAction::Supersede {
            old_id: candidate_id.clone(),
            new_body: CORRECT_CLAIM.to_owned(),
            reason: "verified algorithm from auth service configuration".to_owned(),
            meta: GovernanceMeta {
                confidence: 0.95,
                source_kind: "agent_primary".to_owned(),
                source_ref: Some("t11-correct-supersession".to_owned()),
            },
        }])
        .await;
    assert_eq!(superseded.last_supersede_outcome.as_deref(), Some("promoted"), "{superseded:#?}");
    let correct_id = supersede_new_id(&superseded);

    let observations = agent
        .run_script([
            SimulatorAction::NewSession { cwd: Some(project_cwd), harness: Some("memorum-eval".to_owned()) },
            SimulatorAction::Startup { since_event_id: None },
            SimulatorAction::Search { query: "authentication flow JWT tokens".to_owned(), namespace: None },
        ])
        .await;
    let recall_block = observations.last_startup_block.as_deref().expect("post-correction recall block captured");
    assert!(recall_block.contains(&correct_id), "correct supersession should be recalled:\n{recall_block}");
    assert!(
        !recall_block.contains(&candidate_id),
        "superseded wrong candidate should not be recalled:\n{recall_block}"
    );

    let search_json = observations.last_search_json.as_deref().expect("search response captured");
    let hits = search_hits(search_json);
    assert_eq!(
        hits.first().and_then(|hit| hit.get("id")).and_then(Value::as_str),
        Some(correct_id.as_str()),
        "correct ES256 claim should rank before superseded/candidate rows:\n{search_json}"
    );
    assert!(
        hits.iter().any(|hit| hit.get("id").and_then(Value::as_str) == Some(correct_id.as_str())
            && hit.get("snippet").and_then(Value::as_str).is_some_and(|snippet| snippet.contains("ES256"))),
        "search should return correct ES256 claim:\n{search_json}"
    );
}
