//! Regression: contradiction KNN must include active memories that are excluded
//! from passive recall.
//!
//! `passive_recall = false` suppresses retrieval, but it is not a governance
//! exemption. A new write that contradicts such an active memory must still see
//! the existing memory through substrate KNN and route to quarantine.

use std::sync::Arc;

use chrono::Utc;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::embedding::{worker, EmbeddingProvider, FixtureProvider};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

const TEST_PROJECT_CANONICAL_ID: &str = "proj_passive_recall_knn";
const TEST_PROJECT_ALIAS: &str = "passive-recall-knn";

#[tokio::test]
async fn governed_write_quarantines_against_passive_recall_disabled_active_memory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_passiveknn".to_string()) },
    )
    .await
    .expect("init substrate");
    let triple = substrate.active_embedding_triple().expect("active triple");

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple.clone()));
    let state = HandlerState::new();
    state.embedding_provider_slot().set(Arc::clone(&provider));

    let existing = project_memory(
        "mem_20260610_a1b2c3d4e5f60718_000201",
        "billing service production database engine",
        "The production database engine for the billing service in this project is PostgreSQL version 14.",
        false,
    );
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: existing,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("seed passive-recall-disabled active memory");
    drain_all(&substrate, &provider).await;
    assert!(substrate.vector_count(triple).await.expect("vector count") >= 1, "seed vector should be present");

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new(
            "passive-recall-disabled-contradiction",
            RequestPayload::WriteMemory {
                body: "The production database engine for the billing service in this project is MySQL version 8."
                    .to_string(),
                title: Some("billing service production database engine".to_string()),
                tags: vec!["passive-recall-disabled-contradiction".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "billing service production database engine",
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

    let write = match response.result {
        ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write,
        other => panic!("expected governed write success, got {other:?}"),
    };
    assert_eq!(
        write.status,
        GovernanceStatus::Quarantined,
        "contradiction against passive_recall=false active memory must quarantine, got {:?}",
        write.next_actions,
    );
    assert!(
        write.next_actions.iter().any(|action| action.contains("contradiction")),
        "quarantine reason should name contradiction, got {:?}",
        write.next_actions,
    );
    assert_eq!(write.similarity_degraded, None, "KNN backend was live, so no degradation marker expected");
}

async fn drain_all(substrate: &Substrate, provider: &Arc<dyn EmbeddingProvider>) {
    loop {
        let drained = worker::drain_batch(substrate, provider, 64).await.expect("drain embeddings");
        if drained < 64 {
            break;
        }
    }
}

fn project_memory(id: &str, summary: &str, body: &str, passive_recall: bool) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Claim,
            scope: Scope::Project,
            summary: summary.to_string(),
            confidence: 0.95,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::User,
                user_handle: Some("tester".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("test".to_string()),
            },
            namespace: Some(TEST_PROJECT_ALIAS.to_string()),
            canonical_namespace_id: Some(TEST_PROJECT_CANONICAL_ID.to_string()),
            tags: vec!["passive-recall-disabled-contradiction".to_string()],
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::User,
                reference: Some("test fixture".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall,
                max_scope: Scope::Project,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "test-fixture".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: Default::default(),
        },
        body: body.to_string(),
        path: Some(RepoPath::new(format!("projects/{TEST_PROJECT_ALIAS}/patterns/{id}.md"))),
    }
}
