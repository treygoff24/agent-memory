use memory_substrate::*;

#[test]
fn auxiliary_vectors_are_stale_fenced_queryable_and_invalidated_on_hash_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
    let connection = memory_substrate::index::open_index(&temp.path().join("index.sqlite")).expect("index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, triple.clone());
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000099");
    memory.frontmatter.abstraction = Some("OAuth token policy".into());
    memory.frontmatter.cues = vec!["OAuth refresh".into()];
    index.upsert_memory(&memory, false).expect("upsert");
    let jobs = index.pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers).expect("jobs");
    assert_eq!(jobs.len(), 2);
    let abstraction = jobs.iter().find(|job| job.row_kind == AuxRowKind::Abstraction).expect("abstraction job").clone();
    let cue = jobs.iter().find(|job| job.row_kind == AuxRowKind::Cue).expect("cue job").clone();
    let stale = index
        .update_aux_embedding(&AuxEmbeddingUpdate {
            row_kind: AuxRowKind::Abstraction,
            target_id: abstraction.target_id.clone(),
            expected_content_hash: Sha256::new("sha256:stale"),
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .expect_err("stale fence");
    assert!(matches!(stale, VectorError::StaleAux { .. }));
    index
        .update_aux_embedding(&AuxEmbeddingUpdate {
            row_kind: AuxRowKind::Abstraction,
            target_id: abstraction.target_id.clone(),
            expected_content_hash: abstraction.content_hash.clone(),
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .expect("update");
    assert_eq!(index.query_abstraction_vectors(&triple, &[1.0, 0.0, 0.0], 3, None).expect("query").len(), 1);
    assert_eq!(
        index.all_abstraction_vectors(&triple).expect("enumerate vectors"),
        vec![AbstractionVectorRow { memory_id: memory.frontmatter.id.clone(), vector: vec![1.0, 0.0, 0.0] }]
    );
    index
        .update_aux_embedding(&AuxEmbeddingUpdate {
            row_kind: AuxRowKind::Cue,
            target_id: cue.target_id,
            expected_content_hash: cue.content_hash,
            triple: triple.clone(),
            vector: vec![0.0, 1.0, 0.0],
        })
        .expect("cue update");
    assert_eq!(index.query_cue_vectors(&triple, &[0.0, 1.0, 0.0], 3, None).expect("cue query").len(), 1);
    let chunk = index
        .pending_embedding_jobs(1, EmbeddingLaneEligibility::AllTiers)
        .expect("chunk jobs")
        .pop()
        .expect("chunk job");
    index
        .update_embedding(&EmbeddingUpdate {
            chunk_id: chunk.chunk_id,
            expected_chunk_hash: chunk.content_hash,
            triple: triple.clone(),
            vector: vec![0.0, 0.0, 1.0],
        })
        .expect("chunk update");

    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_embeddings = true;
    index.upsert_memory(&memory, false).expect("direct operator-override upgrade");
    assert_eq!(index.vector_count(&triple).expect("chunk vector count"), 0);
    assert!(index
        .query_abstraction_vectors(&triple, &[1.0, 0.0, 0.0], 3, None)
        .expect("revoked abstraction query")
        .is_empty());
    assert!(index.query_cue_vectors(&triple, &[0.0, 1.0, 0.0], 3, None).expect("revoked cue query").is_empty());
    assert!(!index
        .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers)
        .expect("local override jobs")
        .is_empty());
    assert!(index
        .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::PlaintextOnly)
        .expect("api held-local jobs")
        .is_empty());
    memory.frontmatter.sensitivity = Sensitivity::Public;
    index.upsert_memory(&memory, false).expect("restore public fixture");

    let cue_table = memory_substrate::index::sqlite_vec::aux_vector_table_name(AuxRowKind::Cue, &triple);
    index.connection().execute(&format!("DELETE FROM {cue_table}"), []).expect("simulate missing vector");
    index.reconcile_active_embedding_jobs(EmbeddingLaneEligibility::AllTiers).expect("reconcile missing aux vector");
    assert!(index
        .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers)
        .expect("re-enqueued cue")
        .iter()
        .any(|job| job.row_kind == AuxRowKind::Cue));

    memory.frontmatter.sensitivity = Sensitivity::Confidential;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.frontmatter.retrieval_policy.index_body = false;
    index.upsert_memory(&memory, false).expect("default sensitivity upgrade");
    let abstraction_table =
        memory_substrate::index::sqlite_vec::aux_vector_table_name(AuxRowKind::Abstraction, &triple);
    let remaining: i64 = index
        .connection()
        .query_row(&format!("SELECT COUNT(*) FROM {abstraction_table}"), [], |row| row.get(0))
        .expect("vector count");
    assert_eq!(remaining, 0, "default sensitivity upgrade revokes aux vectors");
    assert!(index.pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers).unwrap().is_empty());

    memory.frontmatter.retrieval_policy.index_embeddings = true;
    index.upsert_memory(&memory, false).expect("operator override");
    assert!(!index.pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers).unwrap().is_empty());
    assert!(index.pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::PlaintextOnly).unwrap().is_empty());
    memory.frontmatter.sensitivity = Sensitivity::Public;
    memory.frontmatter.retrieval_policy.index_body = true;
    index.upsert_memory(&memory, false).expect("restore public fixture");

    memory.frontmatter.abstraction = Some("Rotated token policy".into());
    index.upsert_memory(&memory, false).expect("changed upsert");
    assert!(index.query_abstraction_vectors(&triple, &[1.0, 0.0, 0.0], 3, None).expect("query").is_empty());
    assert!(index
        .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers)
        .expect("replacement job")
        .iter()
        .any(|job| job.row_kind == AuxRowKind::Abstraction && job.content_hash != abstraction.content_hash));

    for status in
        [MemoryStatus::Superseded, MemoryStatus::Archived, MemoryStatus::Tombstoned, MemoryStatus::Quarantined]
    {
        memory.frontmatter.status = status;
        index.upsert_memory(&memory, false).expect("leave servable set");
        assert_eq!(
            index
                .connection()
                .query_row("SELECT COUNT(*) FROM memory_abstractions", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(index.pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers).unwrap().is_empty());
        memory.frontmatter.status = MemoryStatus::Active;
        index.upsert_memory(&memory, false).expect("re-enter servable set");
    }
    memory.frontmatter.status = MemoryStatus::Pinned;
    index.upsert_memory(&memory, false).expect("enter pinned servable set");
    assert_eq!(
        index
            .connection()
            .query_row("SELECT COUNT(*) FROM memory_abstractions", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let job = index
        .pending_aux_embedding_jobs(10, EmbeddingLaneEligibility::AllTiers)
        .unwrap()
        .into_iter()
        .find(|job| job.row_kind == AuxRowKind::Abstraction)
        .expect("re-materialized abstraction job");
    index
        .update_aux_embedding(&AuxEmbeddingUpdate {
            row_kind: job.row_kind,
            target_id: job.target_id,
            expected_content_hash: job.content_hash,
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .expect("drain re-materialized abstraction");
    assert_eq!(index.query_abstraction_vectors(&triple, &[1.0, 0.0, 0.0], 3, None).unwrap().len(), 1);
}

#[test]
fn active_triple_switch_reenqueues_abstraction_and_cue_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("index.sqlite");
    let old = EmbeddingTriple { provider: "synthetic".into(), model_ref: "old".into(), dimension: 3 };
    let new = EmbeddingTriple { provider: "synthetic".into(), model_ref: "new".into(), dimension: 5 };
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000098");
    memory.frontmatter.abstraction = Some("OAuth token policy".into());
    memory.frontmatter.cues = vec!["OAuth refresh".into()];
    let connection = memory_substrate::index::open_index(&path).expect("old index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, old);
    index.upsert_memory(&memory, false).expect("seed semantic rows");
    drop(index);

    let connection = memory_substrate::index::open_index(&path).expect("reopen index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, new.clone());
    index.reconcile_active_embedding_jobs(EmbeddingLaneEligibility::AllTiers).expect("reconcile switched triple");
    let queued: i64 = index
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM aux_pending_embedding_jobs WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            rusqlite::params![new.provider, new.model_ref, i64::from(new.dimension)],
            |row| row.get(0),
        )
        .expect("new aux jobs");
    assert_eq!(queued, 2);
}

#[tokio::test]
async fn update_embedding_rejects_wrong_dimension_and_stale_hash() {
    let (_temp, substrate, memory) = seeded_substrate().await;
    let triple = EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "unit".to_string(), dimension: 3 };
    let chunk = first_index_chunk(&memory);
    let chunk_id = chunk.chunk_id.clone();
    let correct_hash = chunk.body_hash.clone();

    let wrong_dimension = substrate
        .update_embedding(EmbeddingUpdate {
            chunk_id: chunk_id.clone(),
            expected_chunk_hash: correct_hash.clone(),
            triple: triple.clone(),
            vector: vec![1.0, 0.0],
        })
        .await
        .expect_err("dimension mismatch");
    assert!(matches!(wrong_dimension, VectorError::DimensionMismatch { expected: 3, found: 2 }));

    let stale = substrate
        .update_embedding(EmbeddingUpdate {
            chunk_id: chunk_id.clone(),
            expected_chunk_hash: Sha256::new("sha256:stale"),
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect_err("stale hash");
    assert!(matches!(stale, VectorError::StaleChunk { .. }));
}

#[test]
fn aux_hash_change_between_fetch_and_update_rejects_stale_and_preserves_replacement_job() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("index.sqlite");
    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
    let connection = memory_substrate::index::open_index(&path).expect("index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, triple.clone());
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000097");
    memory.frontmatter.abstraction = Some("OAuth token policy".into());
    index.upsert_memory(&memory, false).expect("seed abstraction");

    let job = index
        .pending_aux_embedding_jobs(1, EmbeddingLaneEligibility::AllTiers)
        .expect("jobs")
        .into_iter()
        .find(|job| job.row_kind == AuxRowKind::Abstraction)
        .expect("abstraction job");
    let old_hash = job.content_hash.clone();

    // Interleaved edit: the abstraction changes before the worker writes the vector.
    memory.frontmatter.abstraction = Some("Rotated token policy".into());
    index.upsert_memory(&memory, false).expect("hash change");

    let stale = index
        .update_aux_embedding(&AuxEmbeddingUpdate {
            row_kind: AuxRowKind::Abstraction,
            target_id: job.target_id.clone(),
            expected_content_hash: old_hash.clone(),
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .expect_err("stale aux");
    assert!(matches!(stale, VectorError::StaleAux { .. }));

    let replacement = index
        .pending_aux_embedding_jobs(1, EmbeddingLaneEligibility::AllTiers)
        .expect("replacement job")
        .into_iter()
        .find(|job| job.row_kind == AuxRowKind::Abstraction)
        .expect("replacement abstraction job");
    assert_ne!(replacement.content_hash, old_hash, "replacement job must have the new hash");
    assert_eq!(replacement.target_id, job.target_id);

    // W2-F2 predicate pin (round-2 review): the TOCTOU window itself — hash
    // check passing, then a concurrent replace landing before the job delete —
    // is unreachable through the public API without injection hooks, because
    // the fix moved re-validation and the delete into one transaction on one
    // `&mut` connection. What IS pinnable is the deletion-scoping half of the
    // fix: the pending-job delete must be content_hash-scoped, so a delete
    // keyed to a STALE hash can never remove a FRESH job. Run the exact
    // production predicate against the replacement job and assert survival.
    let deleted = index
        .connection()
        .execute(
            memory_substrate::index::AUX_PENDING_JOB_HASH_SCOPED_DELETE_SQL,
            rusqlite::params![
                "abstraction",
                replacement.target_id,
                triple.provider,
                triple.model_ref,
                i64::from(triple.dimension),
                old_hash.as_str(),
            ],
        )
        .expect("hash-scoped delete");
    assert_eq!(deleted, 0, "a stale-hash-scoped delete must never remove the fresh replacement job");
    assert!(
        index
            .pending_aux_embedding_jobs(1, EmbeddingLaneEligibility::AllTiers)
            .expect("fresh job survives")
            .iter()
            .any(|job| job.row_kind == AuxRowKind::Abstraction),
        "replacement job survives a stale-scoped delete"
    );
}

#[tokio::test]
async fn update_embedding_persists_and_drop_triple_removes_vectors() {
    let (_temp, substrate, memory) = seeded_substrate().await;
    let triple = EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "unit".to_string(), dimension: 3 };
    let chunk = first_index_chunk(&memory);
    let chunk_id = chunk.chunk_id.clone();
    let hash = chunk.body_hash.clone();
    substrate
        .update_embedding(EmbeddingUpdate {
            chunk_id,
            expected_chunk_hash: hash,
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("update");
    assert_eq!(substrate.vector_count(triple.clone()).await.expect("count"), 1);
    assert_eq!(substrate.drop_embedding_model_report(triple.clone()).await.expect("drop").vectors_removed, 1);
    assert_eq!(substrate.vector_count(triple).await.expect("count"), 0);
}

#[tokio::test]
async fn dropped_triple_returns_unknown_and_cannot_be_recreated_by_stale_worker() {
    let (_temp, substrate, memory) = seeded_substrate().await;
    let triple = EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "unit".to_string(), dimension: 3 };
    let chunk = first_index_chunk(&memory);
    let chunk_id = chunk.chunk_id.clone();
    let hash = chunk.body_hash.clone();
    substrate
        .update_embedding(EmbeddingUpdate {
            chunk_id: chunk_id.clone(),
            expected_chunk_hash: hash.clone(),
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect("update");
    assert_eq!(substrate.drop_embedding_model_report(triple.clone()).await.expect("drop").vectors_removed, 1);

    let query_err = substrate
        .query_chunks(ChunkQuery { text: None, triple: Some(triple.clone()), vector: Some(vec![1.0, 0.0, 0.0]), namespaces: None })
        .await
        .expect_err("dropped query is unknown");
    assert!(matches!(query_err, SubstrateError::Vector(VectorError::UnknownEmbeddingTriple(_))));
    let update_err = substrate
        .update_embedding(EmbeddingUpdate {
            chunk_id,
            expected_chunk_hash: hash,
            triple: triple.clone(),
            vector: vec![1.0, 0.0, 0.0],
        })
        .await
        .expect_err("stale worker rejected");
    assert!(matches!(update_err, VectorError::UnknownEmbeddingTriple(_)));
    assert_eq!(substrate.vector_count(triple).await.expect("count"), 0);
}

#[test]
fn vector_table_names_are_collision_resistant_for_exact_triples() {
    let left = EmbeddingTriple { provider: "a_b".to_string(), model_ref: "c".to_string(), dimension: 3 };
    let right = EmbeddingTriple { provider: "a".to_string(), model_ref: "b_c".to_string(), dimension: 3 };
    assert_ne!(
        memory_substrate::index::sqlite_vec::vector_table_name(&left),
        memory_substrate::index::sqlite_vec::vector_table_name(&right)
    );
}

#[tokio::test]
async fn plaintext_writes_enqueue_pending_embedding_jobs_for_active_triple() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000045");
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
    let connection = memory_substrate::index::open_index(&roots.runtime.join("index.sqlite")).expect("open index");
    // The active triple is the production Qwen3 default, not synthetic/32 — read
    // it from the substrate so the assertion tracks the bootstrapped triple.
    let active = substrate.active_embedding_triple().expect("active triple");

    assert_eq!(
        memory_substrate::index::reconcile_pending_jobs(&connection, &active, EmbeddingLaneEligibility::AllTiers)
            .expect("pending jobs"),
        1
    );
}

#[tokio::test]
async fn query_chunks_uses_sqlite_vec_nearest_neighbors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let mut close = sample_memory("mem_20260424_a1b2c3d4e5f60718_000042");
    close.body = "close vector chunk".to_string();
    let mut far = sample_memory("mem_20260424_a1b2c3d4e5f60718_000043");
    far.body = "far vector chunk".to_string();
    for memory in [close.clone(), far.clone()] {
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
    let triple = EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "unit".to_string(), dimension: 3 };
    for (memory, vector) in [(&close, vec![0.9, 0.0, 0.0]), (&far, vec![0.0, 0.9, 0.0])] {
        let chunk = first_index_chunk(memory);
        substrate
            .update_embedding(EmbeddingUpdate {
                chunk_id: chunk.chunk_id,
                expected_chunk_hash: chunk.body_hash,
                triple: triple.clone(),
                vector,
            })
            .await
            .expect("embedding");
    }

    let hits = substrate
        .query_chunks(ChunkQuery { text: None, triple: Some(triple), vector: Some(vec![1.0, 0.0, 0.0]), namespaces: None })
        .await
        .expect("vector query");

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].memory_id, close.frontmatter.id);
    assert!(hits[0].score <= hits[1].score);
}

#[test]
fn chunk_ids_change_when_content_changes_at_the_same_offset() {
    let mut first = sample_memory("mem_20260424_a1b2c3d4e5f60718_000044");
    first.body = "same offset first body".to_string();
    let mut second = first.clone();
    second.body = "same offset second body".to_string();

    let first_chunk = first_index_chunk(&first);
    let second_chunk = first_index_chunk(&second);

    assert_ne!(first_chunk.chunk_id, second_chunk.chunk_id);
    assert_eq!(first_chunk.start_byte, second_chunk.start_byte);
}

#[test]
fn chunking_preserves_utf8_when_multibyte_character_crosses_byte_budget() {
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000046");
    memory.body = format!("{}étail", "a".repeat(4095));

    let chunks = memory_substrate::index::chunk_memory(&memory);
    let rejoined = chunks.iter().map(|chunk| chunk.text.as_str()).collect::<String>();

    assert_eq!(rejoined, memory.body);
    assert!(chunks.iter().all(|chunk| !chunk.text.contains('\u{fffd}')));
    assert_eq!(chunks[0].end_byte, 4095);
    assert_eq!(chunks[1].start_byte, 4095);
}

#[tokio::test]
async fn knn_active_memories_filters_by_scope_status_and_returns_nearest_per_memory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_knn".to_string()) })
            .await
            .expect("init");

    // Two agent-scoped memories (in scope for an `agent` candidate) and one
    // user-scoped memory (out of scope). The user-scoped row must never surface
    // for an agent-scope query even though its vector is nearest.
    let mut near = sample_memory("mem_20260424_a1b2c3d4e5f60718_000050");
    near.body = "near agent memory".to_string();
    let mut far = sample_memory("mem_20260424_a1b2c3d4e5f60718_000051");
    far.body = "far agent memory".to_string();
    let mut other_scope = sample_memory("mem_20260424_a1b2c3d4e5f60718_000052");
    other_scope.body = "user scoped memory".to_string();
    other_scope.frontmatter.scope = Scope::User;
    other_scope.frontmatter.retrieval_policy.max_scope = Scope::User;
    other_scope.path = Some(RepoPath::new("me/knowledge/mem_20260424_a1b2c3d4e5f60718_000052.md"));

    for memory in [near.clone(), far.clone(), other_scope.clone()] {
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

    let triple = EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "knn".to_string(), dimension: 3 };
    // `other_scope` is the literal nearest vector to the query, proving the scope
    // filter — not distance — is what excludes it.
    for (memory, vector) in
        [(&near, vec![0.95, 0.0, 0.0]), (&far, vec![0.0, 0.9, 0.0]), (&other_scope, vec![1.0, 0.0, 0.0])]
    {
        let chunk = first_index_chunk(memory);
        substrate
            .update_embedding(EmbeddingUpdate {
                chunk_id: chunk.chunk_id,
                expected_chunk_hash: chunk.body_hash,
                triple: triple.clone(),
                vector,
            })
            .await
            .expect("embedding");
    }

    let hits = substrate
        .knn_active_memories(&triple, &[1.0, 0.0, 0.0], &[Scope::Agent, Scope::Subagent], 5)
        .await
        .expect("knn");

    let ids: Vec<&str> = hits.iter().map(|hit| hit.memory_id.as_str()).collect();
    assert!(ids.contains(&near.frontmatter.id.as_str()), "in-scope near memory surfaces");
    assert!(ids.contains(&far.frontmatter.id.as_str()), "in-scope far memory surfaces");
    assert!(!ids.contains(&other_scope.frontmatter.id.as_str()), "out-of-scope memory must be filtered out");
    assert_eq!(hits[0].memory_id.as_str(), near.frontmatter.id.as_str(), "nearest in-scope memory ranks first");
    // One row per memory, and the nearest neighbour's cosine is higher than the
    // farther one's.
    assert!(hits[0].similarity > hits[1].similarity, "ordered by descending similarity");

    // Empty scope set returns nothing rather than every memory.
    assert!(substrate.knn_active_memories(&triple, &[1.0, 0.0, 0.0], &[], 5).await.expect("empty scopes").is_empty());

    // Invariant 3: an unknown triple is a typed error, never a silent empty set.
    let unknown = EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "absent".to_string(), dimension: 3 };
    let err = substrate
        .knn_active_memories(&unknown, &[1.0, 0.0, 0.0], &[Scope::Agent], 5)
        .await
        .expect_err("unknown triple is typed");
    assert!(matches!(err, VectorError::UnknownEmbeddingTriple(_)));
}

async fn seeded_substrate() -> (tempfile::TempDir, Substrate, Memory) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) })
            .await
            .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000041");
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
    (temp, substrate, memory)
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "vector".to_string(),
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
        body: "vector body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn first_index_chunk(memory: &Memory) -> memory_substrate::index::Chunk {
    memory_substrate::index::chunk_memory(memory).into_iter().next().expect("memory has one chunk")
}

#[test]
fn abstraction_compile_candidates_include_encrypted_and_metadata_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("index.sqlite");
    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
    let connection = memory_substrate::index::open_index(&path).expect("index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, triple);

    let plaintext = sample_memory("mem_20260424_a1b2c3d4e5f60718_000090");
    index.upsert_memory(&plaintext, false).expect("plaintext active");

    let mut encrypted = sample_memory("mem_20260424_a1b2c3d4e5f60718_000091");
    encrypted.path = Some(RepoPath::new("encrypted/mem_20260424_a1b2c3d4e5f60718_000091.md"));
    index.upsert_memory(&encrypted, false).expect("encrypted active");

    let mut metadata_only = sample_memory("mem_20260424_a1b2c3d4e5f60718_000092");
    metadata_only.frontmatter.retrieval_policy.index_body = false;
    metadata_only.frontmatter.retrieval_policy.index_embeddings = false;
    index.upsert_memory(&metadata_only, true).expect("metadata-only active");

    let candidates = index.abstraction_compile_candidates(10).expect("candidates");
    assert_eq!(candidates.len(), 3);
    assert!(candidates.contains(&plaintext.frontmatter.id));
    assert!(candidates.contains(&encrypted.frontmatter.id));
    assert!(candidates.contains(&metadata_only.frontmatter.id));
}

#[test]
fn amended_rows_leave_candidate_pool_even_without_servable_abstraction_row() {
    // W5 live-backfill regression: encrypted/metadata-only rows never get a
    // servable `memory_abstractions` row, so candidacy keyed on that table
    // alone re-selected (and re-amended) every encrypted row forever. The
    // "already compiled" marker is the frontmatter abstraction.
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("index.sqlite");
    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
    let connection = memory_substrate::index::open_index(&path).expect("index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, triple);

    let mut encrypted = sample_memory("mem_20260424_a1b2c3d4e5f60718_000094");
    encrypted.path = Some(RepoPath::new("encrypted/mem_20260424_a1b2c3d4e5f60718_000094.md"));
    encrypted.frontmatter.retrieval_policy.index_embeddings = false;
    index.upsert_memory(&encrypted, true).expect("encrypted active");
    assert_eq!(index.abstraction_compile_candidates(10).expect("candidates").len(), 1);

    // Amend lands abstraction in frontmatter; still no servable abstraction row.
    encrypted.frontmatter.abstraction = Some("durable encrypted fact".to_string());
    encrypted.frontmatter.cues = vec!["entity aspect".to_string()];
    index.upsert_memory(&encrypted, true).expect("encrypted amended");
    let servable: i64 = index
        .connection()
        .query_row(
            "SELECT count(*) FROM memory_abstractions WHERE memory_id=?1",
            [encrypted.frontmatter.id.as_str()],
            |row| row.get(0),
        )
        .expect("servable row count");
    assert_eq!(servable, 0, "encrypted rows must not gain a servable abstraction row");
    assert!(
        index.abstraction_compile_candidates(10).expect("candidates").is_empty(),
        "amended encrypted row must leave the candidate pool"
    );

    // Plaintext body drift still re-selects via the servable row's source hash.
    let mut plaintext = sample_memory("mem_20260424_a1b2c3d4e5f60718_000095");
    plaintext.frontmatter.abstraction = Some("compiled from old body".to_string());
    index.upsert_memory(&plaintext, false).expect("plaintext with abstraction");
    assert!(index.abstraction_compile_candidates(10).expect("candidates").is_empty());
    plaintext.body = "a different body that drifts the hash".to_string();
    // Simulate a body edit that preserves the previously compiled abstraction:
    // upsert keeps source_body_hash when the abstraction text is unchanged.
    index.upsert_memory(&plaintext, false).expect("plaintext body drift");
    assert_eq!(
        index.abstraction_compile_candidates(10).expect("candidates"),
        vec![plaintext.frontmatter.id.clone()],
        "body drift on a plaintext row must re-enter the candidate pool"
    );
}

#[test]
fn source_body_hash_preserved_on_body_only_edit_and_refreshed_on_remint() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("index.sqlite");
    let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
    let connection = memory_substrate::index::open_index(&path).expect("index");
    let mut index = memory_substrate::index::Index::with_active_embedding(connection, triple);

    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_000093");
    memory.body = "first body".to_string();
    memory.frontmatter.abstraction = Some("first abstraction".to_string());
    memory.frontmatter.cues = vec!["first cue".to_string()];
    index.upsert_memory(&memory, false).expect("mint");

    let first_body_hash = memory_substrate::markdown::hash_bytes(memory.body.as_bytes()).to_string();
    let source_hash = |index: &memory_substrate::index::Index, memory: &memory_substrate::Memory| -> String {
        let id = memory.frontmatter.id.as_str();
        index
            .connection()
            .query_row("SELECT source_body_hash FROM memory_abstractions WHERE memory_id=?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .expect("source_body_hash")
    };
    assert_eq!(source_hash(&index, &memory), first_body_hash);

    memory.body = "second body".to_string();
    memory.frontmatter.updated_at = chrono::Utc::now();
    index.upsert_memory(&memory, false).expect("body-only edit");
    let second_body_hash = memory_substrate::markdown::hash_bytes(memory.body.as_bytes()).to_string();
    assert_eq!(source_hash(&index, &memory), first_body_hash, "body-only edit must keep mint-time source_body_hash");

    memory.frontmatter.abstraction = Some("second abstraction".to_string());
    memory.frontmatter.updated_at = chrono::Utc::now();
    index.upsert_memory(&memory, false).expect("abstraction re-mint");
    assert_eq!(source_hash(&index, &memory), second_body_hash, "abstraction re-mint must refresh source_body_hash");
}
