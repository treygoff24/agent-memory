//! Real-model smoke test for the production embedding lane.
//!
//! `#[ignore]` by design: it loads Qwen3-Embedding-0.6B (~1.1 GB) from the
//! Hugging Face cache and runs real candle inference, which is too heavy and too
//! environment-dependent for CI. Run it manually on a host where the model is
//! cached (or has network for first-use download):
//!
//! ```bash
//! cargo test -p memoryd --test embedding_real_model_smoke -- --ignored --nocapture
//! ```
//!
//! It reports the device that loaded (Metal vs CPU) and per-call latency for
//! both an asymmetric query embedding and a plain document embedding, and
//! asserts the output dimension matches the production triple (1024) and that
//! query/document vectors differ (asymmetric prompting is actually applied).

use std::time::Instant;

use memory_substrate::EmbeddingTriple;
use memoryd::embedding::{EmbeddingProvider, FastembedProvider};

fn production_triple() -> EmbeddingTriple {
    EmbeddingTriple {
        provider: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_PROVIDER.to_string(),
        model_ref: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_MODEL_REF.to_string(),
        dimension: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_DIMENSION,
    }
}

#[test]
#[ignore = "loads the ~1.1GB Qwen3 model from the HF cache; run manually with --ignored"]
fn qwen3_loads_and_embeds_both_ways() {
    let triple = production_triple();

    let load_start = Instant::now();
    // Load via the model_ref directly so the test uses the ambient HF cache
    // (~/.cache/huggingface) rather than a throwaway runtime dir, matching the
    // "model already cached" dogfood scenario.
    let provider =
        FastembedProvider::load_from_repo(&triple.model_ref, triple.clone()).expect("Qwen3 model loads from HF cache");
    let load_ms = load_start.elapsed().as_millis();
    eprintln!("[smoke] loaded {} on {} in {load_ms} ms", triple.model_ref, provider.device().label());

    let doc_start = Instant::now();
    let document = provider
        .embed_document("We standardized on the tokio async runtime for rust daemon concurrency.")
        .expect("document embed");
    let doc_ms = doc_start.elapsed().as_millis();

    let query_start = Instant::now();
    let query = provider.embed_query("which async runtime did we choose for the rust daemon").expect("query embed");
    let query_ms = query_start.elapsed().as_millis();

    eprintln!(
        "[smoke] device={} dim={} document_embed={doc_ms} ms query_embed={query_ms} ms",
        provider.device().label(),
        document.len()
    );

    assert_eq!(document.len(), triple.dimension as usize, "document vector matches the production dimension");
    assert_eq!(query.len(), triple.dimension as usize, "query vector matches the production dimension");
    assert_ne!(document, query, "asymmetric prompting must produce different query vs document vectors");
}
