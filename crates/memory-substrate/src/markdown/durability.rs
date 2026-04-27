//! Durability tier probing.

use std::path::Path;

use crate::model::DurabilityTier;

/// Probe parent directory fsync support for a root.
///
/// Only `ErrorKind::Unsupported` maps to `BestEffort` — genuine errors (EACCES,
/// ENOSPC, EIO, etc.) indicate a storage problem and return `Refused` so the
/// operator is told the storage is unhealthy rather than silently losing
/// durability guarantees (spec §3.1).
pub fn probe_durability(root: &Path, force_unsafe: bool) -> DurabilityTier {
    if force_unsafe {
        return DurabilityTier::BestEffort;
    }
    if std::fs::create_dir_all(root).is_err() {
        return DurabilityTier::Refused;
    }
    match std::fs::File::open(root).and_then(|file| file.sync_all()) {
        Ok(()) => DurabilityTier::Full,
        Err(err) if err.kind() == std::io::ErrorKind::Unsupported => DurabilityTier::BestEffort,
        Err(_) => DurabilityTier::Refused,
    }
}
