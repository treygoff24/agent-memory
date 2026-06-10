//! End-to-end: production embedding-backed contradiction detection (Task 3.2).
//!
//! Writes memory A, drains its vector into the active triple's vec table, then
//! writes a semantically-similar *contradicting* memory B through the same
//! governed write path. With a real top-K similarity backend wired, B's
//! candidate text is embedded (query side) and KNN-matched against A's vector;
//! the above-threshold hit drives the contradiction pipeline, so B is
//! quarantined for review rather than promoted as a peer active memory.
//!
//! Determinism: the whole test runs on the [`FixtureProvider`] (content-derived
//! hashed bag-of-words vectors), shared through the same [`HandlerState`] slot
//! the daemon would publish the real model into — no model download, stable in
//! CI. The fixture's one guaranteed property is that higher token overlap →
//! higher cosine similarity, which is exactly what an above-threshold hit needs.

use std::sync::Arc;

use memory_substrate::{InitOptions, Roots, Scope, Substrate};
use memoryd::embedding::EmbeddingError;
use memoryd::embedding::{worker, EmbeddingProvider, FixtureProvider};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_contradicte2e".to_string()) },
    )
    .await
    .expect("init substrate")
}

/// Write a project-scoped memory through the governed write path with the
/// embedding provider published into `state`, returning the full write response.
async fn write_memory(
    substrate: &Substrate,
    state: &HandlerState,
    summary: &str,
    body: &str,
) -> memoryd::protocol::GovernanceWriteResponse {
    let response = handle_request_with_state(
        substrate,
        state,
        RequestEnvelope::new(
            "contradict-e2e-write",
            RequestPayload::WriteMemory {
                body: body.to_string(),
                title: Some(summary.to_string()),
                tags: vec!["contradict-e2e".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": summary,
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "user",
                    "explicit_user_context": true
                }),
            },
        ),
    )
    .await;

    match response.result {
        ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write,
        other => panic!("expected governed write success, got {other:?}"),
    }
}

async fn drain_all(substrate: &Substrate, provider: &Arc<dyn EmbeddingProvider>) {
    loop {
        let n = worker::drain_batch(substrate, provider, 64).await.expect("drain");
        if n < 64 {
            break;
        }
    }
}

struct RecordingProvider {
    inner: FixtureProvider,
    calls: std::sync::Mutex<Vec<&'static str>>,
}

impl RecordingProvider {
    fn new(triple: memory_substrate::EmbeddingTriple) -> Self {
        Self { inner: FixtureProvider::new(triple), calls: std::sync::Mutex::new(Vec::new()) }
    }

    fn take_calls(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.calls.lock().expect("calls lock"))
    }
}

impl EmbeddingProvider for RecordingProvider {
    fn triple(&self) -> &memory_substrate::EmbeddingTriple {
        self.inner.triple()
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.calls.lock().expect("calls lock").push("query");
        self.inner.embed_query(text)
    }

    fn embed_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.calls.lock().expect("calls lock").push("document");
        self.inner.embed_document(text)
    }
}

#[tokio::test]
async fn similar_contradicting_write_is_quarantined_by_contradiction_detection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let triple = substrate.active_embedding_triple().expect("active triple");

    // Publish the fixture provider into the handler state's slot, exactly as the
    // daemon publishes the loaded model — this is what the governed write path
    // reads to embed the contradiction candidate.
    let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple.clone()));
    let state = HandlerState::new();
    state.embedding_provider_slot().set(Arc::clone(&provider));

    // Memory A: a concrete claim. Heavy token overlap with B below so the
    // fixture's bag-of-words vectors land above the 0.82 cosine threshold.
    let a = write_memory(
        &substrate,
        &state,
        "billing service production database engine",
        "The production database engine for the billing service in this project is PostgreSQL version 14.",
    )
    .await;
    assert_eq!(a.status, GovernanceStatus::Promoted, "first write promotes (no prior conflict)");
    let a_id = a.id.clone().expect("A promoted with an id");

    // Drain so A's chunk vector lands in the active triple's vec table; without
    // it KNN has nothing to match and the contradiction can't be detected.
    drain_all(&substrate, &provider).await;
    assert!(substrate.vector_count(triple.clone()).await.expect("count") >= 1, "A's vector present after drain");

    // Memory B: same subject, contradicting value. Shares nearly every token
    // with A so cosine similarity clears the threshold.
    let b = write_memory(
        &substrate,
        &state,
        "billing service production database engine",
        "The production database engine for the billing service in this project is MySQL version 8.",
    )
    .await;

    // Contradiction detection fired: with the write-path tiebreaker (`Unclear`)
    // an above-threshold similarity hit routes the candidate to quarantine for
    // review instead of promoting it as a second active answer.
    assert_eq!(
        b.status,
        GovernanceStatus::Quarantined,
        "a semantically-similar contradicting write must be quarantined, not promoted (got next_actions {:?})",
        b.next_actions,
    );
    assert!(
        b.next_actions.iter().any(|action| action.contains("contradiction")),
        "quarantine reason should name the contradiction, got {:?}",
        b.next_actions,
    );
    // The degradation marker must NOT appear: the embedding backend was live.
    assert_eq!(
        b.similarity_degraded, None,
        "embedding backend was live, so no degradation marker expected, got {:?}",
        b.similarity_degraded,
    );
    assert_ne!(b.id.as_deref(), Some(a_id.as_str()), "B is its own quarantined record, not a merge into A");
}

/// Write a four-scope `policies/` dir into the substrate repo, with the
/// `project-standard` policy carrying the given `contradiction.similarity_threshold`.
/// All other policies omit the block (default behavior).
fn seed_policies_with_project_threshold(repo: &std::path::Path, similarity_threshold: &str) {
    let dir = repo.join("policies");
    std::fs::create_dir_all(&dir).expect("policy dir");
    let files = [
        ("me-strict.yaml", "me-strict", 1, "me", "0.85", "refuse", "quarantine", String::new()),
        (
            "project-standard.yaml",
            "project-standard",
            2,
            "project",
            "0.7",
            "review",
            "supersede",
            format!("contradiction:\n  similarity_threshold: {similarity_threshold}\n"),
        ),
        ("agent-strict.yaml", "agent-strict", 3, "agent", "0.82", "refuse", "quarantine", String::new()),
        ("dreaming-strict.yaml", "dreaming-strict", 1, "dreaming", "0.95", "refuse", "quarantine", String::new()),
    ];
    for (file, name, version, scope, floor, tombstone, contradiction, block) in files {
        let yaml = format!(
            "name: {name}\nversion: {version}\nscope: {scope}\nconfidence_floor: {floor}\nrequires_grounding: true\ntombstone_enforcement: {tombstone}\ncontradiction_policy: {contradiction}\nreview_gates: []\n{block}"
        );
        std::fs::write(dir.join(file), yaml).expect("write policy");
    }
}

#[tokio::test]
async fn raising_policy_similarity_threshold_promotes_a_write_that_would_otherwise_quarantine() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let triple = substrate.active_embedding_triple().expect("active triple");

    // A near-impossible similarity threshold (0.999): the fixture's bag-of-words
    // cosine for two distinct sentences cannot clear it, so contradiction
    // detection never fires for B even though it shares heavy token overlap with
    // A — the same pair that gets quarantined under the default 0.82 threshold in
    // the sibling test. This proves the YAML threshold changes the decision.
    seed_policies_with_project_threshold(substrate.roots().repo.as_path(), "0.999");

    let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple.clone()));
    let state = HandlerState::new();
    state.embedding_provider_slot().set(Arc::clone(&provider));

    let a = write_memory(
        &substrate,
        &state,
        "billing service production database engine",
        "The production database engine for the billing service in this project is PostgreSQL version 14.",
    )
    .await;
    assert_eq!(a.status, GovernanceStatus::Promoted, "first write promotes");

    drain_all(&substrate, &provider).await;
    assert!(substrate.vector_count(triple.clone()).await.expect("count") >= 1, "A's vector present after drain");

    let b = write_memory(
        &substrate,
        &state,
        "billing service production database engine",
        "The production database engine for the billing service in this project is MySQL version 8.",
    )
    .await;

    // With the threshold raised so high that no hit clears it, B is promoted as
    // its own active memory rather than quarantined for contradiction. The
    // embedding backend was live, so there is no degradation marker either.
    assert_eq!(
        b.status,
        GovernanceStatus::Promoted,
        "raising the policy similarity threshold past any real similarity must let B promote (got next_actions {:?})",
        b.next_actions,
    );
    assert_eq!(
        b.similarity_degraded, None,
        "backend was live; no degradation expected, got {:?}",
        b.similarity_degraded
    );
}

#[tokio::test]
async fn write_without_embedding_provider_records_visible_degradation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    // No provider published into the slot: contradiction detection has no
    // embedding backend. The write still completes (degradation is not a
    // refusal), but the decision trace must carry the visible marker so the
    // operator knows similarity was never actually checked (invariant 3).
    let state = HandlerState::new();
    let response = write_memory(
        &substrate,
        &state,
        "some grounded project claim",
        "This project ships its release artifacts through the internal CI pipeline every friday.",
    )
    .await;

    assert_eq!(response.status, GovernanceStatus::Promoted, "degradation does not block the write");
    assert_eq!(
        response.similarity_degraded.as_deref(),
        Some("similarity_degraded:no_embedding_provider"),
        "missing embedding provider must surface as a visible decision-trace marker, got {:?}",
        response.similarity_degraded,
    );
}

#[tokio::test]
async fn fixture_asymmetry_catches_index_and_query_call_site_swaps() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let triple = substrate.active_embedding_triple().expect("active triple");

    let recording = Arc::new(RecordingProvider::new(triple.clone()));
    let provider: Arc<dyn EmbeddingProvider> = recording.clone();
    let state = HandlerState::new();
    state.embedding_provider_slot().set(Arc::clone(&provider));

    let body = "The project API gateway uses Envoy for edge routing.";
    let first = write_memory(&substrate, &state, "project API gateway edge routing", body).await;
    assert_eq!(first.status, GovernanceStatus::Promoted);
    recording.take_calls(); // First write attempted query-side similarity before vectors existed.

    let pending = substrate.pending_embedding_jobs(1).await.expect("pending jobs");
    let chunk_text = pending.first().expect("one pending chunk").text.clone();
    drain_all(&substrate, &provider).await;
    assert!(
        recording.take_calls().iter().all(|call| *call == "document"),
        "worker drain must use document-flavored embeddings"
    );

    let document_vector = recording.embed_document(&chunk_text).expect("document vector");
    let query_vector = recording.embed_query(&chunk_text).expect("query vector");
    recording.take_calls();
    let scopes = [Scope::Project, Scope::Org];
    let document_hit = substrate
        .knn_active_memories(&triple, &document_vector, &scopes, 1)
        .await
        .expect("document knn")
        .pop()
        .expect("document hit");
    let query_hit = substrate
        .knn_active_memories(&triple, &query_vector, &scopes, 1)
        .await
        .expect("query knn")
        .pop()
        .expect("query hit");
    assert!(
        document_hit.similarity > query_hit.similarity + 0.01,
        "stored vector should be document-flavored; doc similarity {} vs query similarity {}",
        document_hit.similarity,
        query_hit.similarity
    );

    let second = write_memory(
        &substrate,
        &state,
        "project API gateway edge routing",
        "The project API gateway uses NGINX for edge routing.",
    )
    .await;
    assert!(
        recording.take_calls().contains(&"query"),
        "governance candidate similarity must use query-flavored embeddings"
    );
    assert_ne!(second.similarity_degraded.as_deref(), Some("similarity_degraded:no_embedding_provider"));
}
