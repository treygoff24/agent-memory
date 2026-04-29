#[path = "../../memory-test-support/src/governance.rs"]
mod governance_fixtures;

use std::path::Path;

use governance_fixtures::{GovernanceActor, GovernanceScope, ACTOR_FIXTURES, RELATION_FIXTURES, SCOPE_POLICY_FIXTURES};
use memory_substrate::{InitOptions, MemoryId, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
};

#[tokio::test]
async fn governance_matrix_e2e_covers_actor_write_paths() {
    for fixture in ACTOR_FIXTURES {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;
        let grounding_file = temp.path().join("grounding.md");
        std::fs::write(&grounding_file, "local grounding for e2e actor matrix\n").expect("write grounding file");

        let write = write_memory(
            &substrate,
            WriteCase {
                request_id: fixture.name,
                body: fixture.claim,
                actor: fixture.actor,
                grounding_file: Some(&grounding_file),
                namespace: "agent",
            },
        )
        .await;

        match fixture.actor {
            GovernanceActor::User | GovernanceActor::GroundedAgent => {
                assert_eq!(write.status, GovernanceStatus::Promoted, "{}", fixture.name);
                assert!(write.id.is_some(), "{}: promoted writes persist a memory", fixture.name);
            }
            GovernanceActor::UngroundedAgent | GovernanceActor::Subagent => {
                assert_eq!(write.status, GovernanceStatus::Refused, "{}", fixture.name);
                assert_eq!(write.reason, Some(GovernanceRefusalReason::Grounding), "{}", fixture.name);
                assert!(write.id.is_none(), "{}: refused writes do not persist", fixture.name);
            }
        }
    }
}

#[tokio::test]
async fn governance_matrix_e2e_covers_duplicate_contradiction_refinement_and_tombstone_paths() {
    for fixture in RELATION_FIXTURES {
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;

        match fixture.relation {
            governance_fixtures::GovernanceRelation::Fresh => {
                let write = user_project_write(&substrate, fixture.name, fixture.claim).await;
                assert_eq!(write.status, GovernanceStatus::Promoted, "{}", fixture.name);
            }
            governance_fixtures::GovernanceRelation::Duplicate => {
                let first = user_project_write(&substrate, "duplicate-first", fixture.claim).await;
                let second = user_project_write(&substrate, "duplicate-second", fixture.claim).await;
                assert_eq!(second.status, GovernanceStatus::Promoted, "{}", fixture.name);
                assert_eq!(second.id, first.id, "{}: duplicate returns existing id", fixture.name);
                assert_eq!(second.existing_id, first.id, "{}: duplicate response includes existing id", fixture.name);
            }
            governance_fixtures::GovernanceRelation::Refinement
            | governance_fixtures::GovernanceRelation::Contradiction => {
                let first = user_project_write(&substrate, "active-seed", "The deployment target is staging.").await;
                assert_eq!(first.status, GovernanceStatus::Promoted, "seed active memory");

                let follow_up = user_project_write(&substrate, fixture.name, fixture.claim).await;
                assert_eq!(
                    follow_up.status,
                    GovernanceStatus::Promoted,
                    "{}: daemon should not quarantine unrelated writes without an actual similarity provider hit",
                    fixture.name
                );
            }
            governance_fixtures::GovernanceRelation::TombstoneHit => {
                let write = user_project_write(&substrate, fixture.name, fixture.claim).await;
                let id = write.id.expect("write persisted before forget");

                let response = handle_request(
                    &substrate,
                    RequestEnvelope::new(
                        "forget",
                        RequestPayload::Forget { id: id.clone(), reason: "matrix tombstone".to_string() },
                    ),
                )
                .await;
                let ResponseResult::Success(ResponsePayload::GovernanceForget(forget)) = response.result else {
                    panic!("expected governance forget response, got {:?}", response.result);
                };
                assert_eq!(forget.status, GovernanceStatus::Tombstoned, "{}", fixture.name);

                let memory = substrate.read_memory(&MemoryId::new(&id)).await.expect("tombstoned memory readable");
                assert_eq!(memory.frontmatter.status, memory_substrate::MemoryStatus::Tombstoned, "{}", fixture.name);
            }
        }
    }
}

#[tokio::test]
async fn governance_matrix_e2e_covers_daemon_supported_scope_policies() {
    for fixture in SCOPE_POLICY_FIXTURES {
        let Some(namespace) = daemon_namespace(fixture.scope) else {
            continue;
        };
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = init_substrate(&temp).await;
        let body = format!("{} scoped daemon writes select the expected policy.", fixture.name);
        let write = write_memory(
            &substrate,
            WriteCase {
                request_id: fixture.name,
                body: &body,
                actor: GovernanceActor::User,
                grounding_file: None,
                namespace,
            },
        )
        .await;

        assert_eq!(write.status, GovernanceStatus::Promoted, "{namespace}");
        assert_eq!(write.policy_applied.as_deref(), Some(fixture.policy_applied), "{namespace}");
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "dreaming-scope",
            RequestPayload::WriteMemory {
                body: "Dreaming scope is covered in the governance engine matrix until memoryd exposes the namespace."
                    .to_string(),
                title: Some("dreaming".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "dreaming",
                    "type": "claim",
                    "summary": "Dreaming unsupported by daemon",
                    "confidence": 0.96,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected unsupported dreaming namespace to be rejected, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("unsupported namespace `dreaming`"));
}

async fn user_project_write(
    substrate: &Substrate,
    request_id: &str,
    body: &str,
) -> memoryd::protocol::GovernanceWriteResponse {
    write_memory(
        substrate,
        WriteCase { request_id, body, actor: GovernanceActor::User, grounding_file: None, namespace: "project" },
    )
    .await
}

fn daemon_namespace(scope: GovernanceScope) -> Option<&'static str> {
    match scope {
        GovernanceScope::Me => Some("me"),
        GovernanceScope::Project => Some("project"),
        GovernanceScope::Agent => Some("agent"),
        GovernanceScope::Dreaming => None,
    }
}

struct WriteCase<'a> {
    request_id: &'a str,
    body: &'a str,
    actor: GovernanceActor,
    grounding_file: Option<&'a Path>,
    namespace: &'a str,
}

async fn write_memory(substrate: &Substrate, write_case: WriteCase<'_>) -> memoryd::protocol::GovernanceWriteResponse {
    let (source_kind, source_ref, explicit_user_context) = match write_case.actor {
        GovernanceActor::User => ("user", None, true),
        GovernanceActor::GroundedAgent => (
            "agent_primary",
            Some(format!(
                "file:{}",
                write_case.grounding_file.expect("grounded agent fixture has grounding file").display()
            )),
            false,
        ),
        GovernanceActor::UngroundedAgent => ("agent_primary", None, false),
        GovernanceActor::Subagent => {
            ("subagent", Some(format!("session-spawn:{}", governance_fixtures::SPAWNED_SUBAGENT_ID)), false)
        }
    };

    let response = handle_request(
        substrate,
        RequestEnvelope::new(
            write_case.request_id,
            RequestPayload::WriteMemory {
                body: write_case.body.to_string(),
                title: Some(write_case.request_id.to_string()),
                tags: vec!["governance-matrix".to_string()],
                meta: serde_json::json!({
                    "namespace": write_case.namespace,
                    "type": "claim",
                    "summary": write_case.request_id,
                    "confidence": 0.96,
                    "sensitivity": "internal",
                    "source_kind": source_kind,
                    "source_ref": source_ref,
                    "explicit_user_context": explicit_user_context
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance write response, got {:?}", response.result);
    };
    write
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_governancematrix".to_string()) },
    )
    .await
    .expect("init substrate")
}
