//! Hash helpers for CAS preconditions.

use sha2::{Digest, Sha256 as Sha256Hasher};

use crate::model::Sha256;

/// Hash bytes as `sha256:<hex>`.
pub fn hash_bytes(bytes: &[u8]) -> Sha256 {
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    Sha256::new(format!("sha256:{}", hex::encode(hasher.finalize())))
}
