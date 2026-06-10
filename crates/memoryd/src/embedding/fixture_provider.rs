//! Deterministic, model-free embedding provider for tests and CI.
//!
//! Implements the same [`EmbeddingProvider`](super::EmbeddingProvider) contract
//! as the production fastembed lane so the drain worker and e2e tests run with
//! no model download and no GPU. The embedding is a hashed bag-of-words vector
//! (feature hashing into `dimension` buckets, L2-normalized), which has the one
//! property the e2e KNN test needs: text that shares more tokens produces
//! vectors with higher cosine similarity, so nearest-neighbour ordering is
//! meaningful rather than random.
//!
//! It deliberately does not reuse `memory-test-support::perf::synthetic_vectors`
//! for the content-bearing path: those vectors are seeded by index, not by
//! content, so they cannot order a KNN query. That helper stays the sanctioned
//! source for shaped *noise* in the substrate perf gates; here we need
//! content-correlated vectors, which is a different requirement.

use std::hash::{Hash, Hasher};

use memory_substrate::EmbeddingTriple;

use super::{check_dimension, EmbeddingError, EmbeddingProvider};

/// A deterministic content-derived embedding provider.
pub struct FixtureProvider {
    triple: EmbeddingTriple,
}

impl FixtureProvider {
    /// A provider whose triple matches the synthetic test triple used across
    /// the substrate test-suite (`synthetic / stream-a-test / 32`).
    pub fn synthetic_test_triple() -> Self {
        Self::new(EmbeddingTriple {
            provider: "synthetic".to_string(),
            model_ref: "stream-a-test".to_string(),
            dimension: 32,
        })
    }

    /// A content-derived fixture provider for an arbitrary triple.
    pub fn new(triple: EmbeddingTriple) -> Self {
        Self { triple }
    }

    fn embed(&self, role: &'static str, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let vector = hashed_bag_of_words(role, text, self.triple.dimension as usize);
        check_dimension(&self.triple, &vector)?;
        Ok(vector)
    }
}

impl EmbeddingProvider for FixtureProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed("query", text)
    }

    fn embed_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed("document", text)
    }
}

/// Feature-hash the lowercased word tokens of `text` plus an asymmetric role
/// marker into `dimension` buckets, then L2-normalize. Shared tokens →
/// overlapping nonzero buckets → higher cosine similarity, while the role token
/// makes query/document call-site swaps visible in CI.
fn hashed_bag_of_words(role: &'static str, text: &str, dimension: usize) -> Vec<f32> {
    let mut vector = vec![0f32; dimension.max(1)];
    for token in std::iter::once(role).chain(text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty())) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token.to_ascii_lowercase().hash(&mut hasher);
        let bucket = (hasher.finish() as usize) % vector.len();
        vector[bucket] += 1.0;
    }
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for v in &mut vector {
            *v /= norm;
        }
    }
    vector
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn dimension_matches_triple() {
        let provider = FixtureProvider::synthetic_test_triple();
        let vector = provider.embed_document("hello world").expect("embed");
        assert_eq!(vector.len(), 32);
    }

    #[test]
    fn shared_tokens_rank_closer_than_disjoint_text() {
        let triple = EmbeddingTriple { provider: "f".into(), model_ref: "m".into(), dimension: 256 };
        let provider = FixtureProvider::new(triple);
        let query = provider.embed_query("rust async tokio runtime").expect("q");
        let near = provider.embed_document("the tokio async runtime in rust").expect("near");
        let far = provider.embed_document("baking sourdough bread at home").expect("far");
        assert!(cosine(&query, &near) > cosine(&query, &far), "shared-token doc must rank nearer");
    }

    #[test]
    fn embedding_is_deterministic() {
        let provider = FixtureProvider::synthetic_test_triple();
        let first = provider.embed_document("stable content").expect("first");
        let second = provider.embed_document("stable content").expect("second");
        assert_eq!(first, second);
    }

    #[test]
    fn query_and_document_embeddings_are_asymmetric() {
        let provider = FixtureProvider::synthetic_test_triple();
        let query = provider.embed_query("same content").expect("query");
        let document = provider.embed_document("same content").expect("document");
        assert_ne!(query, document, "fixture must catch query/document call-site swaps");
    }
}
