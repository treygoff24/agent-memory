//! sqlite-vec adapter seam.

use std::sync::Once;

use rusqlite::ffi::sqlite3_auto_extension;
use sha2::{Digest, Sha256};

use crate::model::EmbeddingTriple;

static REGISTER_SQLITE_VEC: Once = Once::new();

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
pub fn vector_table_name(triple: &EmbeddingTriple) -> String {
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
