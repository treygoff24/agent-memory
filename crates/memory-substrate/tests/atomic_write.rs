use chrono::Utc;
use memory_substrate::markdown::{atomic_write, AtomicWrite};
use memory_substrate::{
    Author, AuthorKind, DurabilityTier, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath,
    RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, TrustLevel, WriteFailureKind, WriteMode, WritePolicy,
};

#[test]
fn atomic_write_stages_temp_in_target_parent_without_cross_device_rename() {
    let temp = tempfile::tempdir().expect("tempdir");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000777");
    let path = memory.path.clone().expect("path");

    let hash = atomic_write(AtomicWrite {
        repo: temp.path(),
        memory: &memory,
        expected_base_hash: None,
        mode: WriteMode::CreateNew,
        operation_id: &OperationId::new("op_atomic_parent"),
        durability: DurabilityTier::BestEffort,
        suppression: None,
        allow_encrypted_namespace: false,
    })
    .expect("atomic write");

    let target = temp.path().join(path.as_path());
    let parent = target.parent().expect("target parent");
    assert!(target.exists());
    assert_eq!(hash, memory_substrate::markdown::hash_bytes(&std::fs::read(&target).expect("target bytes")));
    assert!(std::fs::read_dir(parent)
        .expect("read parent")
        .filter_map(Result::ok)
        .all(|entry| !entry.file_name().to_string_lossy().contains("op_atomic_parent")));
    assert!(!temp.path().join(".mem_20260424_a1b2c3d4e5f60718_000777.md.op_atomic_parent.tmp").exists());
}

#[test]
fn atomic_write_refuses_plaintext_encrypted_namespace_before_disk_effects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000778");
    memory.path = Some(RepoPath::new("encrypted/agent/patterns/plaintext.md"));

    let failure = atomic_write(AtomicWrite {
        repo: temp.path(),
        memory: &memory,
        expected_base_hash: None,
        mode: WriteMode::CreateNew,
        operation_id: &OperationId::new("op_atomic_encrypted_namespace"),
        durability: DurabilityTier::BestEffort,
        suppression: None,
        allow_encrypted_namespace: false,
    })
    .expect_err("plaintext atomic write to encrypted namespace is refused");

    assert!(matches!(failure.kind, WriteFailureKind::Validation(message) if message.contains("encrypted namespace")));
    assert!(!temp.path().join("encrypted").exists());
}

#[test]
fn atomic_write_refuses_unsafe_repo_path_before_disk_effects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000779");
    // Use from_unchecked so we can construct the unsafe path that the write
    // layer must reject. RepoPath::new panics on invalid paths by design
    // (test fixtures only); refusal-test fixtures use from_unchecked.
    memory.path = Some(RepoPath::from_unchecked("agent/patterns/../../.git/config"));

    let failure = atomic_write(AtomicWrite {
        repo: temp.path(),
        memory: &memory,
        expected_base_hash: None,
        mode: WriteMode::CreateNew,
        operation_id: &OperationId::new("op_atomic_unsafe_path"),
        durability: DurabilityTier::BestEffort,
        suppression: None,
        allow_encrypted_namespace: false,
    })
    .expect_err("unsafe atomic write path is refused");

    assert!(matches!(failure.kind, WriteFailureKind::Validation(message) if message.contains("invalid repo path")));
    assert!(!temp.path().join(".git").exists());
}

fn sample_memory(id: &str) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
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
            extras: std::collections::BTreeMap::new(),
        },
        body: "body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
