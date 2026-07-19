use std::path::Path;
use std::process::Command;

use memory_substrate::{InitOptions, MemoryId, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
};
use sha2::{Digest, Sha256};

const TEST_PROJECT_CANONICAL_ID: &str = "proj_governance_e2e";
const TEST_PROJECT_ALIAS: &str = "governance-e2e";

#[tokio::test]
async fn governance_e2e_grounded_project_write_becomes_active_or_candidate_per_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-project",
            RequestPayload::WriteMemory {
                body: "The agent-memory Stream C implementation plan includes Task 9 governance wiring.".to_string(),
                title: Some("Task 9 governance wiring".to_string()),
                tags: vec!["stream-c".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": "Task 9 governance wiring is in scope",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governed write success, got {:?}", response.result);
    };
    assert!(matches!(write.status, GovernanceStatus::Promoted | GovernanceStatus::Candidate));
    let id = write.id.expect("governed write returns memory id");
    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("governed write persisted");
    assert!(matches!(
        saved.frontmatter.status,
        memory_substrate::MemoryStatus::Active | memory_substrate::MemoryStatus::Candidate
    ));
    assert_eq!(saved.body, "The agent-memory Stream C implementation plan includes Task 9 governance wiring.");
}

#[tokio::test]
async fn governance_e2e_ungrounded_agent_write_is_refused_for_grounding() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-agent-ungrounded",
            RequestPayload::WriteMemory {
                body: "An agent claim without local evidence must fail closed.".to_string(),
                title: None,
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "agent",
                    "type": "claim",
                    "summary": "Ungrounded claim",
                    "confidence": 0.50,
                    "sensitivity": "internal",
                    "source_kind": "agent_primary"
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected structured governance refusal, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Refused);
    assert_eq!(write.reason, Some(GovernanceRefusalReason::Grounding));
    assert!(write.id.is_none(), "refused writes do not create memories");
}

#[tokio::test]
async fn governance_e2e_missing_privacy_classification_is_classified_by_stream_d() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-missing-privacy",
            RequestPayload::WriteMemory {
                body: "Structured durable writes need an explicit Stream D privacy classification boundary."
                    .to_string(),
                title: None,
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "Missing privacy classification",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected structured governance response, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Promoted);
    let id = write.id.expect("classified write persists");
    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("classified memory readable");
    assert_eq!(saved.frontmatter.sensitivity, memory_substrate::Sensitivity::Internal);
    assert!(saved.frontmatter.extras.contains_key("privacy_scan"));
}

#[tokio::test]
async fn governance_e2e_project_write_resolves_namespace_from_meta_cwd_git_remote() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let project = temp.path().join("project-cwd");
    let normalized_remote = "github.com/example/memorum-cwd-fixture";
    init_git_project_with_origin(&project, "https://github.com/example/memorum-cwd-fixture.git");
    let canonical_id = format!("proj_{}", hex::encode(Sha256::digest(normalized_remote.as_bytes())));

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-project-cwd",
            RequestPayload::WriteMemory {
                body: "Cwd-bound project writes should persist into the resolved project namespace.".to_string(),
                title: Some("cwd-bound project write".to_string()),
                tags: vec!["project".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "cwd": project,
                    "type": "project",
                    "summary": "cwd-bound project write",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governed write success, got {:?}", response.result);
    };
    assert!(matches!(write.status, GovernanceStatus::Promoted | GovernanceStatus::Candidate));
    let id = write.id.expect("governed write returns memory id");
    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("governed write persisted");
    assert_eq!(saved.frontmatter.canonical_namespace_id.as_deref(), Some(canonical_id.as_str()));
    assert_eq!(
        saved.frontmatter.namespace.as_deref(),
        Some(canonical_id.as_str()),
        "git-remote bindings have no alias, so placement falls back to canonical id"
    );
    let expected_path = format!("projects/{canonical_id}/decisions/{id}.md");
    assert_eq!(saved.path.as_ref().map(|path| path.as_str()), Some(expected_path.as_str()));
}

#[tokio::test]
async fn governance_e2e_project_write_without_identity_is_invalid_request() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-project-no-identity",
            RequestPayload::WriteMemory {
                body: "Project writes without identity should fail closed.".to_string(),
                title: None,
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": "missing project identity",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid_request, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(
        error.message.contains("project-namespace write requires project identity"),
        "message is actionable: {}",
        error.message
    );
}

#[tokio::test]
async fn governance_e2e_force_quarantine_metadata_is_rejected_not_laundered() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-force-quarantine",
            RequestPayload::WriteMemory {
                body: "A refused write must not be laundered into review.".to_string(),
                title: None,
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "agent",
                    "type": "claim",
                    "summary": "Forbidden force quarantine",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "agent_primary",
                    "force_quarantine": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid request for caller-controlled force_quarantine, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("force_quarantine"));
}

#[tokio::test]
async fn governance_e2e_invalid_disk_policy_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let policy_dir = temp.path().join("repo").join("policies");
    std::fs::create_dir_all(&policy_dir).expect("policy dir");
    std::fs::write(policy_dir.join("bad.yaml"), "name: agent-strict\nunexpected: true\n").expect("bad policy");

    let write = governed_project_write(&substrate, "bad-policy", "Invalid policy files fail closed.").await;

    assert_eq!(write.status, GovernanceStatus::Refused);
    assert_eq!(write.reason, Some(GovernanceRefusalReason::Policy));
    assert!(write.id.is_none());
}

#[tokio::test]
async fn governance_e2e_duplicate_write_returns_existing_id_without_second_active_memory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let body = "Duplicate governance writes should point at the first active memory.";

    let first = governed_project_write(&substrate, "first", body).await;
    let second = governed_project_write(&substrate, "second", body).await;

    assert_eq!(second.status, GovernanceStatus::Promoted);
    assert_eq!(second.existing_id, first.id);
    assert_eq!(second.id, first.id);
}

#[tokio::test]
async fn governance_e2e_supersede_updates_old_and_new_frontmatter() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let old = governed_project_write(&substrate, "old", "The old deployment target is staging.").await;
    let old_id = old.id.expect("old id");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "supersede",
            RequestPayload::Supersede {
                old_id: old_id.clone(),
                content: "The deployment target is production.".to_string(),
                reason: "deployment target changed".to_string(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": "Deployment target is production",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede success, got {:?}", response.result);
    };
    let new_id = supersede.new_id.expect("new id");
    let old_memory = substrate.read_memory(&MemoryId::new(&old_id)).await.expect("old memory readable");
    let new_memory = substrate.read_memory(&MemoryId::new(&new_id)).await.expect("new memory readable");
    assert_eq!(old_memory.frontmatter.status, memory_substrate::MemoryStatus::Superseded);
    assert!(old_memory.frontmatter.superseded_by.iter().any(|id| id.as_str() == new_id));
    assert!(new_memory.frontmatter.supersedes.iter().any(|id| id.as_str() == old_id));
}

#[tokio::test]
async fn governance_e2e_supersede_without_project_identity_inherits_old_namespace_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let old = governed_project_write(&substrate, "inherit-old", "The inherited deployment target is staging.").await;
    let old_id = old.id.expect("old id");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "supersede-inherit",
            RequestPayload::Supersede {
                old_id: old_id.clone(),
                content: "The inherited deployment target is production.".to_string(),
                reason: "deployment target changed".to_string(),
                meta: serde_json::json!({
                    "type": "project",
                    "summary": "Inherited deployment target is production",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede success, got {:?}", response.result);
    };
    let new_id = supersede.new_id.expect("new id");
    let old_memory = substrate.read_memory(&MemoryId::new(&old_id)).await.expect("old memory readable");
    let new_memory = substrate.read_memory(&MemoryId::new(&new_id)).await.expect("new memory readable");
    assert_eq!(new_memory.frontmatter.scope, old_memory.frontmatter.scope);
    assert_eq!(new_memory.frontmatter.namespace, old_memory.frontmatter.namespace);
    assert_eq!(new_memory.frontmatter.canonical_namespace_id, old_memory.frontmatter.canonical_namespace_id);
}

/// The MCP bridge injects `cwd` into every supersede. Placement must still
/// inherit from the old memory — a supersede issued from an unrelated
/// directory must not relocate the memory to the caller's project (or refuse
/// because the caller's cwd resolves to no project).
#[tokio::test]
async fn governance_e2e_supersede_with_foreign_cwd_still_inherits_old_namespace_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let old = governed_project_write(&substrate, "inherit-cwd", "The cwd-shadowed target is staging.").await;
    let old_id = old.id.expect("old id");
    let foreign_cwd = tempfile::tempdir().expect("foreign cwd");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "supersede-inherit-cwd",
            RequestPayload::Supersede {
                old_id: old_id.clone(),
                content: "The cwd-shadowed target is production.".to_string(),
                reason: "deployment target changed".to_string(),
                meta: serde_json::json!({
                    "type": "project",
                    "summary": "Cwd-shadowed target is production",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true,
                    "cwd": foreign_cwd.path().to_string_lossy(),
                }),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) = response.result else {
        panic!("expected supersede success, got {:?}", response.result);
    };
    let new_id = supersede.new_id.expect("new id");
    let old_memory = substrate.read_memory(&MemoryId::new(&old_id)).await.expect("old memory readable");
    let new_memory = substrate.read_memory(&MemoryId::new(&new_id)).await.expect("new memory readable");
    assert_eq!(new_memory.frontmatter.namespace, old_memory.frontmatter.namespace);
    assert_eq!(new_memory.frontmatter.canonical_namespace_id, old_memory.frontmatter.canonical_namespace_id);
}

#[tokio::test]
async fn governance_e2e_forget_tombstones_memory_and_removes_term_from_fts_hits() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let write =
        governed_project_write(&substrate, "forgettable", "ForgettableUniqueTerm should disappear from search.").await;
    let id = write.id.expect("write id");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "forget",
            RequestPayload::Forget { id: id.clone(), reason: "user requested removal".to_string() },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::GovernanceForget(forget)) = response.result else {
        panic!("expected forget success, got {:?}", response.result);
    };
    assert_eq!(forget.status, GovernanceStatus::Tombstoned);
    let tombstoned = substrate.read_memory(&MemoryId::new(&id)).await.expect("tombstoned memory readable");
    assert_eq!(tombstoned.frontmatter.status, memory_substrate::MemoryStatus::Tombstoned);

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "search-after-forget",
            RequestPayload::Search {
                query: "ForgettableUniqueTerm".to_string(),
                limit: Some(10),
                include_body: false,
                cwd: None,
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search success, got {:?}", search.result);
    };
    assert!(search.hits.iter().all(|hit| hit.id != id), "tombstoned memory should not be returned by FTS");

    let resurrect =
        governed_project_write(&substrate, "resurrect", "ForgettableUniqueTerm should disappear from search.").await;
    assert_eq!(resurrect.status, GovernanceStatus::Candidate);
    assert_eq!(resurrect.next_actions, ["tombstone"]);
    let resurrected_id = resurrect.id.expect("tombstone review candidate id");
    let resurrected =
        substrate.read_memory(&MemoryId::new(&resurrected_id)).await.expect("tombstone review candidate readable");
    assert_eq!(resurrected.frontmatter.status, memory_substrate::MemoryStatus::Candidate);
}

#[tokio::test]
async fn governance_e2e_quarantined_writes_show_in_review_queue() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "write-quarantine",
            RequestPayload::WriteMemory {
                body: "This claim is intentionally ambiguous and needs quarantine review.".to_string(),
                title: Some("Needs review".to_string()),
                tags: vec!["review".to_string()],
                meta: serde_json::json!({
                    "namespace": "agent",
                    "type": "claim",
                    "summary": "Needs quarantine review",
                    "confidence": 0.50,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected quarantined write response, got {:?}", response.result);
    };
    assert_eq!(write.status, GovernanceStatus::Candidate);
    let id = write.id.expect("quarantined id");

    let queue = handle_request(
        &substrate,
        RequestEnvelope::new("review-queue", RequestPayload::ReviewQueue { limit: Some(10) }),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::ReviewQueue(queue)) = queue.result else {
        panic!("expected review queue success, got {:?}", queue.result);
    };
    assert!(queue.items.iter().any(|item| item.id == id), "quarantined memory appears in review queue");
}

async fn governed_project_write(
    substrate: &Substrate,
    request_id: &str,
    body: &str,
) -> memoryd::protocol::GovernanceWriteResponse {
    let response = handle_request(
        substrate,
        RequestEnvelope::new(
            request_id,
            RequestPayload::WriteMemory {
                body: body.to_string(),
                title: Some(request_id.to_string()),
                tags: vec!["project".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": request_id,
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governed write success, got {:?}", response.result);
    };
    write
}

fn init_git_project_with_origin(path: &Path, origin: &str) {
    std::fs::create_dir_all(path).expect("project dir");
    git(path, ["init"]);
    git(path, ["remote", "add", "origin", origin]);
}

fn git<const N: usize>(cwd: &Path, args: [&str; N]) {
    let status = Command::new("git").args(args).current_dir(cwd).status().expect("git command starts");
    assert!(status.success(), "git command failed in {}: {:?}", cwd.display(), status);
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_governancee2e".to_string()) },
    )
    .await
    .expect("init substrate")
}
