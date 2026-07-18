use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::session::ProjectBinding;

/// Sent by a Tier 1 harness session to register or refresh Level 3 presence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PeerHeartbeat {
    pub session_id: String,
    pub device_id: Option<String>,
    pub harness: String,
    pub project_binding: Option<ProjectBinding>,
    pub namespace: String,
    pub salient_entities: Vec<String>,
    pub salient_paths: Vec<String>,
    pub capabilities: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub claim_locks_held: Vec<String>,
}

/// Response returned after accepting a peer heartbeat.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PeerHeartbeatAck {
    pub session_id: String,
    pub active_level: u8,
    pub peer_session_count: u32,
    pub active_peers: Vec<ActivePeer>,
    pub conflicting_claim_locks: Vec<ClaimLockInfo>,
}

/// Serializable public projection of an active peer presence record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActivePeer {
    pub session_id: String,
    pub harness: String,
    pub salient_entities: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
}

/// Coordination entries passed to Stream E's recall assembler.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoordinationInsertion {
    pub peer_updates: Vec<PeerUpdateEntry>,
    pub peer_presence: Vec<PeerPresenceEntry>,
    pub capped_peer_updates: u32,
    pub capped_peer_presence: u32,
}

impl CoordinationInsertion {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn has_entries(&self) -> bool {
        !self.peer_updates.is_empty() || !self.peer_presence.is_empty()
    }
}

/// Data needed to render one `<peer-update>` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerUpdateEntry {
    pub harness: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub relevance: f64,
    pub summary: String,
    pub reference: String,
    pub namespace: String,
    pub claim_locked: Option<ClaimLockInfo>,
    pub device: Option<String>,
}

/// Data needed to render one `<peer-presence><session ... /></peer-presence>` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerPresenceEntry {
    pub harness: String,
    pub session_id: String,
    pub salient_entities: Vec<String>,
    pub started_at: DateTime<Utc>,
}

/// Public claim-lock projection returned in heartbeat acknowledgements and status output.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ClaimLockInfo {
    pub memory_id: String,
    pub holder_harness: String,
    pub holder_session_id: String,
    pub expires_at: DateTime<Utc>,
}
