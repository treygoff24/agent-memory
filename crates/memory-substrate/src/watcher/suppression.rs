//! Hash-based self-event suppression.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::model::{OperationId, RepoPath, Sha256};

/// Suppression state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SuppressionState {
    /// Rename is in-flight.
    InFlight { operation_id: OperationId, expected_hash: Sha256 },
    /// Write committed.
    Committed { final_hash: Sha256, expires_at: Instant },
}

/// Suppression ledger keyed by path.
#[derive(Clone, Debug, Default)]
pub struct SuppressionLedger {
    entries: HashMap<RepoPath, SuppressionState>,
}

impl SuppressionLedger {
    /// Add in-flight entry.
    pub fn insert_in_flight(&mut self, path: RepoPath, operation_id: OperationId, expected_hash: Sha256) {
        self.entries.insert(path, SuppressionState::InFlight { operation_id, expected_hash });
    }

    /// Suppression TTL per spec §11.2.
    const TTL: Duration = Duration::from_secs(60);

    /// Promote to committed.
    pub fn promote_committed(&mut self, path: RepoPath, final_hash: Sha256) {
        self.entries.insert(path, SuppressionState::Committed { final_hash, expires_at: Instant::now() + Self::TTL });
    }

    /// Remove a suppression entry after abort/failure cleanup.
    pub fn remove(&mut self, path: &RepoPath) {
        self.entries.remove(path);
    }

    /// Return whether an event should be suppressed for a path/hash.
    pub fn should_suppress(&mut self, path: &RepoPath, hash: &Sha256) -> bool {
        self.expire();
        matches!(
            self.entries.get(path),
            Some(SuppressionState::InFlight { expected_hash, .. }) if expected_hash == hash
        ) || matches!(
            self.entries.get(path),
            Some(SuppressionState::Committed { final_hash, .. }) if final_hash == hash
        )
    }

    fn expire(&mut self) {
        let now = Instant::now();
        self.entries
            .retain(|_, state| !matches!(state, SuppressionState::Committed { expires_at, .. } if *expires_at <= now));
    }
}
