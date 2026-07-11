use memory_substrate::*;

#[tokio::test]
async fn write_read_query_and_event_round_trip_through_public_api() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000011");
    let outcome = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write");
    assert!(outcome.committed);
    let read = substrate.read_memory(&memory.frontmatter.id).await.expect("read");
    assert_eq!(read.body, memory.body);
    let hits = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id.clone()),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query");
    assert_eq!(hits.len(), 1);
    assert!(substrate
        .events()
        .expect("events")
        .iter()
        .any(|event| matches!(event.kind, memory_substrate::events::EventKind::WriteCommitted { .. })));
}

#[tokio::test]
async fn query_memory_filters_by_tag_and_metadata_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");

    let mut visible = sample_memory("mem_20260424_a1b2c3d4e5f60718_000014");
    visible.frontmatter.tags = vec!["shared".to_string(), "alpha".to_string()];
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: visible.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write visible");

    let mut hidden = sample_memory("mem_20260424_a1b2c3d4e5f60718_000015");
    hidden.frontmatter.tags = vec!["shared".to_string(), "secret".to_string()];
    hidden.frontmatter.sensitivity = Sensitivity::Confidential;
    hidden.frontmatter.retrieval_policy.index_body = false;
    hidden.frontmatter.retrieval_policy.index_embeddings = false;
    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: hidden.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("write hidden");

    let visible_hits = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: Some("shared".to_string()),
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query visible");
    assert_eq!(visible_hits.len(), 1);
    assert_eq!(visible_hits[0].id, visible.frontmatter.id);

    let all_hits = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: Some("shared".to_string()),
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("query all");
    assert_eq!(all_hits.len(), 2);
    assert!(all_hits.iter().any(|hit| hit.id == visible.frontmatter.id));
    assert!(all_hits.iter().any(|hit| hit.id == hidden.frontmatter.id));

    let alpha_hits = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: Some("alpha".to_string()),
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query alpha");
    assert_eq!(alpha_hits.len(), 1);
    assert_eq!(alpha_hits[0].id, visible.frontmatter.id);
}

#[tokio::test]
async fn classification_secret_refuses_before_any_disk_effect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000012");
    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Secret,
        })
        .await
        .expect_err("secret refused");
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::SecretRefused);
    assert!(!roots.repo.join(memory.path.expect("path").as_path()).exists());
}

#[tokio::test]
async fn write_refuses_repo_path_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000013");
    memory.path = Some(RepoPath::from_unchecked("../escape.md"));
    let err = substrate
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
        .expect_err("path escape refused");
    assert!(matches!(err.kind, memory_substrate::WriteFailureKind::ValidationTyped(_)));
    assert!(!temp.path().join("escape.md").exists());
}

#[tokio::test]
async fn plaintext_write_refuses_encrypted_namespace_before_disk_index_or_event_effects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000040");
    memory.path = Some(RepoPath::new("encrypted/agent/patterns/plaintext-leak.md"));
    let before_events = substrate.events().expect("events before").len();

    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("encrypted namespace refused");

    assert!(
        matches!(err.kind, memory_substrate::WriteFailureKind::ValidationTyped(message) if message.to_string().contains("encrypted namespace"))
    );
    assert!(!roots.repo.join(memory.path.expect("path").as_path()).exists());
    assert!(substrate
        .query_memory(MemoryQuery {
            id: Some(MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000040")),
            tag: None,
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("query")
        .is_empty());
    // B-API-9 / spec §8.7 step 6: refusals append exactly one audit event
    // (`WriteRefused`) so Stream D can confirm Stream A made a positive call.
    let after = substrate.events().expect("events after");
    assert_eq!(after.len(), before_events + 1);
    let last = after.last().expect("audit event");
    assert!(matches!(&last.kind, memory_substrate::events::EventKind::WriteRefused { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn plaintext_write_refuses_symlinked_parent_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let outside = temp.path().join("outside-parent");
    std::fs::create_dir_all(&outside).expect("outside parent");
    std::fs::remove_dir(roots.repo.join("agent/patterns")).expect("remove empty patterns dir");
    std::os::unix::fs::symlink(&outside, roots.repo.join("agent/patterns")).expect("symlink parent");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000041");

    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("symlinked parent refused");

    assert!(
        matches!(err.kind, memory_substrate::WriteFailureKind::ValidationTyped(message) if message.to_string().contains("symlink"))
    );
    assert!(!outside.join(format!("{}.md", memory.frontmatter.id.as_str())).exists());
}

#[tokio::test]
async fn read_path_refuses_repo_path_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    std::fs::write(temp.path().join("escape.md"), "---\nnot: a memory\n---\nsecret").expect("outside file");

    let err = substrate.read_path(&RepoPath::from_unchecked("../escape.md")).await.expect_err("path escape refused");

    assert!(matches!(
        err,
        memory_substrate::ReadError::Parse { message, .. } if message.contains("invalid repo-relative memory path")
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn read_path_refuses_symlink_escape_even_under_allowed_prefix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let outside = temp.path().join("outside.md");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000039");
    memory.body = "outside secret body".to_string();
    std::fs::write(&outside, memory_substrate::frontmatter::serialize_document(&memory).expect("serialize outside"))
        .expect("outside memory");
    std::os::unix::fs::symlink(&outside, roots.repo.join("agent/patterns/link.md")).expect("symlink");

    let err = substrate.read_path(&RepoPath::new("agent/patterns/link.md")).await.expect_err("symlink escape refused");

    assert!(matches!(
        err,
        memory_substrate::ReadError::Parse { message, .. } if message.contains("resolves outside repository")
    ));
}

#[tokio::test]
async fn privacy_scan_private_credential_refuses_plaintext_before_disk_effect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000038");
    memory
        .frontmatter
        .extras
        .insert("privacy_scan".to_string(), serde_json::json!({ "labels": ["private_credential"] }));
    let path = memory.path.clone().expect("path");

    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("credential label refused");

    assert!(
        matches!(err.kind, memory_substrate::WriteFailureKind::ValidationTyped(message) if message.to_string().contains("privacy_scan.private_credential"))
    );
    assert!(!roots.repo.join(path.as_path()).exists());
    let hits = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id),
            tag: None,
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("query");
    assert!(hits.is_empty());
}

#[tokio::test]
async fn stale_base_replace_leaves_existing_file_unchanged() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000016");
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("create");
    let path = roots.repo.join(memory.path.clone().expect("path").as_path());
    let before = std::fs::read_to_string(&path).expect("before");
    memory.body = "updated body that must not commit".to_string();

    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: Some(Sha256::new("sha256:stale")),
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("stale base");

    assert_eq!(err.kind, memory_substrate::WriteFailureKind::StaleBase);
    assert_eq!(std::fs::read_to_string(path).expect("after"), before);
    let temp_files = std::fs::read_dir(roots.repo.join("agent/patterns"))
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
        .count();
    assert_eq!(temp_files, 0);
}

#[tokio::test]
async fn plaintext_requires_encryption_classification_is_refused_before_disk_effect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000017");
    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("encryption required");
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::EncryptionRequired);
    assert!(!roots.repo.join(memory.path.expect("path").as_path()).exists());
}

#[tokio::test]
async fn trusted_classification_cannot_persist_confidential_plaintext() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000018");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("sensitivity mismatch");
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::ClassificationSensitivityMismatch);
    assert!(!roots.repo.join(memory.path.expect("path").as_path()).exists());
}

#[tokio::test]
async fn best_effort_plaintext_write_requires_explicit_opt_in() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000019");
    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: false,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("durability opt-in required");
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::DurabilityUnavailable);
    assert!(!roots.repo.join(memory.path.expect("path").as_path()).exists());
}

#[tokio::test]
async fn best_effort_encrypted_write_requires_explicit_opt_in() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000020");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.path = None;
    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: false,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("durability opt-in required");
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::DurabilityUnavailable);
    assert!(!roots.repo.join(format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str())).exists());
}

#[tokio::test]
async fn query_chunks_returns_fts_hits() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000014");
    memory.body = "needle chunk body".to_string();
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
        .expect("write");
    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("needle".to_string()), triple: None, vector: None })
        .await
        .expect("chunk query");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.contains("needle"));
}

#[tokio::test]
async fn query_chunks_excludes_passive_recall_disabled_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");

    let mut passive = sample_memory("mem_20260424_a1b2c3d4e5f60718_000116");
    passive.body = "sharedneedle passive body".to_string();

    let mut disabled = sample_memory("mem_20260424_a1b2c3d4e5f60718_000117");
    disabled.body = "sharedneedle disabled body".to_string();
    disabled.frontmatter.retrieval_policy.passive_recall = false;

    for memory in [passive.clone(), disabled] {
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
            .expect("write");
    }

    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("sharedneedle".to_string()), triple: None, vector: None })
        .await
        .expect("chunk query");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].memory_id, passive.frontmatter.id);
    assert!(hits[0].text.contains("passive body"));
}

#[tokio::test]
async fn fts_mutation_removes_old_terms_after_replace() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000021");
    memory.body = "oldterm body".to_string();
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write");
    assert_eq!(
        substrate
            .query_chunks(ChunkQuery { text: Some("oldterm".to_string()), triple: None, vector: None })
            .await
            .expect("oldterm query")
            .len(),
        1
    );

    let path = roots.repo.join(memory.path.clone().expect("path").as_path());
    let base_hash = memory_substrate::markdown::hash_bytes(&std::fs::read(&path).expect("file bytes"));
    memory.body = "newterm body".to_string();
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: Some(base_hash),
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("replace");

    assert!(substrate
        .query_chunks(ChunkQuery { text: Some("oldterm".to_string()), triple: None, vector: None })
        .await
        .expect("oldterm gone")
        .is_empty());
    assert_eq!(
        substrate
            .query_chunks(ChunkQuery { text: Some("newterm".to_string()), triple: None, vector: None })
            .await
            .expect("newterm query")
            .len(),
        1
    );
}

#[tokio::test]
async fn tombstone_memory_persists_status_updates_index_and_records_event() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000037");
    memory.body = "tombstoneindexedterm body".to_string();
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write");
    assert_eq!(
        substrate
            .query_chunks(ChunkQuery { text: Some("tombstoneindexedterm".to_string()), triple: None, vector: None })
            .await
            .expect("pre-tombstone query")
            .len(),
        1
    );

    let outcome = substrate
        .tombstone_memory(TombstoneRequest { id: memory.frontmatter.id.clone(), reason: "forget request".to_string() })
        .await
        .expect("tombstone");

    assert!(outcome.committed);
    assert!(outcome.indexed);
    assert!(outcome.event_recorded);
    let tombstoned = substrate.read_path(&memory.path.expect("path")).await.expect("read tombstoned");
    assert_eq!(tombstoned.frontmatter.status, MemoryStatus::Tombstoned);
    assert_eq!(tombstoned.frontmatter.tombstone_events.len(), 1);
    assert!(substrate
        .query_chunks(ChunkQuery { text: Some("tombstoneindexedterm".to_string()), triple: None, vector: None })
        .await
        .expect("post-tombstone query")
        .is_empty());
    assert!(substrate
        .events()
        .expect("events")
        .iter()
        .any(|event| matches!(event.kind, memory_substrate::events::EventKind::TombstoneCommitted { .. })));
}

#[tokio::test]
async fn event_after_commit_failure_returns_committed_indexed_repair_outcome() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let blocked_event_log = roots.repo.join("events/dev_test.jsonl");
    if blocked_event_log.exists() {
        std::fs::remove_file(&blocked_event_log).expect("remove existing event log");
    }
    std::fs::create_dir_all(&blocked_event_log).expect("block event log path");
    std::fs::create_dir_all(roots.runtime.join("pending/events.jsonl")).expect("block pending event queue");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000022");

    let err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("event append fails after commit");

    assert!(err.outcome.committed);
    assert!(err.outcome.indexed);
    assert!(!err.outcome.event_recorded);
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::RepairQueueFailed);
    assert_eq!(err.outcome.repair_required, Some(RepairRequired::FullStartupScan));
    assert!(roots.repo.join(memory.path.expect("path").as_path()).exists());
    assert_eq!(
        substrate
            .query_memory(MemoryQuery {
                id: Some(MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000022")),
                tag: None,
                include_metadata_only: false,
                ..MemoryQuery::default()
            })
            .await
            .expect("indexed query")
            .len(),
        1
    );
    assert!(roots.runtime.join("startup-reconcile.required").exists());
}

#[tokio::test]
async fn write_outcomes_distinguish_not_committed_full_commit_and_event_repair_states() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let refused = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: sample_memory("mem_20260424_a1b2c3d4e5f60718_000023"),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Secret,
        })
        .await
        .expect_err("secret refused");
    assert!(!refused.outcome.committed);
    assert!(!refused.outcome.indexed);
    assert!(!refused.outcome.event_recorded);

    let full = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: sample_memory("mem_20260424_a1b2c3d4e5f60718_000024"),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("full write");
    assert!(full.committed);
    assert!(full.indexed);
    assert!(full.event_recorded);
    assert!(full.repair_required.is_none());

    let blocked_event_log = roots.repo.join("events/dev_test.jsonl");
    if blocked_event_log.exists() {
        std::fs::remove_file(&blocked_event_log).expect("remove existing event log");
    }
    std::fs::create_dir_all(&blocked_event_log).expect("block event log path");
    std::fs::create_dir_all(roots.runtime.join("pending/events.jsonl")).expect("block pending event queue");
    let repair = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: sample_memory("mem_20260424_a1b2c3d4e5f60718_000025"),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("event repair state");
    assert!(repair.outcome.committed);
    assert!(repair.outcome.indexed);
    assert!(!repair.outcome.event_recorded);
    assert_eq!(repair.kind, memory_substrate::WriteFailureKind::RepairQueueFailed);
    assert_eq!(repair.outcome.repair_required, Some(RepairRequired::FullStartupScan));
}

#[tokio::test]
async fn encrypted_write_uses_encrypted_path_and_metadata_only_index() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000015");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.path = None;
    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext-not-secret-plaintext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("encrypted write");
    let encrypted_path = roots.repo.join(format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str()));
    assert!(encrypted_path.exists());
    let envelope = substrate.read_memory_envelope(&memory.frontmatter.id).await.expect("encrypted envelope");
    match envelope.content {
        MemoryContent::Ciphertext { bytes, .. } => {
            assert_eq!(bytes, b"ciphertext-not-secret-plaintext");
        }
        other => panic!("expected ciphertext envelope, got {other:?}"),
    }
    memory_substrate::tree::validate_tree(&roots.repo, memory_substrate::tree::TreeValidationMode::FullySynced)
        .expect("encrypted tree validates");
    assert!(!roots.repo.join(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str())).exists());
    let hits = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id.clone()),
            tag: None,
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("query");
    assert_eq!(hits[0].path.as_str(), format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str()));
    let chunks = substrate
        .query_chunks(ChunkQuery { text: Some("ciphertext".to_string()), triple: None, vector: None })
        .await
        .expect("chunks");
    assert!(chunks.is_empty());
    drop(substrate);
    for suffix in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
        let index_path = roots.runtime.join(suffix);
        if index_path.exists() {
            std::fs::remove_file(index_path).expect("remove index file");
        }
    }
    let reopened = Substrate::open(roots.clone()).await.expect("reopen after index loss");
    let recovered = reopened
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id.clone()),
            tag: None,
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("recovered encrypted metadata");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].path.as_str(), format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str()));
}

#[tokio::test]
async fn update_encrypted_memory_metadata_preserves_ciphertext_envelope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000031");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.body = "metadata-only placeholder body".to_string();
    memory.path = None;

    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"stable ciphertext bytes".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("encrypted write");
    let before = substrate.read_memory_envelope(&memory.frontmatter.id).await.expect("read encrypted before");

    substrate
        .update_encrypted_memory_metadata(&memory.frontmatter.id, |metadata| {
            metadata.frontmatter.summary = "updated safe metadata".to_string();
            metadata.frontmatter.confidence = 0.64;
            metadata
                .frontmatter
                .extras
                .insert("encryption".to_string(), serde_json::json!({"scheme": "tampered", "recipient": "tampered"}));
            metadata.body = "attempted plaintext replacement".to_string();
        })
        .await
        .expect("metadata update");

    let after = substrate.read_memory_envelope(&memory.frontmatter.id).await.expect("read encrypted after");
    assert_eq!(after.metadata.frontmatter.summary, "updated safe metadata");
    assert_eq!(after.metadata.frontmatter.confidence, 0.64);
    assert_eq!(after.metadata.body, before.metadata.body);
    assert_eq!(after.content, before.content);
}

#[tokio::test]
async fn recall_index_including_metadata_only_does_not_expose_encrypted_plaintext_fragments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000032");
    memory.frontmatter.summary = "safe encrypted summary".to_string();
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.body = "plaintext-leak-needle must not be surfaced".to_string();
    memory.path = None;

    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"opaque ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("encrypted write");

    let rows =
        substrate.query_recall_index_including_metadata_only(RecallIndexQuery::default()).await.expect("recall index");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].summary, "safe encrypted summary");
    let serialized = serde_json::to_string(&rows).expect("serialize rows");
    assert!(!serialized.contains("plaintext-leak-needle"));
}

#[cfg(unix)]
#[tokio::test]
async fn encrypted_write_refuses_symlinked_encrypted_parent_escape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let outside = temp.path().join("outside-encrypted-parent");
    std::fs::create_dir_all(&outside).expect("outside encrypted parent");
    std::os::unix::fs::symlink(&outside, roots.repo.join("encrypted/agent")).expect("encrypted parent symlink");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000042");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;

    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("symlinked encrypted parent refused");

    assert!(
        matches!(err.kind, memory_substrate::WriteFailureKind::IoTyped { context, .. } if context.contains("symlink"))
    );
    assert!(!outside.join("patterns").join(format!("{}.md", memory.frontmatter.id.as_str())).exists());
}

#[tokio::test]
async fn encrypted_write_derives_ciphertext_path_under_encrypted_namespace_and_rejects_bad_originals() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000031");
    memory.path = Some(RepoPath::new("agent/patterns/original-name.md"));
    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory,
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("encrypted write");
    assert!(roots.repo.join("encrypted/agent/patterns/original-name.md").exists());
    assert!(!roots.repo.join("agent/patterns/original-name.md").exists());

    let mut bad = sample_memory("mem_20260424_a1b2c3d4e5f60718_000032");
    bad.path = Some(RepoPath::from_unchecked("events/not-a-memory.md"));
    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: bad,
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("non-memory original path refused");
    assert!(matches!(err.kind, memory_substrate::WriteFailureKind::ValidationTyped(_)));
}

#[tokio::test]
async fn encrypted_create_new_refuses_to_overwrite_existing_ciphertext() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut first = sample_memory("mem_20260424_a1b2c3d4e5f60718_000033");
    first.path = Some(RepoPath::new("agent/patterns/shared-encrypted.md"));
    let first_id = first.frontmatter.id.clone();
    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: first,
            ciphertext: b"first".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("first encrypted write");

    let mut second = sample_memory("mem_20260424_a1b2c3d4e5f60718_000034");
    second.path = Some(RepoPath::new("agent/patterns/shared-encrypted.md"));
    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: second,
            ciphertext: b"second".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("existing ciphertext refused");
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::AlreadyExists);
    let envelope = substrate.read_memory_envelope(&first_id).await.expect("envelope");
    match envelope.content {
        MemoryContent::Ciphertext { bytes, .. } => assert_eq!(bytes, b"first"),
        other => panic!("expected ciphertext envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn encrypted_write_requires_requires_encryption_classification_before_disk_effect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000026");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.path = None;

    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect_err("trusted encrypted write refused");

    assert_eq!(err.kind, memory_substrate::WriteFailureKind::EncryptionRequired);
    assert!(!err.outcome.committed);
    assert!(!roots.repo.join(format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str())).exists());
}

#[tokio::test]
async fn encrypted_safe_projection_indexes_only_safe_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000027");
    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.body = "unsafesecretneedle must never be indexed".to_string();
    memory.path = None;

    substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory,
            ciphertext: b"opaque ciphertext".to_vec(),
            safe_index_projection: Some(IndexProjection { safe_body: Some("safeprojectionneedle".to_string()) }),
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("encrypted write");

    assert_eq!(
        substrate
            .query_chunks(ChunkQuery { text: Some("safeprojectionneedle".to_string()), triple: None, vector: None })
            .await
            .expect("safe query")
            .len(),
        1
    );
    assert!(substrate
        .query_chunks(ChunkQuery { text: Some("unsafesecretneedle".to_string()), triple: None, vector: None })
        .await
        .expect("unsafe query")
        .is_empty());
}

#[tokio::test]
async fn event_after_commit_failure_with_pending_queue_returns_committed_pending_event_outcome() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let blocked_event_log = roots.repo.join("events/dev_test.jsonl");
    if blocked_event_log.exists() {
        std::fs::remove_file(&blocked_event_log).expect("remove existing event log");
    }
    std::fs::create_dir_all(&blocked_event_log).expect("block event log path");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000028");

    let outcome = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("event append failure should enqueue pending event and return committed outcome");

    assert!(outcome.committed);
    assert!(outcome.indexed);
    assert!(!outcome.event_recorded);
    assert_eq!(outcome.repair_required, Some(RepairRequired::PendingEvent));
    assert!(roots.runtime.join("pending/events.jsonl").exists());
    assert!(roots.repo.join(memory.path.expect("path").as_path()).exists());
}

#[tokio::test]
async fn encrypted_event_after_ciphertext_commit_with_pending_queue_returns_committed_pending_event_outcome() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let blocked_event_log = roots.repo.join("events/dev_test.jsonl");
    if blocked_event_log.exists() {
        std::fs::remove_file(&blocked_event_log).expect("remove existing event log");
    }
    std::fs::create_dir_all(&blocked_event_log).expect("block event log path");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000029");
    memory.path = None;

    let outcome = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: Some(IndexProjection { safe_body: Some("encrypted safe index body".to_string()) }),
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect("event append failure should enqueue pending event and return committed outcome");

    assert!(outcome.committed);
    assert!(outcome.indexed);
    assert!(!outcome.event_recorded);
    assert_eq!(outcome.repair_required, Some(RepairRequired::PendingEvent));
    assert!(roots.runtime.join("pending/events.jsonl").exists());
    assert!(roots.repo.join(format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str())).exists());
}

#[tokio::test]
async fn encrypted_event_after_ciphertext_commit_queue_failure_returns_repair_queue_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let blocked_event_log = roots.repo.join("events/dev_test.jsonl");
    if blocked_event_log.exists() {
        std::fs::remove_file(&blocked_event_log).expect("remove existing event log");
    }
    std::fs::create_dir_all(&blocked_event_log).expect("block event log path");
    std::fs::create_dir_all(roots.runtime.join("pending/events.jsonl")).expect("block pending event queue");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000030");
    memory.path = None;

    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: Some(IndexProjection { safe_body: Some("encrypted safe index body".to_string()) }),
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("event append fails after ciphertext commit");

    assert!(err.outcome.committed);
    assert!(err.outcome.indexed);
    assert!(!err.outcome.event_recorded);
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::RepairQueueFailed);
    assert_eq!(err.outcome.repair_required, Some(RepairRequired::FullStartupScan));
    assert!(roots.repo.join(format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str())).exists());
    assert_eq!(
        substrate
            .query_memory(MemoryQuery {
                id: Some(memory.frontmatter.id),
                tag: None,
                include_metadata_only: true,
                ..MemoryQuery::default()
            })
            .await
            .expect("indexed metadata")
            .len(),
        1
    );
    assert!(roots.runtime.join("startup-reconcile.required").exists());
}

#[tokio::test]
async fn encrypted_index_after_ciphertext_commit_is_durably_replayed_on_startup() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let sqlite_path = roots.runtime.join("index.sqlite");
    {
        let conn = rusqlite::Connection::open(&sqlite_path).expect("open index");
        conn.execute(
            "CREATE TRIGGER fail_encrypted_index BEFORE INSERT ON memories BEGIN SELECT RAISE(FAIL, 'injected index failure'); END;",
            [],
        )
        .expect("create failure trigger");
    }
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000035");
    memory.path = None;

    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: Some(IndexProjection { safe_body: Some("replayablesafeprojection".to_string()) }),
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("injected index failure");

    assert!(err.outcome.committed);
    assert!(!err.outcome.indexed);
    assert_eq!(err.kind, memory_substrate::WriteFailureKind::IndexAfterCommitFailed);
    assert_eq!(err.outcome.repair_required, Some(RepairRequired::PendingIndex));
    assert!(roots.runtime.join("pending/encrypted-index-ops.jsonl").exists());

    {
        let conn = rusqlite::Connection::open(&sqlite_path).expect("open index");
        conn.execute("DROP TRIGGER fail_encrypted_index", []).expect("drop failure trigger");
    }
    drop(substrate);
    let reopened = Substrate::open(roots.clone()).await.expect("reopen");
    assert_eq!(
        reopened
            .query_chunks(ChunkQuery { text: Some("replayablesafeprojection".to_string()), triple: None, vector: None })
            .await
            .expect("safe projection replayed")
            .len(),
        1
    );
    // Q11 (open-questions-resolved §Q11): pending queue files are deleted after
    // successful replay; the legacy `.compacted.jsonl` artefact is never produced.
    assert!(!roots.runtime.join("pending/encrypted-index-ops.jsonl").exists());
    assert!(!roots.runtime.join("pending/encrypted-index-ops.compacted.jsonl").exists());
}

#[tokio::test]
async fn encrypted_index_repair_without_safe_projection_never_persists_metadata_body_and_hash_mismatch_blocks_open() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let sqlite_path = roots.runtime.join("index.sqlite");
    {
        let conn = rusqlite::Connection::open(&sqlite_path).expect("open index");
        conn.execute(
            "CREATE TRIGGER fail_encrypted_index_no_projection BEFORE INSERT ON memories BEGIN SELECT RAISE(FAIL, 'injected index failure'); END;",
            [],
        )
        .expect("create failure trigger");
    }
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000036");
    memory.path = None;
    memory.body = "supersecretpendingrepairneedle".to_string();

    let err = substrate
        .write_encrypted(EncryptedWriteRequest {
            operation_id: None,
            metadata_memory: memory.clone(),
            ciphertext: b"ciphertext".to_vec(),
            safe_index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::RequiresEncryption,
        })
        .await
        .expect_err("injected index failure");
    assert_eq!(err.outcome.repair_required, Some(RepairRequired::PendingIndex));
    let pending_path = roots.runtime.join("pending/encrypted-index-ops.jsonl");
    let pending_text = std::fs::read_to_string(&pending_path).expect("pending repair");
    assert!(!pending_text.contains("supersecretpendingrepairneedle"));

    {
        let conn = rusqlite::Connection::open(&sqlite_path).expect("open index");
        conn.execute("DROP TRIGGER fail_encrypted_index_no_projection", []).expect("drop failure trigger");
    }
    std::fs::write(
        roots.repo.join(format!("encrypted/agent/patterns/{}.md", memory.frontmatter.id.as_str())),
        b"tampered",
    )
    .expect("tamper ciphertext");
    drop(substrate);

    // Q11 + Phase 7 reconcile contract: hash-mismatched encrypted repair ops
    // defer (stay in queue) instead of aborting reconciliation. Substrate::open
    // succeeds; the pending file remains for the next replay attempt; no
    // legacy `.compacted.jsonl` artefact is produced (Q11).
    let _reopened = Substrate::open(roots.clone()).await.expect("hash mismatch defers, open succeeds");
    assert!(pending_path.exists(), "hash-mismatched encrypted repair must remain durable");
    assert!(!roots.runtime.join("pending/encrypted-index-ops.compacted.jsonl").exists());
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
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
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        // Body includes the id so each fixture produces a distinct chunk_id —
        // works around R-IX-4 (chunk_id has UNIQUE constraint and is content-
        // addressed; identical bodies across memories collide in the index).
        body: format!("body for {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
