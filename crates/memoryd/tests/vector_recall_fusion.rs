use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EmbeddingTriple, EventContext, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind,
    Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::embedding::{worker, EmbeddingError, EmbeddingProvider, FixtureProvider};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::{
    build_delta_response, build_delta_response_with_vector_recall, DeltaRequest, DeltaResponse, VectorRecallConfig,
    VectorRecallContext,
};

#[tokio::test]
async fn healthy_vector_delta_surfaces_paraphrase_that_fts_misses() {
    let fixture = TestRepo::new("dev_vecdeltahealthy").await;
    let provider = fixture.provider();
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000001_000001",
            "shipping production rollout",
            "The release team documented a shipping production rollout checklist.",
            true,
        ))
        .await;
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000002_000002",
            "kitchen snacks",
            "The kitchen snack shelf stocks apples and crackers.",
            true,
        ))
        .await;
    drain_all(&fixture.substrate, &provider).await;

    let fts_only =
        build_delta_response(&fixture.substrate, fixture.delta_request("deploy production")).await.expect("fts delta");
    assert_eq!(fts_only.delta_block, "<memory-delta empty=\"true\" />\n");

    let fused = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("deploy production"),
        VectorRecallContext::new(Some(Arc::clone(&provider)), VectorRecallConfig::default()),
    )
    .await
    .expect("fused delta");

    assert_eq!(fused.vector_recall_degraded, None);
    assert!(fused.delta_block.contains("mem_20260610_0000000000000001_000001"));
    assert!(fused.delta_block.contains("shipping production rollout checklist"));
}

#[tokio::test]
async fn memory_search_uses_hybrid_vector_path_without_wire_shape_change() {
    let fixture = TestRepo::new("dev_vecsearchhealthy").await;
    let provider = fixture.provider();
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000011_000011",
            "shipping production rollout",
            "The release team documented a shipping production rollout checklist.",
            true,
        ))
        .await;
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000012_000012",
            "kitchen snacks",
            "The kitchen snack shelf stocks apples and crackers.",
            true,
        ))
        .await;
    drain_all(&fixture.substrate, &provider).await;

    let empty_state = HandlerState::new();
    let fts_response = search(&fixture.substrate, &empty_state, "deploy production").await;
    assert!(fts_response.hits.is_empty(), "FTS-only search should miss the paraphrase fixture");

    let state = HandlerState::new();
    state.embedding_provider_slot().set(Arc::clone(&provider));
    let hybrid_response = search(&fixture.substrate, &state, "deploy production").await;

    assert!(!hybrid_response.hits.is_empty());
    assert_eq!(hybrid_response.hits[0].id, "mem_20260610_0000000000000011_000011");
    let json = serde_json::to_value(&hybrid_response).expect("search response serializes");
    assert!(json.get("vector_recall_degraded").is_none(), "search wire shape remains unchanged");
}

#[tokio::test]
async fn vector_degradation_rungs_mark_and_fallback_to_fts() {
    let fixture = TestRepo::new("dev_vecdegraderungs").await;
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000021_000021",
            "fallback exact keyword",
            "fallback exact keyword body",
            true,
        ))
        .await;
    let active_triple = fixture.substrate.active_embedding_triple().expect("active triple");

    assert_degrades_to_fts(&fixture, None, VectorRecallConfig::default(), "no_embedding_provider").await;

    let mismatched: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(EmbeddingTriple {
        provider: "synthetic".to_owned(),
        model_ref: "other-model".to_owned(),
        dimension: active_triple.dimension,
    }));
    assert_degrades_to_fts(&fixture, Some(mismatched), VectorRecallConfig::default(), "triple_mismatch").await;

    let failing: Arc<dyn EmbeddingProvider> = Arc::new(FailingProvider { triple: active_triple.clone() });
    assert_degrades_to_fts(&fixture, Some(failing), VectorRecallConfig::default(), "embedding_failed").await;

    let no_table: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(active_triple.clone()));
    assert_degrades_to_fts(&fixture, Some(no_table), VectorRecallConfig::default(), "no_vector_table").await;

    let wrong_dim: Arc<dyn EmbeddingProvider> = Arc::new(WrongDimensionProvider { triple: active_triple.clone() });
    assert_degrades_to_fts(&fixture, Some(wrong_dim), VectorRecallConfig::default(), "knn_failed").await;

    let slow: Arc<dyn EmbeddingProvider> = Arc::new(SlowProvider { triple: active_triple });
    let timeout_config = VectorRecallConfig { embed_timeout_ms: Some(1), ..VectorRecallConfig::default() };
    assert_degrades_to_fts(&fixture, Some(slow), timeout_config, "embedding_timeout").await;
}

/// F10: the degradation-rung suite must also cover the `embedding_dormant` rung
/// — a configured-but-dormant lifecycle slot (loader configured, no provider
/// loaded) degrades to FTS with the `embedding_dormant` marker, NOT
/// `no_embedding_provider` (which is for never-armed/failed). The existing
/// `None`→`no_embedding_provider` case above is correct for never-armed.
#[tokio::test]
async fn f10_dormant_lifecycle_slot_degrades_to_embedding_dormant_rung() {
    use memoryd::embedding::{EmbeddingIdleWindow, EmbeddingProviderSlot};
    use std::time::Duration;

    let fixture = TestRepo::new("dev_vecdormantrung").await;
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000061_000061",
            "fallback exact keyword",
            "fallback exact keyword body",
            true,
        ))
        .await;
    let triple = fixture.substrate.active_embedding_triple().expect("active triple");

    // Configure a loader on the slot but keep it dormant — don't call
    // ensure_loaded. A slow loader ensures the slot stays dormant during the
    // query rather than completing before the recall path reads it.
    let slot = EmbeddingProviderSlot::empty();
    let triple_for_loader = triple.clone();
    slot.configure_loader(
        triple,
        EmbeddingIdleWindow::from_duration(Some(Duration::from_secs(60)), "test"),
        move || {
            std::thread::sleep(Duration::from_millis(500));
            let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple_for_loader.clone()));
            Ok(provider)
        },
    );

    let response = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("fallback exact keyword"),
        VectorRecallContext::from_lifecycle(slot, VectorRecallConfig::default()),
    )
    .await
    .expect("dormant lifecycle delta");

    assert_eq!(response.vector_recall_degraded.as_deref(), Some("embedding_dormant"));
    assert!(response.delta_block.contains("mem_20260610_0000000000000061_000061"));
}

#[tokio::test]
async fn vector_recall_disabled_does_not_call_provider_or_mark_degraded() {
    let fixture = TestRepo::new("dev_vecdisabled").await;
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000031_000031",
            "fallback exact keyword",
            "fallback exact keyword body",
            true,
        ))
        .await;
    let provider = Arc::new(RecordingProvider::new(fixture.substrate.active_embedding_triple().expect("triple")));
    let provider_dyn: Arc<dyn EmbeddingProvider> = provider.clone();

    let response = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("fallback exact keyword"),
        VectorRecallContext::new(
            Some(provider_dyn),
            VectorRecallConfig { enabled: false, ..VectorRecallConfig::default() },
        ),
    )
    .await
    .expect("delta");

    assert_eq!(response.vector_recall_degraded, None);
    assert!(response.delta_block.contains("mem_20260610_0000000000000031_000031"));
    assert_eq!(provider.query_calls.load(Ordering::SeqCst), 0, "disabled vector recall must not embed");
}

#[tokio::test]
async fn partial_vector_coverage_fuses_one_lane_candidates() {
    let fixture = TestRepo::new("dev_vecpartial").await;
    let provider = fixture.provider();
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000041_000041",
            "shipping production rollout",
            "The release team documented a shipping production rollout checklist.",
            true,
        ))
        .await;
    drain_all(&fixture.substrate, &provider).await;
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000042_000042",
            "coverage unique keyword",
            "coverage unique keyword appears only in this unembedded memory.",
            false,
        ))
        .await;

    let response = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("coverage unique keyword"),
        VectorRecallContext::new(Some(Arc::clone(&provider)), VectorRecallConfig::default()),
    )
    .await
    .expect("partial coverage delta");

    assert_eq!(response.vector_recall_degraded, None);
    assert!(response.delta_block.contains("mem_20260610_0000000000000041_000041"));
    assert!(response.delta_block.contains("mem_20260610_0000000000000042_000042"));
}

#[tokio::test]
async fn delta_block_is_byte_stable_with_fixed_fixture_vectors() {
    let fixture = TestRepo::new("dev_vecstable").await;
    let provider = fixture.provider();
    fixture
        .write_memory(memory_spec(
            "mem_20260610_0000000000000051_000051",
            "shipping production rollout",
            "The release team documented a shipping production rollout checklist.",
            true,
        ))
        .await;
    drain_all(&fixture.substrate, &provider).await;
    let context = || VectorRecallContext::new(Some(Arc::clone(&provider)), VectorRecallConfig::default());

    let first = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("deploy production"),
        context(),
    )
    .await
    .expect("first");
    let second = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("deploy production"),
        context(),
    )
    .await
    .expect("second");

    assert_eq!(first.vector_recall_degraded, None);
    assert_eq!(first.delta_block, second.delta_block);
}

#[test]
fn delta_vector_recall_degraded_field_is_additive_and_skipped_when_none() {
    let healthy = DeltaResponse {
        delta_block: "<memory-delta empty=\"true\" />\n".to_owned(),
        budget_used_tokens: 0,
        guidance: "No passive recall delta matched this turn.".to_owned(),
        vector_recall_degraded: None,
    };
    let json = serde_json::to_value(&healthy).expect("serialize healthy");
    assert!(json.get("vector_recall_degraded").is_none());

    let old_payload = r#"{
        "delta_block":"<memory-delta empty=\"true\" />\n",
        "budget_used_tokens":0,
        "guidance":"No passive recall delta matched this turn."
    }"#;
    let decoded: DeltaResponse = serde_json::from_str(old_payload).expect("old payload deserializes");
    assert_eq!(decoded.vector_recall_degraded, None);

    let degraded = DeltaResponse { vector_recall_degraded: Some("knn_failed".to_owned()), ..healthy };
    let json = serde_json::to_value(&degraded).expect("serialize degraded");
    assert_eq!(json.get("vector_recall_degraded").and_then(|value| value.as_str()), Some("knn_failed"));
}

async fn assert_degrades_to_fts(
    fixture: &TestRepo,
    provider: Option<Arc<dyn EmbeddingProvider>>,
    config: VectorRecallConfig,
    marker: &str,
) {
    let response = build_delta_response_with_vector_recall(
        &fixture.substrate,
        fixture.delta_request("fallback exact keyword"),
        VectorRecallContext::new(provider, config),
    )
    .await
    .expect("degraded delta still succeeds");

    assert_eq!(response.vector_recall_degraded.as_deref(), Some(marker));
    assert!(response.delta_block.contains("mem_20260610_0000000000000021_000021"));
}

async fn search(substrate: &Substrate, state: &HandlerState, query: &str) -> memoryd::protocol::SearchResponse {
    let response = handle_request_with_state(
        substrate,
        state,
        RequestEnvelope::new(
            "search",
            RequestPayload::Search { query: query.to_owned(), limit: Some(5), include_body: false },
        ),
    )
    .await;
    match response.result {
        ResponseResult::Success(ResponsePayload::Search(search)) => search,
        other => panic!("expected search success, got {other:?}"),
    }
}

async fn drain_all(substrate: &Substrate, provider: &Arc<dyn EmbeddingProvider>) {
    loop {
        let drained = worker::drain_batch(substrate, provider, 64).await.expect("drain");
        if drained < 64 {
            break;
        }
    }
}

struct TestRepo {
    _temp: tempfile::TempDir,
    repo: std::path::PathBuf,
    substrate: Substrate,
}

struct TestMemorySpec<'a> {
    id: &'a str,
    summary: &'a str,
    body: &'a str,
    index_embeddings: bool,
}

fn memory_spec<'a>(id: &'a str, summary: &'a str, body: &'a str, index_embeddings: bool) -> TestMemorySpec<'a> {
    TestMemorySpec { id, summary, body, index_embeddings }
}

impl TestRepo {
    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, repo, substrate }
    }

    fn provider(&self) -> Arc<dyn EmbeddingProvider> {
        Arc::new(FixtureProvider::new(self.substrate.active_embedding_triple().expect("active triple")))
    }

    fn delta_request(&self, message: &str) -> DeltaRequest {
        DeltaRequest {
            cwd: self.repo.to_string_lossy().into_owned(),
            session_id: "sess_vector_recall".to_owned(),
            harness: "codex".to_owned(),
            message: message.to_owned(),
            budget_tokens: Some(8_000),
            passive: false,
        }
    }

    async fn write_memory(&self, spec: TestMemorySpec<'_>) {
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: test_memory(spec),
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
}

fn test_memory(spec: TestMemorySpec<'_>) -> Memory {
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(spec.id),
            memory_type: MemoryType::Project,
            scope: Scope::User,
            summary: spec.summary.to_owned(),
            confidence: 0.9,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: instant("2026-06-10T12:00:00Z"),
            updated_at: instant("2026-06-10T12:00:00Z"),
            observed_at: None,
            author: Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_vector_recall".to_owned()),
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_vector_recall".to_owned()),
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
                max_scope: Scope::User,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: spec.index_embeddings,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "vector-recall-fusion-test".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: std::collections::BTreeMap::new(),
        },
        body: spec.body.to_owned(),
        path: Some(RepoPath::new(format!("me/{}.md", spec.id))),
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}

struct FailingProvider {
    triple: EmbeddingTriple,
}

impl EmbeddingProvider for FailingProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Err(EmbeddingError::Inference("intentional query failure".to_owned()))
    }

    fn embed_document(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Err(EmbeddingError::Inference("intentional document failure".to_owned()))
    }
}

struct WrongDimensionProvider {
    triple: EmbeddingTriple,
}

impl EmbeddingProvider for WrongDimensionProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![0.0; self.triple.dimension as usize + 1])
    }

    fn embed_document(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![0.0; self.triple.dimension as usize + 1])
    }
}

struct SlowProvider {
    triple: EmbeddingTriple,
}

impl EmbeddingProvider for SlowProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        std::thread::sleep(Duration::from_millis(50));
        Ok(vec![0.0; self.triple.dimension as usize])
    }

    fn embed_document(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![0.0; self.triple.dimension as usize])
    }
}

struct RecordingProvider {
    inner: FixtureProvider,
    query_calls: AtomicUsize,
}

impl RecordingProvider {
    fn new(triple: EmbeddingTriple) -> Self {
        Self { inner: FixtureProvider::new(triple), query_calls: AtomicUsize::new(0) }
    }
}

impl EmbeddingProvider for RecordingProvider {
    fn triple(&self) -> &EmbeddingTriple {
        self.inner.triple()
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.query_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.embed_query(text)
    }

    fn embed_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.inner.embed_document(text)
    }
}
