use std::collections::HashSet;

use memory_substrate::index::{open_index, reconcile_missing, reconcile_orphans, reconcile_pending_jobs, VectorStore};
use memory_substrate::{EmbeddingLaneEligibility, EmbeddingTriple, VectorError};

#[derive(Default)]
struct InMemoryVectorStore {
    chunks: HashSet<String>,
    deleted: Vec<String>,
}

impl VectorStore for InMemoryVectorStore {
    fn list_chunk_ids(&self, _triple: &EmbeddingTriple) -> Result<HashSet<String>, VectorError> {
        Ok(self.chunks.clone())
    }

    fn delete_vector(&mut self, _triple: &EmbeddingTriple, chunk_id: &str) -> Result<(), VectorError> {
        self.chunks.remove(chunk_id);
        self.deleted.push(chunk_id.to_string());
        Ok(())
    }
}

#[test]
fn vector_orphan_and_missing_reconciliation_deletes_orphans_and_queues_jobs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let triple = triple("unit", 3);
    let valid_chunk_ids = HashSet::from(["chunk-good".to_string(), "chunk-missing".to_string()]);
    let mut store = InMemoryVectorStore {
        chunks: HashSet::from(["chunk-good".to_string(), "chunk-orphan".to_string()]),
        deleted: Vec::new(),
    };

    let deleted = reconcile_orphans(&mut store, &triple, &valid_chunk_ids).expect("orphan reconcile");
    let queued = reconcile_missing(&connection, &store, &triple, &valid_chunk_ids, EmbeddingLaneEligibility::AllTiers)
        .expect("missing reconcile");

    assert_eq!(deleted, 1);
    assert_eq!(store.deleted, vec!["chunk-orphan".to_string()]);
    assert_eq!(queued, 1);
    assert_eq!(
        reconcile_pending_jobs(&connection, &triple, EmbeddingLaneEligibility::AllTiers).expect("pending jobs"),
        1
    );
}

#[test]
fn active_triple_switch_queues_chunks_for_new_embedding_triple() {
    let temp = tempfile::tempdir().expect("tempdir");
    let connection = open_index(&temp.path().join("index.sqlite")).expect("open index");
    let old_triple = triple("old", 3);
    let new_triple = triple("new", 5);
    let old_store = InMemoryVectorStore {
        chunks: HashSet::from(["chunk-a".to_string(), "chunk-b".to_string()]),
        deleted: Vec::new(),
    };
    let new_store = InMemoryVectorStore::default();
    let valid_chunk_ids = old_store.list_chunk_ids(&old_triple).expect("old chunks");

    let queued =
        reconcile_missing(&connection, &new_store, &new_triple, &valid_chunk_ids, EmbeddingLaneEligibility::AllTiers)
            .expect("queue new triple");

    assert_eq!(queued, 2);
    assert_eq!(
        reconcile_pending_jobs(&connection, &new_triple, EmbeddingLaneEligibility::AllTiers).expect("new pending"),
        2
    );
    assert_eq!(
        reconcile_pending_jobs(&connection, &old_triple, EmbeddingLaneEligibility::AllTiers).expect("old pending"),
        0
    );
}

fn triple(model_ref: &str, dimension: u32) -> EmbeddingTriple {
    EmbeddingTriple { provider: "synthetic".to_string(), model_ref: model_ref.to_string(), dimension }
}
