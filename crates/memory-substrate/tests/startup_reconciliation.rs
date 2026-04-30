use chrono::Utc;
use memory_substrate::events::{Event, EventKind};
use memory_substrate::runtime::reconcile::{
    enqueue_pending_event, enqueue_pending_index, PendingEventOp, PendingIndexKind, PendingIndexOp,
};
use memory_substrate::*;
use std::io::Write;

#[tokio::test]
async fn startup_replays_pending_index_queue_before_queries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000021");
    let path = memory.path.clone().expect("path");
    let text = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize");
    std::fs::create_dir_all(roots.repo.join("agent/patterns")).expect("dirs");
    std::fs::write(roots.repo.join(path.as_path()), text).expect("write file");
    enqueue_pending_index(
        &roots.runtime,
        &PendingIndexOp {
            op_id: OperationId::new("op_pending_index"),
            kind: PendingIndexKind::UpsertPath,
            path: path.clone(),
            memory_id: Some(memory.frontmatter.id.clone()),
            expected_file_hash: None,
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: None,
        },
    )
    .expect("enqueue");
    drop(substrate);

    let reopened = Substrate::open(roots).await.expect("reopen");
    let hits = reopened
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id.clone()),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query");
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn startup_reindex_ingests_valid_offline_edits_without_manual_reindex() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000026");
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
    memory.body = "offlineeditneedle body written while substrate is closed".to_string();
    let path = memory.path.clone().expect("path");
    let text = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize");
    drop(substrate);
    std::fs::write(roots.repo.join(path.as_path()), text).expect("offline edit");

    let reopened = Substrate::open(roots).await.expect("reopen");
    let hits = reopened
        .query_chunks(ChunkQuery { text: Some("offlineeditneedle".to_string()), triple: None, vector: None })
        .await
        .expect("query");
    assert_eq!(hits.len(), 1);
}

#[tokio::test]
async fn startup_reindex_requeues_missing_active_embedding_jobs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000031");
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
    {
        let connection = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("open index");
        connection.execute("DELETE FROM pending_embedding_jobs", []).expect("delete pending jobs");
    }
    drop(substrate);

    let _reopened = Substrate::open(roots.clone()).await.expect("reopen");
    let connection =
        memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("open reopened index");
    let active =
        EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "stream-a-test".to_string(), dimension: 32 };

    assert_eq!(memory_substrate::index::reconcile_pending_jobs(&connection, &active).expect("pending jobs"), 1);
}

#[tokio::test]
async fn startup_reindex_requires_operator_repair_for_invalid_offline_edit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000027");
    let path = memory.path.clone().expect("path");
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
    drop(substrate);
    std::fs::write(roots.repo.join(path.as_path()), "---\nschema_version: 99\n---\nbad").expect("invalid edit");

    let err = match Substrate::open(roots).await {
        Ok(_) => panic!("invalid offline edit should require repair"),
        Err(err) => err,
    };
    assert!(matches!(err, memory_substrate::OpenError::OperatorRepairRequired(_)));
}

#[tokio::test]
async fn startup_reindex_requires_operator_repair_for_plaintext_markdown_under_encrypted_namespace() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000029");
    let text = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize");
    drop(substrate);
    std::fs::create_dir_all(roots.repo.join("encrypted/agent/patterns")).expect("encrypted dirs");
    std::fs::write(roots.repo.join("encrypted/agent/patterns/leak.md"), text).expect("encrypted plaintext leak");

    let err = match Substrate::open(roots).await {
        Ok(_) => panic!("plaintext markdown under encrypted namespace should require repair"),
        Err(err) => err,
    };

    assert!(
        matches!(err, memory_substrate::OpenError::OperatorRepairRequired(message) if message.contains("encrypted namespace"))
    );
}

#[tokio::test]
async fn startup_recovery_requires_operator_repair_for_nonfinal_malformed_event_log_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000030");
    let first = Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new("evt_startup_nonfinal_first"),
        at: Utc::now(),
        device: DeviceId::new("dev_test"),
        seq: 0,
        operation_id: Some(OperationId::new("op_startup_nonfinal_first")),
        kind: EventKind::WriteCommitted {
            id: memory.frontmatter.id.clone(),
            path: memory.path.clone().expect("path"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    };
    let second = Event {
        id: EventId::new("evt_startup_nonfinal_second"),
        operation_id: Some(OperationId::new("op_startup_nonfinal_second")),
        ..first.clone()
    };
    let event_log = roots.repo.join("events/dev_test.jsonl");
    memory_substrate::events::append_event(&event_log, &first).expect("append first");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&event_log)
        .expect("open event log")
        .write_all(b"{malformed nonfinal line\n")
        .expect("write malformed line");
    memory_substrate::events::append_event(&event_log, &second).expect("append second");
    drop(substrate);

    let err = match Substrate::open(roots).await {
        Ok(_) => panic!("nonfinal malformed event line should require repair"),
        Err(err) => err,
    };

    assert!(
        matches!(err, memory_substrate::OpenError::OperatorRepairRequired(message) if message.contains("non-final malformed event log line"))
    );
}

#[tokio::test]
async fn startup_replays_pending_event_queue_and_compacts_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000022");
    let queued = Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new("evt_pending_replay"),
        at: Utc::now(),
        device: DeviceId::new("dev_test"),
        seq: 0,
        operation_id: Some(OperationId::new("op_pending_event")),
        kind: EventKind::WriteCommitted {
            id: memory.frontmatter.id.clone(),
            path: memory.path.clone().expect("path"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    };
    enqueue_pending_event(
        &roots.runtime,
        &PendingEventOp {
            op_id: OperationId::new("op_pending_event"),
            event_id: queued.id.clone(),
            event: queued.clone(),
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: Some("injected append failure".to_string()),
        },
    )
    .expect("enqueue event");
    drop(substrate);

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");
    let events = reopened.events().expect("events");
    assert!(events.iter().any(|event| event.id == queued.id));
    // Q11: delete after replay, not rename to .compacted.jsonl.
    assert!(!roots.runtime.join("pending/events.jsonl").exists());
    assert!(!roots.runtime.join("pending/events.compacted.jsonl").exists());
}

#[tokio::test]
async fn startup_refuses_nonfinal_malformed_pending_event_without_compacting_queue() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000032");
    let event = Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new("evt_pending_before_corruption"),
        at: Utc::now(),
        device: DeviceId::new("dev_test"),
        seq: 0,
        operation_id: Some(OperationId::new("op_pending_before_corruption")),
        kind: EventKind::WriteCommitted {
            id: memory.frontmatter.id.clone(),
            path: memory.path.clone().expect("path"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    };
    enqueue_pending_event(
        &roots.runtime,
        &PendingEventOp {
            op_id: OperationId::new("op_pending_before_corruption"),
            event_id: event.id.clone(),
            event: event.clone(),
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: None,
        },
    )
    .expect("enqueue first pending event");
    let pending_path = roots.runtime.join("pending/events.jsonl");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&pending_path)
        .expect("open pending events")
        .write_all(b"malformed nonfinal pending frame\n")
        .expect("write malformed pending frame");
    enqueue_pending_event(
        &roots.runtime,
        &PendingEventOp {
            op_id: OperationId::new("op_pending_after_corruption"),
            event_id: EventId::new("evt_pending_after_corruption"),
            event,
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: None,
        },
    )
    .expect("enqueue second pending event");
    drop(substrate);

    let err = match Substrate::open(roots.clone()).await {
        Ok(_) => panic!("nonfinal malformed pending frame should require repair"),
        Err(err) => err,
    };

    assert!(
        matches!(err, memory_substrate::OpenError::OperatorRepairRequired(message) if message.contains("pending repair frame"))
    );
    assert!(pending_path.exists(), "corrupted pending queue must remain for operator repair");
    assert!(!roots.runtime.join("pending/events.compacted.jsonl").exists());
}

#[tokio::test]
async fn startup_retains_unresolved_pending_index_ops_when_other_repairs_compact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000028");
    let path = memory.path.clone().expect("path");
    let text = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize");
    std::fs::create_dir_all(roots.repo.join("agent/patterns")).expect("dirs");
    std::fs::write(roots.repo.join(path.as_path()), text).expect("write file");
    enqueue_pending_index(
        &roots.runtime,
        &PendingIndexOp {
            op_id: OperationId::new("op_stale_pending_index"),
            kind: PendingIndexKind::UpsertPath,
            path: path.clone(),
            memory_id: Some(memory.frontmatter.id.clone()),
            expected_file_hash: Some(Sha256::new("sha256:stale")),
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: None,
        },
    )
    .expect("enqueue stale index op");
    let queued = Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new("evt_pending_compacts_alongside_stale_index"),
        at: Utc::now(),
        device: DeviceId::new("dev_test"),
        seq: 0,
        operation_id: Some(OperationId::new("op_pending_event_with_stale_index")),
        kind: EventKind::WriteCommitted {
            id: memory.frontmatter.id.clone(),
            path,
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    };
    enqueue_pending_event(
        &roots.runtime,
        &PendingEventOp {
            op_id: OperationId::new("op_pending_event_with_stale_index"),
            event_id: queued.id.clone(),
            event: queued,
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: Some("force event compaction".to_string()),
        },
    )
    .expect("enqueue event");
    drop(substrate);

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");

    assert!(reopened
        .events()
        .expect("events")
        .iter()
        .any(|event| event.id == EventId::new("evt_pending_compacts_alongside_stale_index")));
    let pending_index = roots.runtime.join("pending/index-ops.jsonl");
    assert!(pending_index.exists(), "stale pending index op must remain active");
    let pending_text = std::fs::read_to_string(pending_index).expect("pending index text");
    assert!(pending_text.contains("op_stale_pending_index"));
    // Q11: delete after replay, not rename to .compacted.jsonl.
    assert!(!roots.runtime.join("pending/events.jsonl").exists());
    assert!(!roots.runtime.join("pending/events.compacted.jsonl").exists());
}

#[tokio::test]
async fn startup_replay_skips_pending_event_already_in_log() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000023");
    let queued = Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new("evt_pending_duplicate"),
        at: Utc::now(),
        device: DeviceId::new("dev_test"),
        seq: 0,
        operation_id: Some(OperationId::new("op_pending_duplicate")),
        kind: EventKind::WriteCommitted {
            id: memory.frontmatter.id.clone(),
            path: memory.path.clone().expect("path"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    };
    let event_log = roots.repo.join("events/dev_test.jsonl");
    memory_substrate::events::append_event(&event_log, &queued).expect("append first");
    enqueue_pending_event(
        &roots.runtime,
        &PendingEventOp {
            op_id: OperationId::new("op_pending_duplicate"),
            event_id: queued.id.clone(),
            event: queued.clone(),
            enqueued_at: Utc::now(),
            attempts: 0,
            last_error: Some("crash before compaction".to_string()),
        },
    )
    .expect("enqueue event");
    drop(substrate);

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");
    let duplicate_count = reopened.events().expect("events").into_iter().filter(|event| event.id == queued.id).count();
    assert_eq!(duplicate_count, 1);
    assert!(!roots.runtime.join("pending/events.jsonl").exists());
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "pending repair".to_string(),
            confidence: 1.0,
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
        body: "pending repair body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
