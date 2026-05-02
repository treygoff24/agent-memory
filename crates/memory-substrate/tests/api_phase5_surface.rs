//! Phase 5 public API surface tests.
//!
//! Covers:
//! - B-API-1: `read_memory_envelope` returns the spec §16.2 `MemoryEnvelope` shape.
//! - B-API-4: `drop_embedding_model_report` returns the spec §16.4 `DropTripleReport`.
//! - B-API-9: write refusals emit `WriteRefused` audit events.
//! - B-API-10: lock-poisoning maps to `VectorError::IndexUnavailable`.
//! - B-API-12 (Q4): `Substrate::open` fails with `DeviceIdentityMissing` when
//!   `local-device.yaml` is absent.

use memory_substrate::events::EventKind;
use memory_substrate::*;

#[tokio::test]
async fn read_memory_envelope_returns_plaintext_variant() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_phase5a".to_string()) },
    )
    .await
    .expect("init");

    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_700001");
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

    let envelope = substrate.read_memory_envelope(&memory.frontmatter.id).await.expect("read envelope");
    assert_eq!(envelope.metadata.frontmatter.id, memory.frontmatter.id);
    match envelope.content {
        MemoryContent::Plaintext(body) => assert_eq!(body, memory.body),
        other => panic!("expected Plaintext envelope, got {other:?}"),
    }
}

#[tokio::test]
async fn read_memory_envelope_missing_id_returns_not_found() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_phase5b".to_string()) },
    )
    .await
    .expect("init");

    let err = substrate
        .read_memory_envelope(&MemoryId::new("mem_20260424_a1b2c3d4e5f60718_999999"))
        .await
        .expect_err("missing id");
    assert!(matches!(err, ReadError::NotFound(_)));
}

#[tokio::test]
async fn drop_embedding_model_report_returns_structured_counts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_phase5c".to_string()) },
    )
    .await
    .expect("init");

    let triple =
        EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "stream-a-test".to_string(), dimension: 32 };

    let mut vectorized = sample_memory("mem_20260424_a1b2c3d4e5f60718_700010");
    vectorized.body = "vectorized chunk body".to_string();
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: vectorized.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write vectorized");
    let chunk = memory_substrate::index::chunk_memory(&vectorized).into_iter().next().expect("chunk");
    substrate
        .update_embedding(EmbeddingUpdate {
            chunk_id: chunk.chunk_id.clone(),
            expected_chunk_hash: chunk.body_hash.clone(),
            triple: triple.clone(),
            vector: vec![0.5; 32],
        })
        .await
        .expect("vector update");

    let mut pending = sample_memory("mem_20260424_a1b2c3d4e5f60718_700011");
    pending.body = "pending chunk body".to_string();
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: pending,
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write pending");

    let report = substrate.drop_embedding_model_report(triple.clone()).await.expect("drop report");
    assert_eq!(report.vectors_removed, 1);
    assert_eq!(report.meta_rows_removed, 1);
    assert_eq!(report.pending_jobs_dropped, 1);
    assert!(report.table_dropped);
    assert_eq!(substrate.vector_count(triple).await.expect("vector count"), 0);
}

#[tokio::test]
async fn write_refused_audit_event_is_emitted_on_secret_classification() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_phase5d".to_string()) },
    )
    .await
    .expect("init");
    let before = substrate.events().expect("events before").len();

    let _err = substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: sample_memory("mem_20260424_a1b2c3d4e5f60718_700004"),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Secret,
        })
        .await
        .expect_err("secret refused");

    let after = substrate.events().expect("events after");
    assert_eq!(after.len(), before + 1, "exactly one WriteRefused appended");
    let last = after.last().expect("audit event");
    match &last.kind {
        EventKind::WriteRefused { classification, reason, .. } => {
            assert!(matches!(classification, ClassificationOutcome::Secret));
            assert!(reason.contains("secret"), "reason mentions secret refusal: {reason}");
        }
        other => panic!("expected WriteRefused, got {other:?}"),
    }
}

#[tokio::test]
async fn event_sequence_recovers_from_existing_log_high_water() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_phase5e".to_string()) },
    )
    .await
    .expect("init");
    drop(substrate);

    let event_log = roots.repo.join("events/dev_phase5e.jsonl");
    let seeded_event = memory_substrate::events::Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new("evt_phase5e_seeded"),
        at: chrono::Utc::now(),
        device: DeviceId::new("dev_phase5e"),
        seq: 7,
        operation_id: Some(OperationId::new("op_phase5e_seeded")),
        kind: EventKind::WriteCommitted {
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_700020"),
            path: RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_700020.md"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    };
    memory_substrate::events::append_event(&event_log, &seeded_event).expect("append seeded event");

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");
    let reopened_events = reopened.events().expect("events after reopen");
    assert_eq!(reopened_events.last().expect("completion event").seq, 8);

    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_700021");
    reopened
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
        .expect("write after recovery");

    let events = reopened.events().expect("events");
    let last = events.last().expect("last event");
    assert_eq!(last.seq, 9);
}

#[tokio::test]
async fn open_fails_with_device_identity_missing_when_local_device_yaml_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    // Bootstrap the repo tree without minting a device identity by hand-rolling
    // the steps `Substrate::init` runs except for `git::adopt_clone`.
    let merge_driver = std::env::current_exe().expect("current_exe"); // expect-justified: test
    memory_substrate::git::init_git_repo(&roots.repo, &merge_driver).expect("init git"); // expect-justified: test
    std::fs::create_dir_all(&roots.runtime).expect("create runtime"); // expect-justified: test
    std::fs::write(
        roots.repo.join("config.yaml"),
        "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\n",
    )
    .expect("write config"); // expect-justified: test

    let err = match Substrate::open(roots).await {
        Ok(_) => panic!("open without local-device.yaml must fail"),
        Err(err) => err,
    };
    assert!(matches!(err, OpenError::DeviceIdentityMissing { repair: _ }));
}

#[tokio::test]
async fn open_fails_when_active_embedding_config_is_missing_even_if_device_identity_exists() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    memory_substrate::tree::bootstrap_repo_layout(&roots.repo).expect("bootstrap layout");
    std::fs::create_dir_all(&roots.runtime).expect("create runtime");
    std::fs::write(
        roots.runtime.join("local-device.yaml"),
        "schema_version: 1\ndevice:\n  id: dev_phase5e\n  name: phase5e\n  shard: phase5e\n",
    )
    .expect("write local device");

    let err = match Substrate::open(roots).await {
        Ok(_) => panic!("open without config.yaml must fail"),
        Err(err) => err,
    };

    assert!(matches!(err, OpenError::InvalidRoots(message) if message.contains("config.yaml missing")));
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z")
        .expect("date") // expect-justified: test
        .with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: format!("phase 5 fixture {id}"),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("phase5-test".to_string()),
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
        // Body includes the id so each fixture's chunk_id is distinct (R-IX-4).
        body: format!("phase 5 envelope body for {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
