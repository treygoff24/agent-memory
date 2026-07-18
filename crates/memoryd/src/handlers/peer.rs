//! Peer coordination: cross-device delivery audit, per-session update cooldowns,
//! and the peer heartbeat/status/activity/release-lock request handlers.

use super::*;

const PEER_DELIVERY_AUDIT_CAPACITY: usize = 200;
const PEER_ACTIVITY_LIMIT_DEFAULT: usize = 50;
const PEER_ACTIVITY_LIMIT_MAX: usize = 200;
const PEER_STATUS_RECENT_DELIVERIES: usize = 5;

impl DeltaPeerDeliveryRecorder for HandlerState {
    fn record_delta_peer_delivery(&self, delivery: DeltaPeerDelivery) {
        self.record_peer_delivery(delivery);
    }
}

impl DeltaPeerCooldownStore for HandlerState {
    fn surfaced_peer_writes(&self, session_binding: &SessionBinding) -> HashSet<String> {
        self.peer_update_cooldowns.surfaced_peer_writes(session_binding)
    }

    fn record_surfaced_peer_writes(&self, session_binding: &SessionBinding, memory_ids: &[String]) {
        self.peer_update_cooldowns.record_surfaced_peer_writes(session_binding, memory_ids);
    }
}

#[derive(Debug, Default)]
pub(crate) struct PeerDeliveryAudit {
    entries: StdMutex<VecDeque<PeerDeliveryAuditEntry>>,
}

#[derive(Debug, Default)]
pub(crate) struct PeerUpdateCooldowns {
    surfaced: StdMutex<BTreeMap<PeerUpdateCooldownKey, BTreeSet<String>>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PeerUpdateCooldownKey {
    harness: String,
    session_id: String,
    namespaces: Vec<String>,
}

impl PeerDeliveryAudit {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(&self, entry: PeerDeliveryAuditEntry) {
        let mut entries = self.entries.lock().expect("peer delivery audit lock poisoned");
        if entries.len() == PEER_DELIVERY_AUDIT_CAPACITY {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub(crate) fn snapshot(&self) -> Vec<PeerDeliveryAuditEntry> {
        self.entries.lock().expect("peer delivery audit lock poisoned").iter().cloned().collect()
    }
}

impl PeerUpdateCooldowns {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn surfaced_peer_writes(&self, session_binding: &SessionBinding) -> HashSet<String> {
        self.surfaced
            .lock()
            .expect("peer update cooldown lock poisoned")
            .get(&PeerUpdateCooldownKey::from(session_binding))
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    fn record_surfaced_peer_writes(&self, session_binding: &SessionBinding, memory_ids: &[String]) {
        if memory_ids.is_empty() {
            return;
        }
        let mut surfaced = self.surfaced.lock().expect("peer update cooldown lock poisoned");
        surfaced.entry(PeerUpdateCooldownKey::from(session_binding)).or_default().extend(memory_ids.iter().cloned());
    }
}

impl PeerUpdateCooldownKey {
    fn from(session_binding: &SessionBinding) -> Self {
        let mut namespaces = session_binding.namespaces_in_scope.clone();
        namespaces.sort();
        namespaces.dedup();
        Self { harness: session_binding.harness.clone(), session_id: session_binding.session_id.clone(), namespaces }
    }
}

pub(crate) async fn peer_heartbeat_response(
    substrate: &Substrate,
    state: &HandlerState,
    heartbeat: crate::protocol::PeerHeartbeat,
) -> Result<ResponsePayload, HandlerError> {
    let clock = ClaimLockClock::now();
    let mut ack = coordination_handle_peer_heartbeat(
        state.presence(),
        heartbeat.clone(),
        PeerHeartbeatOptions {
            default_level: state.coordination_level(),
            now: clock.instant,
            stale_threshold: state.presence_config().stale_after(),
            claim_lock_renewal: Some(ClaimLockHeartbeatRenewal {
                registry: state.claim_locks(),
                ttl: state.claim_lock_ttl(),
                clock,
            }),
        },
    )
    .map_err(peer_heartbeat_error)?;
    // INVARIANT: `conflicting_claim_locks` is populated only at Level 3.
    // Level 1/2 acknowledgements intentionally return `[]`; Level 2 dogfood
    // keeps same-device claim-lock conflict signals dormant until full
    // concurrent-session mode is explicitly enabled.
    // See docs/specs/stream-i-cross-session-v0.1.md §6 and
    // docs/api/stream-i-cross-session-api.md (heartbeat notes).
    if ack.active_level == 3 {
        ack.conflicting_claim_locks = conflicting_claim_locks_for_heartbeat(substrate, state, &heartbeat).await;
    }
    Ok(ResponsePayload::PeerHeartbeat(ack))
}

fn peer_heartbeat_error(error: PeerHeartbeatError) -> HandlerError {
    match error {
        PeerHeartbeatError::InvalidRequest { message } => HandlerError::invalid_request(message),
    }
}

async fn conflicting_claim_locks_for_heartbeat(
    substrate: &Substrate,
    state: &HandlerState,
    heartbeat: &crate::protocol::PeerHeartbeat,
) -> Vec<ClaimLockInfo> {
    let heartbeat_entities = heartbeat
        .salient_entities
        .iter()
        .map(|entity| entity.trim().to_ascii_lowercase())
        .filter(|entity| !entity.is_empty())
        .collect::<HashSet<_>>();
    if heartbeat_entities.is_empty() {
        return Vec::new();
    }

    // Candidate locks held by *other* sessions; same-session locks never
    // conflict and are dropped before any index work.
    let candidate_locks = state
        .claim_locks()
        .active_locks()
        .into_iter()
        .filter(|lock| !(lock.holder_harness == heartbeat.harness && lock.holder_session_id == heartbeat.session_id))
        .filter(|lock| MemoryId::try_new(lock.memory_id.clone()).is_ok())
        .collect::<Vec<_>>();
    if candidate_locks.is_empty() {
        return Vec::new();
    }

    // Resolve the entity/alias sets for every candidate lock in one batched
    // index query instead of an O(locks x total_memories) file tree-walk.
    let memory_ids = candidate_locks.iter().map(|lock| lock.memory_id.clone()).collect::<Vec<_>>();
    let entities_by_memory = match substrate.entities_for_memories(&memory_ids).await {
        Ok(entities) => entities,
        Err(err) => {
            // Index unavailable: entity intersections can't be computed. Reporting
            // "no conflicts" is fail-open (the peer proceeds unwarned), so surface
            // the degradation instead of swallowing it silently.
            tracing::warn!(error = %err, "claim-lock conflict check skipped: entity index unavailable");
            return Vec::new();
        }
    };

    let mut locks = Vec::new();
    for lock in candidate_locks {
        let Some(entities) = entities_by_memory.get(&lock.memory_id) else {
            continue;
        };
        let intersects = entities.iter().any(|entity| {
            heartbeat_entities.contains(entity.id.trim().to_ascii_lowercase().as_str())
                || entity
                    .aliases
                    .iter()
                    .any(|alias| heartbeat_entities.contains(alias.trim().to_ascii_lowercase().as_str()))
        });
        if intersects {
            locks.push(lock);
        }
    }
    locks.sort_by(|left, right| {
        left.memory_id
            .cmp(&right.memory_id)
            .then_with(|| left.holder_harness.cmp(&right.holder_harness))
            .then_with(|| left.holder_session_id.cmp(&right.holder_session_id))
    });
    locks
}

pub(crate) fn peer_status_response(state: &HandlerState) -> PeerStatusResponse {
    let now = Instant::now();
    let stale_after = state.presence_config().stale_after();
    let mut active_sessions = state
        .presence()
        .all_records()
        .into_iter()
        .filter(|record| now.duration_since(record.last_heartbeat_at) <= stale_after)
        .map(|record| PeerSessionStatus {
            session_id: record.session_id,
            harness: record.harness,
            namespace: record.namespace,
            salient_entities: record.salient_entities.into_iter().take(5).collect(),
            started_at: record.started_at,
            last_heartbeat_age_seconds: now.duration_since(record.last_heartbeat_at).as_secs(),
        })
        .collect::<Vec<_>>();
    active_sessions.sort_by(|left, right| {
        left.harness
            .cmp(&right.harness)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.namespace.cmp(&right.namespace))
    });

    let mut claim_locks = state.claim_locks().active_locks();
    claim_locks.sort_by(|left, right| {
        left.memory_id
            .cmp(&right.memory_id)
            .then_with(|| left.holder_harness.cmp(&right.holder_harness))
            .then_with(|| left.holder_session_id.cmp(&right.holder_session_id))
    });

    let recent_deliveries = recent_deliveries(state, PEER_STATUS_RECENT_DELIVERIES);

    PeerStatusResponse {
        coordination_level: state.coordination_level(),
        active_sessions,
        claim_locks,
        recent_deliveries,
    }
}

pub(crate) fn peer_activity_response(
    state: &HandlerState,
    session: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<PeerActivityResponse, HandlerError> {
    let limit = limit.unwrap_or(PEER_ACTIVITY_LIMIT_DEFAULT).min(PEER_ACTIVITY_LIMIT_MAX);
    let since = since.map(parse_peer_activity_since).transpose()?;
    let mut entries = state
        .peer_deliveries
        .snapshot()
        .into_iter()
        .filter(|entry| session.is_none_or(|session| peer_delivery_matches_session(entry, session)))
        .filter(|entry| since.is_none_or(|since| entry.delivered_at >= since))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .delivered_at
            .cmp(&left.delivered_at)
            .then_with(|| left.from_session_id.cmp(&right.from_session_id))
            .then_with(|| left.to_session_id.cmp(&right.to_session_id))
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
    let total_recorded = entries.len();
    entries.truncate(limit);
    Ok(PeerActivityResponse { entries, limit, total_recorded })
}

pub(crate) fn peer_release_lock_response(
    state: &HandlerState,
    memory_id: &str,
) -> Result<PeerReleaseLockResponse, HandlerError> {
    let memory_id = memory_id.trim();
    if memory_id.is_empty() {
        return Err(HandlerError::invalid_request("memory_id must not be empty"));
    }

    let released = state
        .claim_locks()
        .get(memory_id)
        .and_then(|lock| state.claim_locks().release(memory_id, &lock.holder_harness, &lock.holder_session_id));
    let status = if released.is_some() { PeerReleaseLockStatus::Released } else { PeerReleaseLockStatus::NoLockFound };

    Ok(PeerReleaseLockResponse { memory_id: memory_id.to_owned(), status, released })
}

fn recent_deliveries(state: &HandlerState, limit: usize) -> Vec<PeerDeliveryAuditEntry> {
    let mut deliveries = state.peer_deliveries.snapshot();
    deliveries.sort_by(|left, right| right.delivered_at.cmp(&left.delivered_at));
    deliveries.truncate(limit);
    deliveries
}

fn peer_delivery_matches_session(entry: &PeerDeliveryAuditEntry, session: &str) -> bool {
    entry.from_session_id == session || entry.to_session_id == session
}

fn parse_peer_activity_since(raw: &str) -> Result<chrono::DateTime<chrono::Utc>, HandlerError> {
    if let Ok(date_time) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(date_time.with_timezone(&chrono::Utc));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let Some(date_time) = date.and_hms_opt(0, 0, 0) else {
            return Err(HandlerError::invalid_request("invalid peer activity --since date"));
        };
        return Ok(date_time.and_utc());
    }
    if let Ok(time) = chrono::NaiveTime::parse_from_str(raw, "%H:%M") {
        let date = chrono::Utc::now().date_naive();
        return Ok(date.and_time(time).and_utc());
    }
    Err(HandlerError::invalid_request("peer activity --since must be HH:MM, YYYY-MM-DD, or RFC3339"))
}
