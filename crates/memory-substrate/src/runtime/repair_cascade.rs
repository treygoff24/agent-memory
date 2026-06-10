//! Post-commit index-repair cascade (spec §8.7).
//!
//! When a write's canonical file is durably committed but the derived index
//! upsert fails, the data is on disk but unsearchable. The cascade tries, in
//! order, to make the repair recoverable: enqueue a durable pending-index op →
//! write the startup-reconcile marker → escalate to operator action. Each step
//! is strictly weaker than the last, and the ordering is spec-mandated — the
//! daemon relies on it to decide whether writes may resume.
//!
//! Two op-type families flow through this cascade and must stay distinct:
//!
//! - **Plaintext** writes (and tombstones) enqueue a [`PendingIndexOp`] via
//!   [`enqueue_pending_index`]. The replay carries a repo path + expected file
//!   hash.
//! - **Encrypted** writes (and encrypted-metadata updates) enqueue a
//!   [`PendingEncryptedIndexOp`] via [`enqueue_pending_encrypted_index`]. The
//!   replay carries the safe index projection + expected ciphertext hash.
//!
//! The two families also map cascade outcomes to *different* failure kinds, so
//! [`CascadeFailureKinds`] selects the per-site mapping. Collapsing either the
//! op type or the failure-kind mapping would lose correctness, not noise.

use std::path::Path;

use crate::error::{WriteFailure, WriteFailureKind};
use crate::model::{DurabilityTier, OperationId, RepairRequired, WriteOutcome};

use super::reconcile::{
    enqueue_pending_encrypted_index, enqueue_pending_index, write_startup_marker, PendingEncryptedIndexOp,
    PendingIndexOp,
};

/// The durable op a failed index upsert must enqueue, kept distinct per family.
///
/// The variant determines which enqueue function the cascade calls; the two
/// queues replay through entirely different code paths during reconciliation.
pub enum IndexRepairOp {
    /// Plaintext write / tombstone: replay re-upserts a repo path by file hash.
    Plain(PendingIndexOp),
    /// Encrypted write / metadata update: replay re-upserts a safe projection
    /// by ciphertext hash. Boxed because the embedded [`PendingEncryptedIndexOp`]
    /// carries a full `Memory` and dwarfs the plaintext variant.
    Encrypted(Box<PendingEncryptedIndexOp>),
}

impl IndexRepairOp {
    /// Attempt the durable enqueue for this op family.
    fn enqueue(&self, runtime: &Path) -> std::io::Result<()> {
        match self {
            IndexRepairOp::Plain(op) => enqueue_pending_index(runtime, op),
            IndexRepairOp::Encrypted(op) => enqueue_pending_encrypted_index(runtime, op),
        }
    }
}

/// How a write site maps cascade outcomes to a [`WriteFailureKind`].
///
/// The plaintext-write site reports a single kind regardless of how far the
/// cascade degraded; the encrypted sites and the tombstone site report a
/// distinct kind per cascade step. Both mappings are part of the public write
/// contract and must survive verbatim.
pub enum CascadeFailureKinds {
    /// Always [`WriteFailureKind::IndexAfterCommitFailed`] (plaintext write).
    AlwaysIndexAfterCommit,
    /// Per-step: enqueued → `IndexAfterCommitFailed`, marker →
    /// `RepairQueueFailed`, operator → `RepairStateNotDurable` (encrypted
    /// writes, encrypted-metadata updates, tombstones).
    Tiered,
}

/// Parameters for one invocation of the repair cascade.
pub struct RepairCascade<'a> {
    /// Runtime directory holding the durable repair queues and marker.
    pub runtime: &'a Path,
    /// The durable op to enqueue, distinct per op family.
    pub op: IndexRepairOp,
    /// Reason recorded in the startup marker if the enqueue step fails. Each
    /// write site supplies its own so operators can tell the paths apart.
    pub marker_reason: &'a str,
    /// Per-site mapping from cascade outcome to [`WriteFailureKind`].
    pub failure_kinds: CascadeFailureKinds,
    /// Durability tier carried into the resulting [`WriteOutcome`].
    pub durability: DurabilityTier,
    /// Operation id carried into the resulting [`WriteOutcome`].
    pub operation_id: OperationId,
}

impl RepairCascade<'_> {
    /// Run the cascade and build the [`WriteFailure`] for a committed-but-
    /// unindexed write.
    ///
    /// The returned outcome is always `committed: true, indexed: false`: the
    /// canonical file is on disk, only the derived index lagged.
    pub fn into_failure(self) -> WriteFailure {
        let (repair_required, kind) = if self.op.enqueue(self.runtime).is_ok() {
            (RepairRequired::PendingIndex, WriteFailureKind::IndexAfterCommitFailed)
        } else if write_startup_marker(self.runtime, self.marker_reason).is_ok() {
            (RepairRequired::FullStartupScan, WriteFailureKind::RepairQueueFailed)
        } else {
            (
                RepairRequired::OperatorRequired("repair state not durable".to_string()),
                WriteFailureKind::RepairStateNotDurable,
            )
        };
        let kind = match self.failure_kinds {
            CascadeFailureKinds::AlwaysIndexAfterCommit => WriteFailureKind::IndexAfterCommitFailed,
            CascadeFailureKinds::Tiered => kind,
        };
        WriteFailure {
            outcome: WriteOutcome {
                committed: true,
                indexed: false,
                event_recorded: false,
                durability: self.durability,
                repair_required: Some(repair_required),
                operation_id: self.operation_id,
            },
            kind,
        }
    }
}
