/// Shared fixture helpers for `memoryd export` integration tests.
///
/// Each export test binary declares `#[path = "export_fixture/mod.rs"] mod export_fixture;`
/// to pull in these helpers without duplication.
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind,
    Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};

pub async fn init_substrate(temp: &tempfile::TempDir, device_id: &str) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions {
        force_unsafe_durability: true,
        device_id: Some(device_id.to_string()),
    })
    .await
    .expect("init substrate")
}

/// Build a plaintext memory with the given id, body, and RFC3339 timestamp string.
pub fn make_plaintext_memory(id: &str, body: &str, ts_str: &str) -> Memory {
    let ts = chrono::DateTime::parse_from_rfc3339(ts_str)
        .expect("fixed ts")
        .with_timezone(&chrono::Utc);
    let mid = MemoryId::new(id);
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: mid.clone(),
            memory_type: MemoryType::Claim,
            scope: Scope::Agent,
            summary: format!("export fixture {mid}"),
            confidence: 0.9,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: ts,
            updated_at: ts,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("export-test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: vec!["export-test".to_string()],
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::System,
                reference: None,
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
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "trusted-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: body.to_string(),
        path: Some(RepoPath::new(format!("agent/claims/{}.md", mid.as_str()))),
    }
}

pub async fn write_plaintext(substrate: &Substrate, memory: Memory) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write plaintext memory");
}
