//! Deterministic content hashing primitives shared across governance modules.
//!
//! Tombstone matching and contradiction detection both fingerprint claim text
//! and entity sets the same way: normalize whitespace + case, then run an
//! in-process FNV-1a hash so the digest stays stable across crate versions
//! and platforms (we deliberately avoid `std::collections::hash_map::DefaultHasher`
//! because its seed is randomized per process).
//!
//! `#[allow(dead_code)]` covers `#[path]`-included integration tests that only
//! pull in one of the consumer modules (e.g. `tombstone_contract.rs` does not
//! reference `canonical_entity_hash`).
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

/// Canonicalize free-form text by collapsing whitespace and lowercasing.
pub(crate) fn canonical_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Stable hash for a claim string after canonicalization.
pub(crate) fn canonical_claim_hash(claim: &str) -> String {
    stable_hash(&canonical_text(claim))
}

/// Stable hash for a set of entity ids after canonicalization and deduplication.
pub(crate) fn canonical_entity_hash(entity_ids: &[String]) -> String {
    stable_hash(
        &entity_ids
            .iter()
            .map(|entity_id| canonical_text(entity_id))
            .filter(|entity_id| !entity_id.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// FNV-1a 64-bit hash, formatted as `fnv64:{hex}`.
pub(crate) fn stable_hash(value: &str) -> String {
    let mut hasher = StableHasher::default();
    value.hash(&mut hasher);
    format!("fnv64:{:016x}", hasher.finish())
}

#[derive(Default)]
struct StableHasher(u64);

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        if self.0 == 0 {
            self.0 = 0xcbf2_9ce4_8422_2325;
        }

        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}
