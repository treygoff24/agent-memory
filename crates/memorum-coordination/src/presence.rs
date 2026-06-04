use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::{mapref::entry::Entry, DashMap};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Instant as TokioInstant, MissedTickBehavior};

use crate::claim_lock::{ClaimLockClock, ClaimLockRegistry, ClaimLockRenewRequest};
use crate::config::PresenceConfig;
use crate::{ActivePeer, ClaimLockInfo, ConcurrentSessionMode, PeerHeartbeat, PeerHeartbeatAck, ProjectBinding};

const MAX_SESSION_ID_BYTES: usize = 128;
const MAX_HARNESS_BYTES: usize = 128;
const MAX_SALIENT_ENTITIES: usize = 32;
const MAX_ENTITY_BYTES: usize = 128;
const MAX_SALIENT_PATHS: usize = 32;
const MAX_PATH_BYTES: usize = 256;
const MAX_CAPABILITIES: usize = 16;
const MAX_CAPABILITY_BYTES: usize = 64;
const MAX_CLAIM_LOCKS_HELD: usize = 16;
const MAX_CLAIM_LOCK_ID_BYTES: usize = 128;
const MAX_ACTIVE_PEER_ENTITIES: usize = 5;
const ACTIVE_PEER_SESSION_ID_BYTES: usize = 6;
pub const PRESENCE_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

pub trait StaleSessionClaimLockReleaser: Send + Sync + 'static {
    fn release_all_held_by(&self, harness: &str, session_id: &str);

    fn sweep_expired_at(&self, _now: Instant) -> Vec<ClaimLockInfo> {
        Vec::new()
    }
}

impl StaleSessionClaimLockReleaser for ClaimLockRegistry {
    fn release_all_held_by(&self, harness: &str, session_id: &str) {
        // Returned lock infos are intentionally discarded: the cleanup path
        // only needs the release side-effect, not the released-lock inventory.
        let _released = ClaimLockRegistry::release_all_held_by(self, harness, session_id);
    }

    fn sweep_expired_at(&self, now: Instant) -> Vec<ClaimLockInfo> {
        ClaimLockRegistry::sweep_expired_at(self, now)
    }
}

/// In-memory presence state for one peer session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceRecord {
    pub session_id: String,
    pub device_id: Option<String>,
    pub harness: String,
    pub project_binding: Option<ProjectBinding>,
    pub namespace: String,
    pub salient_entities: Vec<String>,
    pub salient_paths: Vec<String>,
    pub capabilities: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: Instant,
    pub claim_locks_held: Vec<String>,
}

/// Monotonic active-peer query parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivePeerQuery<'a> {
    pub namespace: &'a str,
    pub own_session_id: Option<&'a str>,
    pub now: Instant,
    pub stale_threshold: Duration,
}

/// Runtime inputs needed to process a heartbeat without coupling to memoryd state.
#[derive(Clone, Copy, Debug)]
pub struct ClaimLockHeartbeatRenewal<'a> {
    pub registry: &'a ClaimLockRegistry,
    pub ttl: Duration,
    pub clock: ClaimLockClock,
}

#[derive(Clone, Copy, Debug)]
pub struct PeerHeartbeatOptions<'a> {
    pub default_level: u8,
    pub now: Instant,
    pub stale_threshold: Duration,
    pub claim_lock_renewal: Option<ClaimLockHeartbeatRenewal<'a>>,
}

/// Validation error for malformed heartbeat requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerHeartbeatError {
    InvalidRequest { message: String },
}

impl fmt::Display for PeerHeartbeatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message } => write!(formatter, "invalid heartbeat request: {message}"),
        }
    }
}

impl Error for PeerHeartbeatError {}

/// Concurrent in-memory presence registry keyed by session id.
#[derive(Debug, Default)]
pub struct PresenceRegistry {
    records: DashMap<String, PresenceRecord>,
}

impl PresenceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, record: PresenceRecord) {
        match self.records.entry(record.session_id.clone()) {
            Entry::Occupied(mut entry) => {
                let mut updated_record = record;
                updated_record.started_at = entry.get().started_at.or(updated_record.started_at);
                entry.insert(updated_record);
            }
            Entry::Vacant(entry) => {
                entry.insert(record);
            }
        }
    }

    pub fn remove(&self, session_id: &str) {
        self.records.remove(session_id);
    }

    pub fn snapshot_for_namespace(&self, namespace: &str) -> Vec<PresenceRecord> {
        sorted_presence_records(
            self.records
                .iter()
                .filter(|entry| entry.namespace == namespace)
                .map(|entry| entry.value().clone())
                .collect(),
        )
    }

    pub fn all_records(&self) -> Vec<PresenceRecord> {
        sorted_presence_records(self.records.iter().map(|entry| entry.value().clone()).collect())
    }

    pub fn active_peers(&self, query: ActivePeerQuery<'_>) -> Vec<PresenceRecord> {
        sorted_presence_records(
            self.records
                .iter()
                .filter(|entry| entry.namespace == query.namespace)
                .filter(|entry| query.own_session_id != Some(entry.session_id.as_str()))
                .filter(|entry| !is_stale(entry.value(), query.now, query.stale_threshold))
                .map(|entry| entry.value().clone())
                .collect(),
        )
    }

    pub fn cleanup_stale(&self, stale_threshold: Duration) -> Vec<String> {
        self.cleanup_stale_at(Instant::now(), stale_threshold)
    }

    pub fn cleanup_stale_at(&self, now: Instant, stale_threshold: Duration) -> Vec<String> {
        self.cleanup_stale_records_at(now, stale_threshold).into_iter().map(|record| record.session_id).collect()
    }

    pub fn cleanup_stale_records_at(&self, now: Instant, stale_threshold: Duration) -> Vec<PresenceRecord> {
        let mut stale_session_ids = self
            .records
            .iter()
            .filter(|entry| is_stale(entry.value(), now, stale_threshold))
            .map(|entry| entry.session_id.clone())
            .collect::<Vec<_>>();
        stale_session_ids.sort();

        stale_session_ids
            .into_iter()
            .filter_map(|session_id| {
                self.records
                    .remove_if(&session_id, |_, record| is_stale(record, now, stale_threshold))
                    .map(|(_, record)| record)
            })
            .collect()
    }
}

pub fn cleanup_stale_sessions<R>(
    presence_registry: &PresenceRegistry,
    claim_lock_registry: &R,
    now: Instant,
    stale_threshold: Duration,
) -> Vec<String>
where
    R: StaleSessionClaimLockReleaser + ?Sized,
{
    let removed_records = presence_registry.cleanup_stale_records_at(now, stale_threshold);
    for record in &removed_records {
        claim_lock_registry.release_all_held_by(&record.harness, &record.session_id);
    }
    removed_records.into_iter().map(|record| record.session_id).collect()
}

pub fn spawn_stale_session_cleanup_task<R>(
    presence_registry: Arc<PresenceRegistry>,
    claim_lock_registry: Arc<R>,
    presence_config: PresenceConfig,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()>
where
    R: StaleSessionClaimLockReleaser,
{
    tokio::spawn(run_stale_session_cleanup_task(
        presence_registry,
        claim_lock_registry,
        presence_config.stale_after(),
        shutdown_rx,
    ))
}

async fn run_stale_session_cleanup_task<R>(
    presence_registry: Arc<PresenceRegistry>,
    claim_lock_registry: Arc<R>,
    stale_threshold: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) where
    R: StaleSessionClaimLockReleaser,
{
    let mut interval = interval_at(TokioInstant::now() + PRESENCE_CLEANUP_INTERVAL, PRESENCE_CLEANUP_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                cleanup_stale_sessions(
                    presence_registry.as_ref(),
                    claim_lock_registry.as_ref(),
                    Instant::now(),
                    stale_threshold,
                );
                claim_lock_registry.sweep_expired_at(Instant::now());
            }
            shutdown = shutdown_rx.changed() => {
                if shutdown.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
}

pub fn handle_peer_heartbeat(
    registry: &PresenceRegistry,
    heartbeat: PeerHeartbeat,
    options: PeerHeartbeatOptions<'_>,
) -> Result<PeerHeartbeatAck, PeerHeartbeatError> {
    let validated = ValidatedHeartbeat::try_from(heartbeat)?;
    let active_level = active_level(validated.project_binding.as_ref(), options.default_level);

    if active_level == 3 {
        renew_held_claim_locks(&validated, options.claim_lock_renewal);
        registry.upsert(validated.to_presence_record(options.now));
    }

    let active_peers = if active_level == 3 {
        registry
            .active_peers(ActivePeerQuery {
                namespace: &validated.namespace,
                own_session_id: Some(&validated.session_id),
                now: options.now,
                stale_threshold: options.stale_threshold,
            })
            .into_iter()
            .map(ActivePeer::from)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    Ok(PeerHeartbeatAck {
        session_id: validated.session_id,
        active_level,
        peer_session_count: active_peers.len() as u32,
        active_peers,
        conflicting_claim_locks: Vec::new(),
    })
}

struct ValidatedHeartbeat {
    session_id: String,
    device_id: Option<String>,
    harness: String,
    project_binding: Option<ProjectBinding>,
    namespace: String,
    salient_entities: Vec<String>,
    salient_paths: Vec<String>,
    capabilities: Vec<String>,
    started_at: Option<DateTime<Utc>>,
    claim_locks_held: Vec<String>,
}

impl ValidatedHeartbeat {
    fn try_from(heartbeat: PeerHeartbeat) -> Result<Self, PeerHeartbeatError> {
        let session_id = validate_trimmed_required("session_id", heartbeat.session_id, MAX_SESSION_ID_BYTES)?;
        let harness = validate_trimmed_required("harness", heartbeat.harness, MAX_HARNESS_BYTES)?;
        let namespace = validate_trimmed_required("namespace", heartbeat.namespace, MAX_PATH_BYTES)?;

        validate_bounded_items(
            "salient_entities",
            &heartbeat.salient_entities,
            MAX_SALIENT_ENTITIES,
            MAX_ENTITY_BYTES,
        )?;
        validate_bounded_items("salient_paths", &heartbeat.salient_paths, MAX_SALIENT_PATHS, MAX_PATH_BYTES)?;
        validate_bounded_items("capabilities", &heartbeat.capabilities, MAX_CAPABILITIES, MAX_CAPABILITY_BYTES)?;
        validate_count("claim_locks_held", heartbeat.claim_locks_held.len(), MAX_CLAIM_LOCKS_HELD)?;
        let claim_locks_held = validate_claim_locks_held(heartbeat.claim_locks_held)?;

        Ok(Self {
            session_id,
            device_id: normalize_optional(heartbeat.device_id),
            harness,
            project_binding: heartbeat.project_binding,
            namespace,
            salient_entities: heartbeat.salient_entities,
            salient_paths: heartbeat.salient_paths,
            capabilities: heartbeat.capabilities,
            started_at: heartbeat.started_at,
            claim_locks_held,
        })
    }

    fn to_presence_record(&self, last_heartbeat_at: Instant) -> PresenceRecord {
        PresenceRecord {
            session_id: self.session_id.clone(),
            device_id: self.device_id.clone(),
            harness: self.harness.clone(),
            project_binding: self.project_binding.clone(),
            namespace: self.namespace.clone(),
            salient_entities: self.salient_entities.clone(),
            salient_paths: self.salient_paths.clone(),
            capabilities: self.capabilities.clone(),
            started_at: self.started_at,
            last_heartbeat_at,
            claim_locks_held: self.claim_locks_held.clone(),
        }
    }
}

impl From<PresenceRecord> for ActivePeer {
    fn from(record: PresenceRecord) -> Self {
        Self {
            session_id: truncate_for_display(&record.session_id, ACTIVE_PEER_SESSION_ID_BYTES),
            harness: record.harness,
            salient_entities: record.salient_entities.into_iter().take(MAX_ACTIVE_PEER_ENTITIES).collect(),
            started_at: record.started_at,
        }
    }
}

fn renew_held_claim_locks(heartbeat: &ValidatedHeartbeat, renewal: Option<ClaimLockHeartbeatRenewal<'_>>) {
    // Renewals are best-effort: the registry returns a typed result we
    // intentionally discard. A failure here means the lock was already
    // released, expired, or transferred — none of which should block the
    // heartbeat path. The next heartbeat will retry.
    let Some(renewal) = renewal else {
        return;
    };

    for memory_id in &heartbeat.claim_locks_held {
        let request = ClaimLockRenewRequest::new(
            memory_id.as_str(),
            heartbeat.session_id.as_str(),
            heartbeat.harness.as_str(),
            renewal.ttl,
        );
        let _result = renewal.registry.renew_at(request, renewal.clock);
    }
}

fn active_level(project_binding: Option<&ProjectBinding>, default_level: u8) -> u8 {
    match project_binding.and_then(|binding| binding.concurrent_session_mode) {
        Some(ConcurrentSessionMode::Minimal) => 1,
        Some(ConcurrentSessionMode::Default) => 2,
        Some(ConcurrentSessionMode::Collaborative) => 3,
        None => default_level,
    }
}

fn validate_trimmed_required(
    field: &'static str,
    value: String,
    max_bytes: usize,
) -> Result<String, PeerHeartbeatError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return invalid_request(format!("{field} must be non-empty"));
    }
    if trimmed.len() > max_bytes {
        return invalid_request(format!("{field} must be at most {max_bytes} UTF-8 bytes"));
    }
    Ok(trimmed.to_string())
}

fn validate_bounded_items(
    field: &'static str,
    values: &[String],
    max_count: usize,
    max_bytes: usize,
) -> Result<(), PeerHeartbeatError> {
    validate_count(field, values.len(), max_count)?;
    for value in values {
        if value.len() > max_bytes {
            return invalid_request(format!("{field} entries must be at most {max_bytes} UTF-8 bytes"));
        }
    }
    Ok(())
}

fn validate_count(field: &'static str, count: usize, max_count: usize) -> Result<(), PeerHeartbeatError> {
    if count > max_count {
        return invalid_request(format!("{field} must contain at most {max_count} entries"));
    }
    Ok(())
}

fn validate_claim_locks_held(claim_locks_held: Vec<String>) -> Result<Vec<String>, PeerHeartbeatError> {
    claim_locks_held
        .into_iter()
        .map(|memory_id| {
            let memory_id = validate_trimmed_required("claim_locks_held", memory_id, MAX_CLAIM_LOCK_ID_BYTES)?;
            if !memory_id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
            {
                return invalid_request(
                    "claim_locks_held entries may contain only ASCII letters, digits, '_' or '-'".to_string(),
                );
            }
            Ok(memory_id)
        })
        .collect()
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn invalid_request<T>(message: String) -> Result<T, PeerHeartbeatError> {
    Err(PeerHeartbeatError::InvalidRequest { message })
}

fn truncate_for_display(value: &str, max_bytes: usize) -> String {
    value
        .chars()
        .scan(0, |used, character| {
            let next = *used + character.len_utf8();
            (next <= max_bytes).then(|| {
                *used = next;
                character
            })
        })
        .collect()
}

fn sorted_presence_records(mut records: Vec<PresenceRecord>) -> Vec<PresenceRecord> {
    records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    records
}

fn is_stale(record: &PresenceRecord, now: Instant, stale_threshold: Duration) -> bool {
    now.checked_duration_since(record.last_heartbeat_at).is_some_and(|elapsed| elapsed > stale_threshold)
}
