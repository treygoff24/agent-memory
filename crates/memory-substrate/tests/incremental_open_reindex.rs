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

/// Warm open with many drifted plaintext files should reindex every stale row
/// while streaming repairs through one all-or-nothing index transaction.
#[tokio::test]
async fn warm_open_reindexes_large_plaintext_drift_set() {
    const DRIFTED_MEMORY_COUNT: usize = 70;

    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let mut memories = (0..DRIFTED_MEMORY_COUNT)
        .map(|offset| sample_memory(&format!("mem_20260424_a1b2c3d4e5f60718_{:06}", 60_000 + offset)))
        .collect::<Vec<_>>();
    for memory in &memories {
        write_through_api(&substrate, memory).await;
    }
    drop(substrate);

    for (offset, memory) in memories.iter_mut().enumerate() {
        memory.body = format!("batchdriftneedle_{offset:03} offline edit");
        write_memory_file(&roots, memory);
    }

    let reopened = Substrate::open(roots).await.expect("reopen");

    assert_eq!(
        reopened.startup_reconcile_report().reindexed_memories as usize,
        DRIFTED_MEMORY_COUNT,
        "every drifted plaintext file reindexes in one commit chunk (70 < REINDEX_COMMIT_CHUNK)"
    );
    for offset in [0, 63, 64, DRIFTED_MEMORY_COUNT - 1] {
        let hits = reopened
            .query_chunks(ChunkQuery {
                text: Some(format!("batchdriftneedle_{offset:03}")),
                triple: None,
                vector: None,
            })
            .await
            .expect("query reindexed edit");
        assert_eq!(hits.len(), 1, "edited body should be searchable for batch offset {offset}");
    }
}

/// Phase-6 reindex commits drifted rows in bounded chunks (REINDEX_COMMIT_CHUNK).
/// A drift set larger than one chunk must still reindex every file across the
/// multiple mid-walk commits and the post-walk supersession resync — the
/// multi-chunk loop the sub-chunk warm/rollback tests never reach.
#[tokio::test]
async fn warm_open_reindexes_drift_set_spanning_multiple_commit_chunks() {
    // > REINDEX_COMMIT_CHUNK (512): the walk flushes at least one full chunk
    // mid-loop and a partial chunk after, crossing the boundary 70-file tests miss.
    const UNINDEXED_MEMORY_COUNT: usize = 600;

    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    drop(substrate);

    // Write the memory files straight to disk (no API write, so they stay
    // unindexed); the next open must reindex them through the chunked walk.
    let memories = (0..UNINDEXED_MEMORY_COUNT)
        .map(|offset| sample_memory(&format!("mem_20260424_a1b2c3d4e5f60718_{:06}", 80_000 + offset)))
        .collect::<Vec<_>>();
    for memory in &memories {
        write_memory_file(&roots, memory);
    }

    let reopened = Substrate::open(roots).await.expect("reopen");

    assert_eq!(
        reopened.startup_reconcile_report().reindexed_memories as usize,
        UNINDEXED_MEMORY_COUNT,
        "every file reindexes even when the drift set spans multiple commit chunks"
    );
    // Spot-check rows on both sides of the 512 commit boundary.
    for offset in [0, 511, 512, UNINDEXED_MEMORY_COUNT - 1] {
        assert_eq!(
            query_by_id(&reopened, &memories[offset].frontmatter.id).await.len(),
            1,
            "memory at chunk offset {offset} should be indexed"
        );
    }
}

/// If a later stale Markdown file is malformed, startup reconciliation must not
/// leave earlier stale files partially repaired in the derived index. The next
/// successful open should still count and repair the whole drift set.
#[tokio::test]
async fn warm_open_rolls_back_reindex_transaction_when_later_stale_file_is_malformed() {
    const DRIFTED_MEMORY_COUNT: usize = 70;

    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let mut memories = (0..DRIFTED_MEMORY_COUNT)
        .map(|offset| sample_memory(&format!("mem_20260424_a1b2c3d4e5f60718_{:06}", 70_000 + offset)))
        .collect::<Vec<_>>();
    for memory in &memories {
        write_through_api(&substrate, memory).await;
    }
    drop(substrate);

    for (offset, memory) in memories.iter_mut().enumerate() {
        memory.body = format!("rollbackneedle_{offset:03} offline edit");
        write_memory_file(&roots, memory);
    }
    let malformed = memories.last().expect("last memory").path.clone().expect("path");
    write_malformed_memory_file(&roots, &malformed);

    let error = match Substrate::open(roots.clone()).await {
        Ok(_) => panic!("malformed stale file should block open"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("operator repair required"), "{error}");

    write_memory_file(&roots, memories.last().expect("last memory"));
    let reopened = Substrate::open(roots).await.expect("reopen after repair");

    assert_eq!(
        reopened.startup_reconcile_report().reindexed_memories as usize,
        DRIFTED_MEMORY_COUNT,
        "failed open must not partially commit earlier stale-file repairs"
    );
    for offset in [0, 63, 64, DRIFTED_MEMORY_COUNT - 1] {
        let hits = reopened
            .query_chunks(ChunkQuery { text: Some(format!("rollbackneedle_{offset:03}")), triple: None, vector: None })
            .await
            .expect("query reindexed edit");
        assert_eq!(hits.len(), 1, "edited body should be searchable after repaired open for offset {offset}");
    }
}

/// Public `with_index` is for read-only consumers; accidental writes through
/// the live SQLite connection must fail rather than mutating derived state.
#[tokio::test]
async fn with_index_rejects_accidental_live_index_writes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_080001");
    write_through_api(&substrate, &memory).await;

    let result = substrate.with_index(|index| {
        index
            .connection()
            .execute("DELETE FROM memories WHERE id = ?1", [memory.frontmatter.id.as_str()])
            .map(drop)
            .map_err(Into::into)
    });

    assert!(result.is_err(), "query_only should reject writes through with_index");
    assert_eq!(
        query_by_id(&substrate, &memory.frontmatter.id).await.len(),
        1,
        "rejected write must leave the index row intact"
    );
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

    let reopened = Substrate::open(roots.clone()).await.expect("reopen");

    // The active triple's missing job was re-enqueued during open. Read the
    // active triple from the substrate rather than hardcoding it: the
    // bootstrapped default is the production Qwen3 triple, not synthetic/32.
    let active = reopened.active_embedding_triple().expect("active triple");
    let connection = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("reopen index");
    assert_eq!(
        memory_substrate::index::reconcile_pending_jobs(
            &connection,
            &active,
            memory_substrate::EmbeddingLaneEligibility::AllTiers,
        )
        .expect("pending jobs"),
        1,
        "open must reconcile active embedding jobs"
    );
}

/// Phase-6 index consistency must skip repo `.md` files that are not canonical
/// memories: Stream F dream journals (frontmatter-less prose the dream pass
/// legitimately writes) and stray non-Stream-A files (e.g. a model-cache README
/// when the runtime dir nests inside the repo). Regression: the first live
/// dream run broke every subsequent `Substrate::open` with "bad shape for
/// frontmatter delimiters".
#[tokio::test]
async fn open_tolerates_dream_journal_and_non_stream_a_markdown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_090001");
    write_memory_file(&roots, &memory);
    drop(substrate);

    // Frontmatter-less Stream F prose artifact, exactly what `dream now` writes.
    let journal = roots.repo.join("dreams/journal/me/2026-07-08.md");
    std::fs::create_dir_all(journal.parent().expect("parent")).expect("dirs");
    std::fs::write(&journal, "# Dream reflection\n\nProse only, no frontmatter.\n").expect("write journal");

    // Non-Stream-A markdown inside the repo (runtime-dir artifact shape).
    let stray = roots.repo.join(".memoryd/models/README.md");
    std::fs::create_dir_all(stray.parent().expect("parent")).expect("dirs");
    std::fs::write(&stray, "---\nlicense: apache-2.0\n---\nA model card, not a memory.\n").expect("write stray");

    let reopened = Substrate::open(roots.clone()).await.expect("open must tolerate non-memory markdown");
    let report = reopened.startup_reconcile_report();
    assert!(report.blocking_conflicts.is_empty(), "prose artifacts must not register as conflicts");

    // The canonical memory is still indexed and recallable.
    let hits = reopened
        .query_chunks(ChunkQuery { text: Some("body".to_string()), triple: None, vector: None })
        .await
        .expect("query plaintext");
    assert!(!hits.is_empty(), "canonical memory still indexed alongside prose artifacts");
}

// helpers

/// Regression: two distinct memories with byte-identical bodies must both index
/// without colliding on `memory_chunks.chunk_id`. Under the old text-only
/// derivation the second write crashed with `UNIQUE constraint failed:
/// memory_chunks.chunk_id`; the spec §10.3 derivation folds the memory id and
/// ordinal into the id so identical text in two memories stays distinct.
#[tokio::test]
async fn two_memories_with_identical_body_both_index_and_are_recallable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let shared_body = "an identical paragraph of body text shared verbatim by two distinct memories";
    let mut first = sample_memory("mem_20260424_a1b2c3d4e5f60718_020001");
    first.body = shared_body.to_string();
    let mut second = sample_memory("mem_20260424_a1b2c3d4e5f60718_020002");
    second.body = shared_body.to_string();

    // Both writes must succeed — the second is the one that used to crash.
    write_through_api(&substrate, &first).await;
    write_through_api(&substrate, &second).await;

    // The shared text is body-indexed and the FTS query returns BOTH memories.
    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("verbatim".to_string()), triple: None, vector: None })
        .await
        .expect("query chunks");
    let ids: std::collections::HashSet<&str> = hits.iter().map(|hit| hit.memory_id.as_str()).collect();
    assert!(ids.contains(first.frontmatter.id.as_str()), "first memory must be recallable");
    assert!(ids.contains(second.frontmatter.id.as_str()), "second memory must be recallable");
}

/// migrate_v5 changes the `chunk_id` derivation, so it must force a full body
/// reindex by invalidating `file_hash`. Simulate a pre-v5 index whose chunk rows
/// are gone but whose `memories.file_hash` still matches disk (the reconciliation
/// short-circuit trap), then confirm a normal open rebuilds the chunks.
#[tokio::test]
async fn v5_migration_forces_chunk_reindex_on_open() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_020003");
    memory.body = "a body whose chunks must be rebuilt after the v5 migration forces a reindex".to_string();
    write_through_api(&substrate, &memory).await;
    drop(substrate);

    // Simulate a pre-v5 database: drop the v5 migration row and empty the chunk
    // table, leaving `memories.file_hash` matching disk so a plain reindex would
    // short-circuit and never repopulate.
    {
        let conn = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("open index");
        conn.execute("DELETE FROM schema_migrations WHERE version >= 5", []).expect("simulate pre-v5");
        conn.execute("DELETE FROM memory_chunks", []).expect("drop chunks");
    }

    // A normal open runs migrate_v5 (file_hash invalidation) → reconciliation
    // rechunks every memory → the emptied chunks come back.
    let reopened = Substrate::open(roots.clone()).await.expect("reopen");
    let hits = reopened
        .query_chunks(ChunkQuery { text: Some("rebuilt".to_string()), triple: None, vector: None })
        .await
        .expect("query chunks");
    assert!(!hits.is_empty(), "v5 migration must force a reindex that repopulates memory_chunks");
}

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

fn write_malformed_memory_file(roots: &Roots, repo_path: &RepoPath) {
    let path = roots.repo.join(repo_path.as_path());
    std::fs::write(&path, "---\nid: [unterminated\n---\nmalformed").expect("write malformed file");
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
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        // Body embeds the id so each memory yields distinct chunk_ids (chunk_id
        // is content-hash derived; identical bodies would collide on UNIQUE).
        body: format!("body for {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
