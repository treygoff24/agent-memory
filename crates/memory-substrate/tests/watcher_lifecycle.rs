use std::time::{Duration, Instant};

use memory_substrate::watcher::{FileEvent, SuppressionLedger, WatchEventKind};
use memory_substrate::*;

#[tokio::test]
async fn watch_subscription_outlives_substrate_until_unsubscribe() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let subscription = substrate.watch().expect("watch");
    std::thread::sleep(Duration::from_millis(500));
    drop(substrate);

    let changed = roots.repo.join("agent/patterns/watch-subscription.md");
    std::fs::write(&changed, "watch me").expect("write");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut observed = false;
    while Instant::now() < deadline {
        if let Ok(event) = subscription.recv_timeout(Duration::from_millis(250)) {
            if event.path == changed || event.path.file_name() == changed.file_name() {
                observed = true;
                break;
            }
        }
    }
    assert!(observed, "watch subscription did not deliver event after Substrate drop");
    subscription.unsubscribe();
}

#[test]
fn watcher_overflow_event_requests_rescan() {
    let event = FileEvent::rescan_required("agent/patterns");

    assert_eq!(event.kind, WatchEventKind::RescanRequired);
    assert_eq!(event.path, std::path::PathBuf::from("agent/patterns"));
}

#[test]
fn suppression_ledger_suppresses_matching_in_flight_and_committed_hashes_only() {
    let path = RepoPath::new("agent/patterns/mem.md");
    let matching_hash = Sha256::new("sha256:matching");
    let other_hash = Sha256::new("sha256:other");
    let mut ledger = SuppressionLedger::default();

    ledger.insert_in_flight(path.clone(), OperationId::new("op_inflight"), matching_hash.clone());
    assert!(ledger.should_suppress(&path, &matching_hash));
    assert!(!ledger.should_suppress(&path, &other_hash));

    ledger.promote_committed(path.clone(), other_hash.clone());
    assert!(ledger.should_suppress(&path, &other_hash));
    assert!(!ledger.should_suppress(&path, &matching_hash));
}

#[tokio::test]
async fn substrate_write_suppresses_own_watcher_event_but_external_edit_is_delivered() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let subscription = substrate.watch().expect("watch");
    std::thread::sleep(Duration::from_millis(500));
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000901");
    let target = roots.repo.join(memory.path.clone().expect("path").as_path());

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

    let quiet_until = Instant::now() + Duration::from_millis(700);
    while Instant::now() < quiet_until {
        if let Ok(event) = subscription.recv_timeout(Duration::from_millis(100)) {
            assert_ne!(event.path, target, "programmatic write should be suppressed");
        }
    }

    std::fs::write(&target, "external edit with different hash").expect("external edit");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut observed = false;
    while Instant::now() < deadline {
        if let Ok(event) = subscription.recv_timeout(Duration::from_millis(250)) {
            if event.path == target || event.path.file_name() == target.file_name() {
                observed = true;
                break;
            }
        }
    }
    assert!(observed, "external edit with different hash should not be suppressed");
    subscription.unsubscribe();
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "watch sample".to_string(),
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
        body: "watch body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
