//! sqlite-vec adapter seam.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once, OnceLock};

use rusqlite::ffi::sqlite3_auto_extension;
use sha2::{Digest, Sha256};

use crate::model::EmbeddingTriple;

static REGISTER_SQLITE_VEC: Once = Once::new();

/// Memoized `(provider, model_ref, dimension) -> table name` map.
///
/// `vector_table_name` is a pure function of the triple and is invoked on every
/// vector query / drop / reconcile. The active embedding triple is effectively
/// constant, so without a cache the same SHA-256 is recomputed on each hot-path
/// vector search. The map is tiny (one entry per distinct triple ever seen) and
/// lives behind a `Mutex` since `Index` connections are shared `&self`.
///
/// Values are `Arc<str>` so a cache hit returns a refcount bump rather than a
/// fresh heap allocation per call.
#[allow(clippy::type_complexity)]
static VECTOR_TABLE_NAME_CACHE: OnceLock<Mutex<HashMap<EmbeddingTriple, Arc<str>>>> = OnceLock::new();

/// Register sqlite-vec as an auto extension for rusqlite connections.
pub fn register_extension() {
    REGISTER_SQLITE_VEC.call_once(|| {
        // SAFETY: sqlite-vec exposes `sqlite3_vec_init` with SQLite's extension
        // initializer ABI. `sqlite3_auto_extension` requires the generic SQLite
        // extension entrypoint type, so this is the standard registration cast
        // documented by sqlite-vec for rusqlite integration.
        unsafe {
            sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut std::os::raw::c_char,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> std::os::raw::c_int,
            >(sqlite_vec::sqlite3_vec_init as *const ())));
        }
    });
}

/// Deterministically name a vector table for an embedding triple.
///
/// Memoized per triple: the digest is a pure function of the triple, so repeated
/// hot-path calls for the active embedding return the cached name without
/// recomputing SHA-256. The cached value is an `Arc<str>`, so a hit is a
/// refcount bump rather than a fresh allocation, and the miss path takes the
/// lock once (insert-then-return) instead of locking twice. A poisoned cache
/// lock falls back to recomputation.
pub fn vector_table_name(triple: &EmbeddingTriple) -> Arc<str> {
    let cache = VECTOR_TABLE_NAME_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        return guard
            .entry(triple.clone())
            .or_insert_with(|| Arc::from(compute_vector_table_name(triple).as_str()))
            .clone();
    }
    Arc::from(compute_vector_table_name(triple).as_str())
}

/// Compute the vector table name digest without consulting the memoization cache.
fn compute_vector_table_name(triple: &EmbeddingTriple) -> String {
    let mut hasher = Sha256::new();
    hash_length_prefixed(&mut hasher, triple.provider.as_bytes());
    hash_length_prefixed(&mut hasher, triple.model_ref.as_bytes());
    hash_length_prefixed(&mut hasher, &triple.dimension.to_be_bytes());
    format!("vec_{}", hex::encode(&hasher.finalize()[..16]))
}

/// Validate vector dimension.
///
/// Delegates to `vector::validate_dimension`; kept here as a pub re-export so
/// callers that import from `sqlite_vec` continue to compile without change.
/// R-IX-3: the actual logic lives in `vector.rs` where it belongs.
pub use crate::index::vector::validate_dimension;

/// Serialize a float vector to sqlite-vec's compact float32 blob format.
pub fn serialize_f32(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn hash_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod vector_table_name_tests {
    use super::{compute_vector_table_name, vector_table_name};
    use crate::model::EmbeddingTriple;

    fn triple(provider: &str, model_ref: &str, dimension: u32) -> EmbeddingTriple {
        EmbeddingTriple { provider: provider.to_string(), model_ref: model_ref.to_string(), dimension }
    }

    #[test]
    fn memoized_name_matches_the_uncached_digest_and_is_stable() {
        let t = triple("openai", "text-embedding-3-small", 1536);
        let direct = compute_vector_table_name(&t);
        // First call populates the cache; subsequent calls must return the same
        // value, and it must equal the uncached digest (no behavior change).
        assert_eq!(vector_table_name(&t).as_ref(), direct.as_str());
        assert_eq!(vector_table_name(&t).as_ref(), direct.as_str());
        assert!(direct.starts_with("vec_"));
    }

    #[test]
    fn distinct_triples_produce_distinct_names() {
        let a = vector_table_name(&triple("synthetic", "model-a", 32));
        let b = vector_table_name(&triple("synthetic", "model-b", 32));
        let c = vector_table_name(&triple("synthetic", "model-a", 64));
        assert_ne!(a, b, "model_ref participates in the digest");
        assert_ne!(a, c, "dimension participates in the digest");
    }
}
