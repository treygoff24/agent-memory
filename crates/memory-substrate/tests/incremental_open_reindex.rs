//! Task 1.2 — incremental `Substrate::open` reindexing.
//!
//! `open` no longer runs a full O(n) reindex. It runs an incremental sweep:
//! orphan-row cleanup + encrypted-tier hash-compare reindex +
//! `reconcile_active_embedding_jobs`, leaving plaintext freshness to phase-6
//! stale detection. These tests pin the five behaviors the review called out.

use memory_substrate::*;

/// (a) Fresh index, plaintext + encrypted files on disk before first open →
/// both tiers get indexed.
#[tokio::test]
async fn fresh_open_indexes_plaintext_and_encrypted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));

    // Init creates the substrate marker + empty index, then we close it.
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    drop(substrate);

    // Lay down one plaintext and one encrypted memory file on disk while closed.
    let plaintext = sample_memory("mem_20260424_a1b2c3d4e5f60718_010001");
    write_memory_file(&roots, &plaintext);
    let encrypted = encrypted_memory("mem_20260424_a1b2c3d4e5f60718_010002", Some("encneedle safe projection body"));
    write_memory_file(&roots, &encrypted);

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");

    // Plaintext is body-indexed and queryable.
    let hits = reopened
        .query_chunks(ChunkQuery { text: Some("body".to_string()), triple: None, vector: None })
        .await
        .expect("query plaintext");
    assert!(!hits.is_empty(), "plaintext memory should be body-indexed at fresh open");

    // Encrypted row is present (indexed via its safe-body projection).
    let (present, _metadata_only) = encrypted_row_state(&roots, &encrypted);
    assert!(present, "encrypted memory should be indexed at fresh open");
}

/// (b) Warm index, one externally-modified plaintext file → only that file is
/// reindexed (phase-6 hash-based stale detection), not every file.
#[tokio::test]
async fn warm_open_reindexes_only_modified_plaintext() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    // Two indexed plaintext memories.
    let stable = sample_memory("mem_20260424_a1b2c3d4e5f60718_020001");
    let mut edited = sample_memory("mem_20260424_a1b2c3d4e5f60718_020002");
    for memory in [&stable, &edited] {
        write_through_api(&substrate, memory).await;
    }

    // Externally edit exactly one file while closed.
    drop(substrate);
    edited.body = "warmeditneedle changed offline".to_string();
    write_memory_file(&roots, &edited);

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");

    // Phase-6 reports a single reindexed memory — not both.
    assert_eq!(reopened.startup_reconcile_report().reindexed_memories, 1, "only the modified file should reindex");

    let hits = reopened
        .query_chunks(ChunkQuery { text: Some("warmeditneedle".to_string()), triple: None, vector: None })
        .await
        .expect("query edited");
    assert_eq!(hits.len(), 1, "edited body should be searchable after open");
}

/// (c) A memory deleted on disk → the orphan sweep removes its index row.
#[tokio::test]
async fn open_orphan_sweep_removes_row_for_deleted_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let kept = sample_memory("mem_20260424_a1b2c3d4e5f60718_030001");
    let doomed = sample_memory("mem_20260424_a1b2c3d4e5f60718_030002");
    write_through_api(&substrate, &kept).await;
    write_through_api(&substrate, &doomed).await;
    drop(substrate);

    // Remove the canonical file out of band (simulating a deleted/moved memory).
    std::fs::remove_file(roots.repo.join(doomed.path.clone().expect("path").as_path())).expect("remove file");

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");

    // The doomed memory's index row is gone; the kept one survives.
    assert!(
        query_by_id(&reopened, &doomed.frontmatter.id).await.is_empty(),
        "orphan sweep should delete the index row for the deleted file"
    );
    assert_eq!(query_by_id(&reopened, &kept.frontmatter.id).await.len(), 1, "untouched memory must remain indexed");
}

/// (d) Encrypted memory present at warm open → indexed metadata-only, even
/// though phase-6 skips the `encrypted/` tier.
#[tokio::test]
async fn open_indexes_encrypted_tier_metadata_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    drop(substrate);

    // No safe-body projection → strictly metadata-only indexing.
    let encrypted = encrypted_memory("mem_20260424_a1b2c3d4e5f60718_040001", None);
    write_memory_file(&roots, &encrypted);

    let _reopened = Substrate::open(roots.clone()).await.expect("reopen");

    let (present, metadata_only) = encrypted_row_state(&roots, &encrypted);
    assert!(present, "encrypted memory should be indexed at open");
    assert!(metadata_only, "encrypted memory must be indexed metadata-only");
}

/// (e) Pending embedding jobs survive open — `reconcile_active_embedding_jobs`
/// re-enqueues missing jobs for the active triple.
#[tokio::test]
async fn open_reconciles_pending_embedding_jobs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    write_through_api(&substrate, &sample_memory("mem_20260424_a1b2c3d4e5f60718_050001")).await;

    // Wipe pending jobs out of band, then close.
    {
        let connection = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("open index");
        connection.execute("DELETE FROM pending_embedding_jobs", []).expect("delete pending jobs");
    }
    drop(substrate);

    let _reopened = Substrate::open(roots.clone()).await.expect("reopen");

    // The active triple's missing job was re-enqueued during open.
    let connection = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("reopen index");
    let active =
        EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "stream-a-test".to_string(), dimension: 32 };
    assert_eq!(
        memory_substrate::index::reconcile_pending_jobs(&connection, &active).expect("pending jobs"),
        1,
        "open must reconcile active embedding jobs"
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn write_through_api(substrate: &Substrate, memory: &Memory) {
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
}

/// Serialize a memory and write it directly to its canonical path on disk.
fn write_memory_file(roots: &Roots, memory: &Memory) {
    let path = roots.repo.join(memory.path.clone().expect("path").as_path());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    let text = memory_substrate::frontmatter::serialize_document(memory).expect("serialize");
    std::fs::write(&path, text).expect("write file");
}

async fn query_by_id(substrate: &Substrate, id: &MemoryId) -> Vec<QueryResult> {
    substrate
        .query_memory(MemoryQuery {
            id: Some(id.clone()),
            tag: None,
            include_metadata_only: true,
            ..MemoryQuery::default()
        })
        .await
        .expect("query by id")
}

/// `(present, metadata_only)` for an encrypted memory, read straight from the
/// derived index so the metadata-only projection flag is observable.
fn encrypted_row_state(roots: &Roots, memory: &Memory) -> (bool, bool) {
    let connection = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("open index");
    connection
        .query_row("SELECT metadata_only FROM memories WHERE id = ?1", [memory.frontmatter.id.as_str()], |row| {
            row.get::<_, i64>(0)
        })
        .map(|metadata_only| (true, metadata_only == 1))
        .unwrap_or((false, false))
}

/// Build an encrypted-tier memory: lives under `encrypted/` and carries the
/// `encryption` envelope extra. With `safe_body: Some(_)` it also exposes a
/// `safe_body` index projection (indexed over the safe body, `metadata_only =
/// false`); with `None` it is indexed strictly metadata-only (`true`), matching
/// the production encrypted-write projection in `collect_reindex_paths`.
fn encrypted_memory(id: &str, safe_body: Option<&str>) -> Memory {
    let mut memory = sample_memory(id);
    memory.path = Some(RepoPath::new(format!("encrypted/agent/patterns/{id}.md")));
    memory.body = "ciphertext-placeholder".to_string();
    memory
        .frontmatter
        .extras
        .insert("encryption".to_string(), serde_json::json!({ "scheme": "age", "recipients": ["test"] }));
    if let Some(safe_body) = safe_body {
        memory.frontmatter.extras.insert("index_projection".to_string(), serde_json::json!({ "safe_body": safe_body }));
    }
    memory
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "reindex".to_string(),
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
        // Body embeds the id so each memory yields distinct chunk_ids (chunk_id
        // is content-hash derived; identical bodies would collide on UNIQUE).
        body: format!("body for {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
