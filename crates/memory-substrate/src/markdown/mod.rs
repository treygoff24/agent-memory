//! Durable Markdown read/write helpers.

mod atomic;
mod durability;

pub use atomic::{
    atomic_write, fsync_dir, parse_memory_bytes, read_memory_file, read_memory_file_bytes, read_memory_file_hash,
    remove_file_if_exists, AtomicWrite,
};
pub use durability::probe_durability;

/// Re-exported from the [`crate::cas`] leaf module so the historical
/// `markdown::hash_bytes` path keeps working for existing callers.
pub use crate::cas::hash_bytes;
