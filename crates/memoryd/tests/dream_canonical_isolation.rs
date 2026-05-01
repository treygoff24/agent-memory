use memory_substrate::tree::{bootstrap_repo_tree, validate_tree, TreeValidationMode};
use memory_substrate::{
    ChunkQuery, ClassificationOutcome, Entity, InitOptions, Memory, MemoryId, MemoryQuery, MemoryStatus, MemoryType,
    ReadError, RecallIndexQuery, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, ValidationError, WriteMode, WriteRequest,
};

#[test]
fn daemon_visible_tree_validation_accepts_stream_f_noncanonical_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("bootstrap tree");
    for (path, contents) in noncanonical_files() {
        write_file(temp.path(), path, contents);
    }

    validate_tree(temp.path(), TreeValidationMode::FullySynced).expect("daemon-visible validation passes");
}

#[test]
fn daemon_visible_tree_validation_rejects_malformed_dream_jsonl() {
    let temp = tempfile::tempdir().expect("tempdir");
    bootstrap_repo_tree(temp.path()).expect("bootstrap tree");
    write_file(temp.path(), "dreams/questions/project/proj_abc/2026-04-30.jsonl", r#"{"question":7}"#);

    let err = validate_tree(temp.path(), TreeValidationMode::FullySynced).expect_err("invalid question rejected");

    assert!(matches!(
        err,
        ValidationError::NonCanonicalStreamFFile { path, .. }
            if path == std::path::PathBuf::from("dreams/questions/project/proj_abc/2026-04-30.jsonl")
    ));
}

#[tokio::test]
async fn daemon_visible_substrate_api_refuses_noncanonical_path_reads_and_query_indexing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init substrate");
    let memory = canonical_memory("mem_20260430_a1b2c3d4e5f60718_000001");
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: Default::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write canonical memory");

    for (path, contents) in noncanonical_files() {
        write_file(&roots.repo, path, contents);
        let repo_path = RepoPath::new(path);
        let err = substrate.read_path_envelope(&repo_path).await.expect_err("path read refused");
        assert!(matches!(err, ReadError::NotACanonicalMemory { path: refused } if refused == repo_path));
    }

    substrate.reindex().await.expect("noncanonical files ignored during daemon reindex");
    let memory_hits = substrate.query_memory(MemoryQuery::default()).await.expect("memory query");
    let recall_hits = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: None,
            statuses: vec![MemoryStatus::Active],
            passive_recall_only: true,
            updated_since: None,
            match_terms: vec!["stream-f-isolation".to_string()],
        })
        .await
        .expect("recall query");
    let chunk_hits = substrate
        .query_chunks(ChunkQuery { text: Some("stream-f-isolation".to_string()), triple: None, vector: None })
        .await
        .expect("chunk query");

    assert_eq!(memory_hits.len(), 1);
    assert_eq!(memory_hits[0].path, memory.path.clone().expect("canonical path"));
    assert_eq!(recall_hits.len(), 1);
    assert_eq!(recall_hits[0].path, memory.path.clone().expect("canonical path"));
    assert_eq!(chunk_hits.len(), 1);
    assert_eq!(chunk_hits[0].memory_id, memory.frontmatter.id);
}

fn noncanonical_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("dreams/journal/me/2026-04-30.md", "Masked journal body without frontmatter.\n"),
        (
            "dreams/questions/project/proj_abc/2026-04-30.jsonl",
            r#"{"entities":["ent_stream_f"],"question":"What assumption should we revisit?"}"#,
        ),
        ("dreams/cleanup/dev_local/2026-04-30.json", r#"{"device_id":"dev_local","operations":[]}"#),
        ("substrate/dev_local/2026-04-30.jsonl", r#"{"id":"sub_01","text":"plain substrate observation"}"#),
        (
            "encrypted/substrate/dev_local/2026-04-30.jsonl",
            r#"{"id":"sub_02","ciphertext":"age1...","descriptor":"encrypted fragment"}"#,
        ),
        ("leases/journal.lease", r#"{"scope":"me","device_id":"dev_local"}"#),
    ]
}

fn write_file(root: &std::path::Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    std::fs::write(path, contents).expect("write fixture");
}

fn canonical_memory(id: &str) -> Memory {
    Memory {
        frontmatter: memory_frontmatter(id),
        body: "stream-f-isolation canonical body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn memory_frontmatter(id: &str) -> memory_substrate::Frontmatter {
    memory_substrate::Frontmatter {
        schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: MemoryId::new(id),
        memory_type: MemoryType::Pattern,
        scope: Scope::Agent,
        namespace: None,
        canonical_namespace_id: None,
        summary: "stream-f-isolation canonical summary".to_string(),
        confidence: 1.0,
        trust_level: TrustLevel::Trusted,
        sensitivity: Sensitivity::Internal,
        status: MemoryStatus::Active,
        created_at: "2026-04-30T12:00:00Z".parse().expect("created_at"),
        updated_at: "2026-04-30T12:00:00Z".parse().expect("updated_at"),
        author: memory_substrate::Author {
            kind: memory_substrate::AuthorKind::System,
            user_handle: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            phase: None,
            component: Some("test".to_string()),
        },
        source: Source {
            kind: SourceKind::System,
            reference: None,
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        },
        tags: vec!["stream-f-isolation".to_string()],
        entities: vec![Entity {
            id: "ent_stream_f".to_string(),
            label: "Stream F".to_string(),
            aliases: vec!["stream-f-isolation".to_string()],
        }],
        aliases: Vec::new(),
        supersedes: Vec::new(),
        superseded_by: Vec::new(),
        related: Vec::new(),
        evidence: Vec::new(),
        retrieval_policy: RetrievalPolicy {
            passive_recall: true,
            max_scope: Scope::Agent,
            mask_personal_for_synthesis: false,
            index_body: true,
            index_embeddings: false,
        },
        write_policy: memory_substrate::WritePolicy {
            human_review_required: false,
            policy_applied: "test".to_string(),
            expected_base_hash: None,
        },
        review_state: None,
        requires_user_confirmation: false,
        tombstone_events: Vec::new(),
        merge_diagnostics: None,
        extras: Default::default(),
    }
}
