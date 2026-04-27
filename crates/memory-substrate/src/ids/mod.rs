//! Device-sharded ID allocation and duplicate repair.

mod repair;
mod sequence;

pub use repair::{repair_duplicate_ids, RepairReport};
pub use sequence::{next_memory_id, next_memory_ids, shard_for_device, SeqState};
