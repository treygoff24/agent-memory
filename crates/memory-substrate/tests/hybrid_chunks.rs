use memory_substrate::*;
use rusqlite::Connection;

#[tokio::test]
async fn hybrid_chunks_filters_recall_membership_in_both_lanes() {
    let (_temp, roots, substrate) = new_substrate().await;
    let triple = test_triple("membership");

    let mut allowed = sample_memory("mem_20260424_a1b2c3d4e5f60718_010000");
    allowed.body = "filterneedle allowed current memory".to_string();

    let mut metadata_only = sample_memory("mem_20260424_a1b2c3d4e5f60718_010001");
    metadata_only.body = "filterneedle metadata-only stale chunk".to_string();

    let mut passive_disabled = sample_memory("mem_20260424_a1b2c3d4e5f60718_010002");
    passive_disabled.body = "filterneedle passive-disabled stale chunk".to_string();
    passive_disabled.frontmatter.retrieval_policy.passive_recall = false;

    let mut superseded = sample_memory("mem_20260424_a1b2c3d4e5f60718_010003");
    superseded.body = "filterneedle superseded stale chunk".to_string();

    let mut tombstoned = sample_memory("mem_20260424_a1b2c3d4e5f60718_010004");
    tombstoned.body = "filterneedle tombstoned stale chunk".to_string();

    for memory in [&allowed, &metadata_only, &passive_disabled, &superseded, &tombstoned] {
        write_memory(&substrate, memory.clone()).await;
        embed_first_chunk(&substrate, memory, &triple, vec![1.0, 0.0, 0.0]).await;
    }

    set_metadata_only(&roots, &metadata_only.frontmatter.id);
    set_status(&roots, &superseded.frontmatter.id, "superseded");
    set_status(&roots, &tombstoned.frontmatter.id, "tombstoned");

    let hits = substrate
        .query_hybrid_chunks(
            "filterneedle",
            Some(HybridVectorQuery { triple: &triple, vector: &[1.0, 0.0, 0.0] }),
            10,
            None,
        )
        .await
        .expect("hybrid query");

    assert_eq!(ids(&hits), vec![allowed.frontmatter.id.as_str().to_string()]);
    assert_eq!(hits[0].score_breakdown.bm25_rank, Some(1));
    assert_eq!(hits[0].score_breakdown.cosine_similarity, Some(1.0));
}

#[tokio::test]
async fn hybrid_chunks_collapses_chunks_to_best_memory_candidate() {
    let (_temp, _roots, substrate) = new_substrate().await;
    let triple = test_triple("collapse");

    let mut multi_chunk = sample_memory("mem_20260424_a1b2c3d4e5f60718_010010");
    multi_chunk.body = format!("collapseneedle {} collapseneedle", "filler ".repeat(700));
    let multi_chunks = memory_substrate::index::chunk_memory(&multi_chunk);
    assert!(multi_chunks.len() >= 2, "fixture must exercise duplicate chunks");

    let mut medium = sample_memory("mem_20260424_a1b2c3d4e5f60718_010011");
    medium.body = "collapseneedle medium memory".to_string();

    write_memory(&substrate, multi_chunk.clone()).await;
    write_memory(&substrate, medium.clone()).await;

    embed_chunks(&substrate, &multi_chunk, &triple, vec![vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]]).await;
    embed_first_chunk(&substrate, &medium, &triple, vec![0.8, 0.6, 0.0]).await;

    let hits = substrate
        .query_hybrid_chunks(
            "collapseneedle",
            Some(HybridVectorQuery { triple: &triple, vector: &[1.0, 0.0, 0.0] }),
            10,
            None,
        )
        .await
        .expect("hybrid query");

    let hit_ids = ids(&hits);
    assert_eq!(hit_ids.len(), 2, "one candidate per memory");
    assert_eq!(
        hit_ids.iter().filter(|id| **id == multi_chunk.frontmatter.id.as_str()).count(),
        1,
        "duplicate chunk hits collapse to one memory candidate"
    );

    let multi = find_hit(&hits, &multi_chunk.frontmatter.id);
    let medium = find_hit(&hits, &medium.frontmatter.id);
    assert!(multi.score_breakdown.bm25_rank.is_some());
    assert!(
        multi.score_breakdown.cosine_similarity > medium.score_breakdown.cosine_similarity,
        "vector collapse must keep the minimum L2 distance per memory"
    );
}

#[tokio::test]
async fn hybrid_chunks_return_representative_chunk_text() {
    let (_temp, _roots, substrate) = new_substrate().await;
    let triple = test_triple("chunk-text");

    let mut bm25_and_vector = sample_memory("mem_20260424_a1b2c3d4e5f60718_010015");
    bm25_and_vector.body = body_with_marker_at_word("b", "bestbm25needle desired-bm25-chunk", 450, 560);
    let bm25_chunks = memory_substrate::index::chunk_memory(&bm25_and_vector);
    let expected_bm25_text = bm25_chunks
        .iter()
        .find(|chunk| chunk.text.contains("desired-bm25-chunk"))
        .expect("fixture marker is in one chunk")
        .text
        .clone();

    let mut vector_only = sample_memory("mem_20260424_a1b2c3d4e5f60718_010016");
    vector_only.body = body_with_marker_at_word("v", "nearest-vector-chunk", 450, 560);
    let vector_chunks = memory_substrate::index::chunk_memory(&vector_only);
    let expected_vector_text = vector_chunks
        .iter()
        .find(|chunk| chunk.text.contains("nearest-vector-chunk"))
        .expect("fixture marker is in one chunk")
        .text
        .clone();

    write_memory(&substrate, bm25_and_vector.clone()).await;
    write_memory(&substrate, vector_only.clone()).await;

    embed_chunks(&substrate, &bm25_and_vector, &triple, vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]]).await;
    embed_chunks(&substrate, &vector_only, &triple, vec![vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]]).await;

    let hits = substrate
        .query_hybrid_chunks(
            "bestbm25needle",
            Some(HybridVectorQuery { triple: &triple, vector: &[1.0, 0.0, 0.0] }),
            10,
            None,
        )
        .await
        .expect("hybrid query");

    let bm25_hit = find_hit(&hits, &bm25_and_vector.frontmatter.id);
    assert!(bm25_hit.score_breakdown.bm25_rank.is_some());
    assert!(bm25_hit.score_breakdown.cosine_similarity.is_some());
    assert_eq!(bm25_hit.text, expected_bm25_text, "BM25 hits carry the best matching chunk text");

    let vector_hit = find_hit(&hits, &vector_only.frontmatter.id);
    assert_eq!(vector_hit.score_breakdown.bm25_rank, None);
    assert!(vector_hit.score_breakdown.cosine_similarity.is_some());
    assert_eq!(vector_hit.text, expected_vector_text, "vector-only hits carry the nearest chunk text");
}

#[tokio::test]
async fn hybrid_chunks_tolerates_partial_vector_coverage_in_both_directions() {
    let (_temp, _roots, substrate) = new_substrate().await;
    let triple = test_triple("partial");

    let mut both = sample_memory("mem_20260424_a1b2c3d4e5f60718_010020");
    both.body = "partialneedle both lanes".to_string();

    let mut bm25_only = sample_memory("mem_20260424_a1b2c3d4e5f60718_010021");
    bm25_only.body = "partialneedle bm25 only".to_string();

    let mut vector_only = sample_memory("mem_20260424_a1b2c3d4e5f60718_010022");
    vector_only.body = "semantic vector-only memory".to_string();

    for memory in [&both, &bm25_only, &vector_only] {
        write_memory(&substrate, memory.clone()).await;
    }
    embed_first_chunk(&substrate, &both, &triple, vec![1.0, 0.0, 0.0]).await;
    embed_first_chunk(&substrate, &vector_only, &triple, vec![0.0, 1.0, 0.0]).await;

    let hits = substrate
        .query_hybrid_chunks(
            "partialneedle",
            Some(HybridVectorQuery { triple: &triple, vector: &[1.0, 0.0, 0.0] }),
            10,
            None,
        )
        .await
        .expect("hybrid query");

    let both_hit = find_hit(&hits, &both.frontmatter.id);
    assert!(both_hit.score_breakdown.bm25_rank.is_some());
    assert!(both_hit.score_breakdown.cosine_similarity.is_some());

    let bm25_hit = find_hit(&hits, &bm25_only.frontmatter.id);
    assert!(bm25_hit.score_breakdown.bm25_rank.is_some());
    assert_eq!(bm25_hit.score_breakdown.cosine_similarity, None);

    let vector_hit = find_hit(&hits, &vector_only.frontmatter.id);
    assert_eq!(vector_hit.score_breakdown.bm25_rank, None);
    assert!(vector_hit.score_breakdown.cosine_similarity.is_some());
}

#[tokio::test]
async fn hybrid_chunks_unknown_triple_is_typed_error() {
    let (_temp, _roots, substrate) = new_substrate().await;
    let unknown = test_triple("absent");

    let err = substrate
        .query_hybrid_chunks(
            "anything",
            Some(HybridVectorQuery { triple: &unknown, vector: &[1.0, 0.0, 0.0] }),
            10,
            None,
        )
        .await
        .expect_err("unknown triple must not silently return empty results");

    assert!(matches!(err, VectorError::UnknownEmbeddingTriple(triple) if triple == unknown));
}

#[tokio::test]
async fn hybrid_chunks_are_deterministic_and_tie_break_by_memory_id() {
    let (_temp, _roots, substrate) = new_substrate().await;
    let triple = test_triple("determinism");

    let later_id = MemoryId::new("mem_20260424_a1b2c3d4e5f60718_010032");
    let earlier_id = MemoryId::new("mem_20260424_a1b2c3d4e5f60718_010031");
    let mut later = sample_memory(later_id.as_str());
    later.body = "semantic tie later".to_string();
    let mut earlier = sample_memory(earlier_id.as_str());
    earlier.body = "semantic tie earlier".to_string();

    for memory in [&later, &earlier] {
        write_memory(&substrate, memory.clone()).await;
        embed_first_chunk(&substrate, memory, &triple, vec![0.5, 0.5, 0.70710677]).await;
    }

    let mut observed = Vec::new();
    for _ in 0..5 {
        let hits = substrate
            .query_hybrid_chunks(
                "nonmatchingneedle",
                Some(HybridVectorQuery { triple: &triple, vector: &[0.5, 0.5, 0.70710677] }),
                10,
                None,
            )
            .await
            .expect("hybrid query");
        observed.push(ids(&hits));
    }

    assert!(observed.windows(2).all(|pair| pair[0] == pair[1]), "fixed inputs must produce identical ordering");
    assert_eq!(observed[0], vec![earlier_id.as_str().to_string(), later_id.as_str().to_string()]);
}

#[tokio::test]
async fn hybrid_chunks_namespace_scoping_filters_both_lanes() {
    let (_temp, _roots, substrate) = new_substrate().await;
    let triple = test_triple("namespace");

    let mut me_memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_050000");
    me_memory.body = "scopeneedle me-visible fact".to_string();
    me_memory.frontmatter.scope = Scope::User;

    let mut agent_memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_050001");
    agent_memory.body = "scopeneedle agent-visible fact".to_string();

    let mut project_memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_050002");
    project_memory.body = "scopeneedle project-visible fact".to_string();
    project_memory.frontmatter.scope = Scope::Project;
    project_memory.frontmatter.namespace = Some("project".to_string());
    project_memory.frontmatter.canonical_namespace_id = Some("proj_alpha".to_string());
    project_memory.frontmatter.retrieval_policy.max_scope = Scope::Project;

    for memory in [&me_memory, &agent_memory, &project_memory] {
        write_memory(&substrate, memory.clone()).await;
        embed_first_chunk(&substrate, memory, &triple, vec![1.0, 0.0, 0.0]).await;
    }

    let query = |namespaces: Option<Vec<String>>| {
        let substrate = &substrate;
        let triple = &triple;
        async move {
            let hits = substrate
                .query_hybrid_chunks(
                    "scopeneedle",
                    Some(HybridVectorQuery { triple, vector: &[1.0, 0.0, 0.0] }),
                    10,
                    namespaces.as_deref(),
                )
                .await
                .expect("hybrid query");
            let mut hit_ids = ids(&hits);
            hit_ids.sort();
            hit_ids
        }
    };

    let me_id = me_memory.frontmatter.id.as_str().to_string();
    let agent_id = agent_memory.frontmatter.id.as_str().to_string();
    let project_id = project_memory.frontmatter.id.as_str().to_string();

    // Unscoped keeps the store-wide behavior.
    assert_eq!(query(None).await, vec![me_id.clone(), agent_id.clone(), project_id.clone()]);

    // The no-project session set (the leak regression): a project memory must
    // not surface from a me+agent-only session even as the best lexical and
    // vector match.
    assert_eq!(query(Some(vec!["me".to_string(), "agent".to_string()])).await, vec![me_id.clone(), agent_id.clone()]);

    // Project-scoped sessions see exactly their project.
    assert_eq!(query(Some(vec!["project:proj_alpha".to_string()])).await, vec![project_id]);
    assert!(query(Some(vec!["project:proj_other".to_string()])).await.is_empty());

    // Fail-closed: an empty or all-invalid set serves nothing rather than
    // falling open to every namespace.
    assert!(query(Some(Vec::new())).await.is_empty());
    assert!(query(Some(vec!["bogus".to_string()])).await.is_empty());

    // The plain FTS surface (`query_chunks`) applies the same predicate.
    let fts_hits = substrate
        .query_chunks(ChunkQuery {
            text: Some("scopeneedle".to_string()),
            triple: None,
            vector: None,
            namespaces: Some(vec!["me".to_string()]),
        })
        .await
        .expect("fts query");
    assert_eq!(
        fts_hits.iter().map(|hit| hit.memory_id.as_str().to_string()).collect::<Vec<_>>(),
        vec![me_id]
    );
}

async fn new_substrate() -> (tempfile::TempDir, Roots, Substrate) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_hybrid".to_string()) },
    )
    .await
    .expect("init");
    (temp, roots, substrate)
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
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
        .expect("write memory");
}

async fn embed_first_chunk(substrate: &Substrate, memory: &Memory, triple: &EmbeddingTriple, vector: Vec<f32>) {
    embed_chunks(substrate, memory, triple, vec![vector]).await;
}

async fn embed_chunks(substrate: &Substrate, memory: &Memory, triple: &EmbeddingTriple, vectors: Vec<Vec<f32>>) {
    let chunks = memory_substrate::index::chunk_memory(memory);
    assert!(chunks.len() >= vectors.len(), "fixture provided more vectors than chunks");
    for (chunk, vector) in chunks.into_iter().zip(vectors) {
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
}

fn set_metadata_only(roots: &Roots, id: &MemoryId) {
    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open index");
    conn.execute("UPDATE memories SET metadata_only = 1 WHERE id = ?1", [id.as_str()]).expect("set metadata_only");
}

fn set_status(roots: &Roots, id: &MemoryId, status: &str) {
    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open index");
    conn.execute("UPDATE memories SET status = ?1 WHERE id = ?2", [status, id.as_str()]).expect("set status");
}

fn ids(hits: &[HybridMemoryCandidate]) -> Vec<String> {
    hits.iter().map(|hit| hit.memory_id.as_str().to_string()).collect()
}

fn find_hit<'a>(hits: &'a [HybridMemoryCandidate], id: &MemoryId) -> &'a HybridMemoryCandidate {
    hits.iter().find(|hit| hit.memory_id == *id).expect("candidate present")
}

fn test_triple(model_ref: &str) -> EmbeddingTriple {
    EmbeddingTriple { provider: "synthetic".to_string(), model_ref: model_ref.to_string(), dimension: 3 }
}

fn body_with_marker_at_word(seed: &str, marker: &str, marker_index: usize, total_words: usize) -> String {
    (0..total_words)
        .map(|index| if index == marker_index { marker.to_string() } else { format!("{seed}{index}") })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "hybrid".to_string(),
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
        body: "hybrid body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
