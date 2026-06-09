use memory_substrate::*;

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
        .query_chunks(ChunkQuery { text: None, triple: Some(triple.clone()), vector: Some(vec![1.0, 0.0, 0.0]) })
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
    let active =
        EmbeddingTriple { provider: "synthetic".to_string(), model_ref: "stream-a-test".to_string(), dimension: 32 };

    assert_eq!(memory_substrate::index::reconcile_pending_jobs(&connection, &active).expect("pending jobs"), 1);
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
        .query_chunks(ChunkQuery { text: None, triple: Some(triple), vector: Some(vec![1.0, 0.0, 0.0]) })
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
            extras: std::collections::BTreeMap::new(),
        },
        body: "vector body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn first_index_chunk(memory: &Memory) -> memory_substrate::index::Chunk {
    memory_substrate::index::chunk_memory(memory).into_iter().next().expect("memory has one chunk")
}
