//! Content-addressed hashing helpers shared by the markdown writer and the
//! filesystem watcher. Kept as a leaf module so neither `markdown` nor
//! `watcher` has to depend on the other purely to compute a CAS hash.

use sha2::{Digest, Sha256 as Sha256Hasher};

use crate::model::Sha256;

/// Hash bytes as `sha256:<hex>`.
pub fn hash_bytes(bytes: &[u8]) -> Sha256 {
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    Sha256::new(format!("sha256:{}", hex::encode(hasher.finalize())))
}
