#![deny(unsafe_op_in_unsafe_fn)]
//! Stream I cross-session coordination primitives.

pub mod claim_lock;
pub mod config;
pub mod framing_tests;
pub mod gate;
pub mod presence;
pub mod protocol;
pub mod session;

pub use claim_lock::{ClaimLockAcquireResult, ClaimLockRegistry, ClaimLockRenewResult};
pub use config::{ClaimLockConfig, CoordinationConfig, PresenceConfig, RelevanceGateConfig};
pub use gate::{PeerWriteCandidate, RelevanceGate};
pub use presence::{
    cleanup_stale_sessions, handle_peer_heartbeat, spawn_stale_session_cleanup_task, PeerHeartbeatError,
    PeerHeartbeatOptions, PresenceRecord, PresenceRegistry, StaleSessionClaimLockReleaser, PRESENCE_CLEANUP_INTERVAL,
};
pub use protocol::{
    ActivePeer, ClaimLockInfo, CoordinationInsertion, PeerHeartbeat, PeerHeartbeatAck, PeerPresenceEntry,
    PeerUpdateEntry,
};
pub use session::{ConcurrentSessionMode, ProjectBinding, QueryEmbedding, SessionContext};
