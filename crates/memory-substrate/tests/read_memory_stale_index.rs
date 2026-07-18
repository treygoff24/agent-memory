//! Regression tests for the index-first `read_memory` fast path: a stale index
//! must never change the answer relative to the legacy disk-walk. The id is
//! resolved via a PK lookup on `memories.id`, but the resolved file is verified
//! (and the disk-walk is the fallback) so a moved or repointed file does not
//! make `read_memory` return the wrong memory or the wrong error.

use chrono::Utc;
use memory_substrate::InitOptions;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType,
    ReadError, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};

#[tokio::test]
async fn read_memory_falls_back_to_disk_walk_when_index_path_is_stale() {
    let (_temp, substrate, roots) = seeded("mem_20260424_a1b2c3d4e5f60718_030001").await;
    let id = MemoryId::new("mem_20260424_a1b2c3d4e5f60718_030001");

    // Relocate the canonical file on disk WITHOUT reindexing: the index still
    // resolves the id to the old path, which no longer exists.
    let old_path = roots.repo.join("agent/patterns/mem_20260424_a1b2c3d4e5f60718_030001.md");
    let new_path = roots.repo.join("agent/patterns/relocated.md");
    std::fs::rename(&old_path, &new_path).expect("relocate canonical file");

    // A stale indexed path must fall through to the disk walk.
    let memory = substrate.read_memory(&id).await.expect("stale index still resolves via disk-walk");
    assert_eq!(memory.frontmatter.id, id);
}

#[tokio::test]
async fn read_memory_rejects_index_hit_resolving_to_a_different_id() {
    let (_temp, substrate, roots) = seeded("mem_20260424_a1b2c3d4e5f60718_030002").await;
    let requested = MemoryId::new("mem_20260424_a1b2c3d4e5f60718_030002");

    // Overwrite the resolved file with a DIFFERENT id's content, no reindex: the
    // index still maps `requested` to this path, but the file now holds another
    // id. The fast path must verify frontmatter.id and refuse to return it.
    let resolved_path = roots.repo.join("agent/patterns/mem_20260424_a1b2c3d4e5f60718_030002.md");
    let mut other = sample_memory("mem_20260424_a1b2c3d4e5f60718_030099");
    other.path = Some(RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_030002.md"));
    let markdown = memory_substrate::frontmatter::serialize_document(&other).expect("serialize other id");
    std::fs::write(&resolved_path, markdown).expect("repoint file to a different id");

    // An id mismatch must fall through rather than return the wrong memory.
    let result = substrate.read_memory(&requested).await;
    assert!(
        matches!(result, Err(ReadError::NotFound(_))),
        "expected NotFound for a stale index hit holding a different id, got {result:?}"
    );
}

async fn seeded(id: &str) -> (tempfile::TempDir, Substrate, Roots) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: sample_memory(id),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write seed memory");
    (temp, substrate, roots)
}

fn sample_memory(id: &str) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "stale-index regression".to_string(),
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
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
