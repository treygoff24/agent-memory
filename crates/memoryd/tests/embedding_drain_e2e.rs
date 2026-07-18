//! End-to-end: write → drain → vector present → KNN orders correctly.
//!
//! Exercises the real production write path (the
//! governance `WriteMemory` handler, which produces chunks and
//! `pending_embedding_jobs`), the real drain unit (`worker::drain_batch`), and
//! the real vector query path — all on the deterministic [`FixtureProvider`] so
//! it needs no model download and is stable in CI.

use std::sync::Arc;

use memory_substrate::{ChunkQuery, InitOptions, Roots, Substrate};
use memoryd::embedding::{worker, EmbeddingProvider, FixtureProvider};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

const TEST_PROJECT_CANONICAL_ID: &str = "proj_embedding_drain_e2e";
const TEST_PROJECT_ALIAS: &str = "embedding-drain-e2e";

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_embeddraine2e".to_string()) },
    )
    .await
    .expect("init substrate")
}

/// Write a project-scoped memory through the governed write path and return its
/// id. The body must be grounded enough to promote under the default policy.
async fn write_memory(substrate: &Substrate, summary: &str, body: &str) -> String {
    let response = handle_request(
        substrate,
        RequestEnvelope::new(
            "embed-e2e-write",
            RequestPayload::WriteMemory {
                body: body.to_string(),
                title: Some(summary.to_string()),
                tags: vec!["embed-e2e".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "project",
                    "summary": summary,
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
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
        ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => {
            write.id.expect("governed write returns a memory id")
        }
        other => panic!("expected governed write success, got {other:?}"),
    }
}

#[tokio::test]
async fn write_drain_vector_present_and_knn_orders_correctly() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;

    // The fresh substrate ships the production Qwen3 triple. The fixture
    // provider must produce vectors for that exact triple so the worker writes
    // into the active vector table.
    let triple = substrate.active_embedding_triple().expect("active triple");
    let provider: Arc<dyn EmbeddingProvider> = Arc::new(FixtureProvider::new(triple.clone()));

    // Two semantically distinct memories.
    let rust_id = write_memory(
        &substrate,
        "tokio async runtime choice",
        "We standardized on the tokio async runtime for all rust daemon concurrency in this project.",
    )
    .await;
    let _bread_id = write_memory(
        &substrate,
        "office snack policy",
        "The office snack policy stocks sourdough bread and fresh fruit every monday morning.",
    )
    .await;

    // No vectors yet — only chunks + pending jobs.
    assert_eq!(substrate.vector_count(triple.clone()).await.expect("count before"), 0, "no vectors before drain");

    let mut total = 0usize;
    loop {
        let n = worker::drain_batch(&substrate, &provider, 64).await.expect("drain");
        total += n;
        if n < 64 {
            break;
        }
    }
    assert!(total >= 2, "both memories produced at least one chunk job each, got {total}");

    let vectors = substrate.vector_count(triple.clone()).await.expect("count after");
    assert!(vectors >= 2, "expected vectors after drain, got {vectors}");

    let query_vector =
        provider.embed_query("which async runtime did we pick for the rust daemon").expect("embed query");
    let results = substrate
        .query_chunks(ChunkQuery { text: None, triple: Some(triple), vector: Some(query_vector) })
        .await
        .expect("vector query");

    assert!(!results.is_empty(), "vector KNN returned candidates");
    assert_eq!(
        results[0].memory_id.as_str(),
        rust_id,
        "the tokio/rust memory must rank first for a rust-runtime query, got {:?}",
        results.iter().map(|r| r.memory_id.as_str().to_string()).collect::<Vec<_>>()
    );
}
