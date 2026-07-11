use memory_substrate::*;

#[test]
fn public_api_contracts_compile() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Substrate>();

    let roots = Roots::new("repo", "runtime");
    let init = InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) };
    let adopt = AdoptOptions { force_new_device: true, merge_driver_path: Some("memory-merge-driver".into()) };
    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "test".into(), dimension: 32 };
    let _ = (roots, init, adopt, triple);
}

#[test]
fn write_requests_require_explicit_classification() {
    let memory = sample_memory();
    let request = WriteRequest {
        operation_id: None,
        memory: memory.clone(),
        expected_base_hash: None,
        write_mode: WriteMode::CreateNew,
        index_projection: None,
        event_context: EventContext::default(),
        allow_best_effort_durability: false,
        classification: ClassificationOutcome::Trusted,
    };
    let encrypted = EncryptedWriteRequest {
        operation_id: None,
        metadata_memory: memory,
        ciphertext: vec![1, 2, 3],
        safe_index_projection: None,
        event_context: EventContext::default(),
        allow_best_effort_durability: false,
        classification: ClassificationOutcome::RequiresEncryption,
    };
    assert_eq!(request.classification, ClassificationOutcome::Trusted);
    assert_eq!(encrypted.classification, ClassificationOutcome::RequiresEncryption);
}

fn sample_memory() -> Memory {
    let now = chrono::Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000001"),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "sample".to_string(),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
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
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        body: "body".to_string(),
        path: Some(RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md")),
    }
}
