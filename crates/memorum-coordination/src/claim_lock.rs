use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::{mapref::entry::Entry, DashMap};

use crate::protocol::ClaimLockInfo;

pub const CLAIM_LOCK_CONTENTION_CODE: &str = "claim_lock_contention";

/// Clock values captured at one point in time.
///
/// Expiry decisions use `instant` so tests and production code are immune to
/// wall-clock jumps. `utc` is only the serializable timestamp surfaced to
/// protocol/status callers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClaimLockClock {
    pub instant: Instant,
    pub utc: DateTime<Utc>,
}

impl ClaimLockClock {
    pub fn now() -> Self {
        Self { instant: Instant::now(), utc: Utc::now() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimLockAcquireRequest {
    pub memory_id: String,
    pub session_id: String,
    pub harness: String,
    pub ttl: Duration,
}

impl ClaimLockAcquireRequest {
    pub fn new(
        memory_id: impl Into<String>,
        session_id: impl Into<String>,
        harness: impl Into<String>,
        ttl: Duration,
    ) -> Self {
        Self { memory_id: memory_id.into(), session_id: session_id.into(), harness: harness.into(), ttl }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimLockRenewRequest {
    pub memory_id: String,
    pub session_id: String,
    pub harness: String,
    pub ttl: Duration,
}

impl ClaimLockRenewRequest {
    pub fn new(
        memory_id: impl Into<String>,
        session_id: impl Into<String>,
        harness: impl Into<String>,
        ttl: Duration,
    ) -> Self {
        Self { memory_id: memory_id.into(), session_id: session_id.into(), harness: harness.into(), ttl }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimLockAcquireResult {
    Acquired(ClaimLockInfo),
    AlreadyHeld(ClaimLockInfo),
    Contended(ClaimLockContention),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimLockContention {
    pub warning_code: &'static str,
    pub message: String,
    pub memory_id: String,
    pub holder: ClaimLockInfo,
    pub contender_harness: String,
    pub contender_session_id: String,
}

impl ClaimLockContention {
    pub fn holder_label(&self) -> String {
        holder_label(&self.holder.holder_harness, &self.holder.holder_session_id)
    }

    pub fn contender_label(&self) -> String {
        holder_label(&self.contender_harness, &self.contender_session_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimLockRenewResult {
    Renewed(ClaimLockInfo),
    NotHeld,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClaimLockEntry {
    memory_id: String,
    holder_harness: String,
    holder_session_id: String,
    acquired_at: Instant,
    expires_at: Instant,
    expires_at_utc: DateTime<Utc>,
}

impl ClaimLockEntry {
    fn new(request: ClaimLockAcquireRequest, clock: ClaimLockClock) -> Self {
        Self {
            memory_id: request.memory_id,
            holder_harness: request.harness,
            holder_session_id: request.session_id,
            acquired_at: clock.instant,
            expires_at: clock.instant + request.ttl,
            expires_at_utc: expires_at_utc(clock.utc, request.ttl),
        }
    }

    fn is_live_at(&self, now: Instant) -> bool {
        self.expires_at > now
    }

    fn is_held_by(&self, harness: &str, session_id: &str) -> bool {
        self.holder_harness == harness && self.holder_session_id == session_id
    }

    fn renew(&mut self, ttl: Duration, clock: ClaimLockClock) {
        self.acquired_at = clock.instant;
        self.expires_at = clock.instant + ttl;
        self.expires_at_utc = expires_at_utc(clock.utc, ttl);
    }

    fn info(&self) -> ClaimLockInfo {
        ClaimLockInfo {
            memory_id: self.memory_id.clone(),
            holder_harness: self.holder_harness.clone(),
            holder_session_id: self.holder_session_id.clone(),
            expires_at: self.expires_at_utc,
        }
    }
}

/// Concurrent RAM-only advisory claim-lock registry keyed by memory id.
#[derive(Debug, Default)]
pub struct ClaimLockRegistry {
    locks: DashMap<String, ClaimLockEntry>,
}

impl ClaimLockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&self, request: ClaimLockAcquireRequest) -> ClaimLockAcquireResult {
        self.acquire_at(request, ClaimLockClock::now())
    }

    pub fn acquire_at(&self, request: ClaimLockAcquireRequest, clock: ClaimLockClock) -> ClaimLockAcquireResult {
        let memory_id = request.memory_id.clone();

        match self.locks.entry(memory_id.clone()) {
            Entry::Occupied(mut entry) => {
                let existing = entry.get();
                if existing.is_live_at(clock.instant) {
                    let existing_info = existing.info();
                    if existing.is_held_by(&request.harness, &request.session_id) {
                        return ClaimLockAcquireResult::AlreadyHeld(existing_info);
                    }

                    // INVARIANT: advisory contention — the *original* holder's
                    // lock entry is preserved in the registry.  The caller
                    // receives `Contended` with the original holder info so it
                    // can warn the user and decide whether to proceed.  We do
                    // NOT replace the entry here; the contender may call
                    // `restore` to reclaim a previous snapshot after it
                    // completes its own work, or it may simply proceed without
                    // taking ownership.  Replacing the entry on contention would
                    // silently evict the holder — surprising for advisory
                    // semantics and observable via `peer status`.
                    let contender_harness = request.harness;
                    let contender_session_id = request.session_id;
                    return ClaimLockAcquireResult::Contended(contention(
                        memory_id,
                        existing_info,
                        contender_harness,
                        contender_session_id,
                    ));
                }

                let lock = ClaimLockEntry::new(request, clock);
                let info = lock.info();
                entry.insert(lock);
                ClaimLockAcquireResult::Acquired(info)
            }
            Entry::Vacant(entry) => {
                let lock = ClaimLockEntry::new(request, clock);
                let info = lock.info();
                entry.insert(lock);
                ClaimLockAcquireResult::Acquired(info)
            }
        }
    }

    pub fn renew(&self, request: ClaimLockRenewRequest) -> ClaimLockRenewResult {
        self.renew_at(request, ClaimLockClock::now())
    }

    pub fn renew_at(&self, request: ClaimLockRenewRequest, clock: ClaimLockClock) -> ClaimLockRenewResult {
        let Some(mut entry) = self.locks.get_mut(&request.memory_id) else {
            return ClaimLockRenewResult::NotHeld;
        };

        if !entry.is_held_by(&request.harness, &request.session_id) || !entry.is_live_at(clock.instant) {
            return ClaimLockRenewResult::NotHeld;
        }

        entry.renew(request.ttl, clock);
        ClaimLockRenewResult::Renewed(entry.info())
    }

    pub fn release(&self, memory_id: &str, harness: &str, session_id: &str) -> Option<ClaimLockInfo> {
        self.locks.remove_if(memory_id, |_, entry| entry.is_held_by(harness, session_id)).map(|(_, entry)| entry.info())
    }

    pub fn release_all_held_by(&self, harness: &str, session_id: &str) -> Vec<ClaimLockInfo> {
        let memory_ids = self
            .locks
            .iter()
            .filter(|entry| entry.is_held_by(harness, session_id))
            .map(|entry| entry.memory_id.clone())
            .collect::<Vec<_>>();

        memory_ids.into_iter().filter_map(|memory_id| self.release(&memory_id, harness, session_id)).collect()
    }

    pub fn restore(&self, info: ClaimLockInfo) -> Option<ClaimLockInfo> {
        let now = ClaimLockClock::now();
        let Ok(remaining) = (info.expires_at - now.utc).to_std() else {
            return None;
        };
        if remaining.is_zero() {
            return None;
        }

        let restored = ClaimLockEntry::new(
            ClaimLockAcquireRequest::new(
                info.memory_id.as_str(),
                info.holder_session_id.as_str(),
                info.holder_harness.as_str(),
                remaining,
            ),
            now,
        );
        let restored_info = restored.info();

        match self.locks.entry(info.memory_id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(restored);
                Some(restored_info)
            }
            Entry::Occupied(mut entry) => {
                if !entry.get().is_live_at(now.instant)
                    || entry.get().is_held_by(&info.holder_harness, &info.holder_session_id)
                {
                    entry.insert(restored);
                    Some(restored_info)
                } else {
                    None
                }
            }
        }
    }

    pub fn sweep_expired_at(&self, now: Instant) -> Vec<ClaimLockInfo> {
        let memory_ids = self
            .locks
            .iter()
            .filter(|entry| !entry.is_live_at(now))
            .map(|entry| entry.memory_id.clone())
            .collect::<Vec<_>>();

        memory_ids
            .into_iter()
            .filter_map(|memory_id| {
                self.locks.remove_if(&memory_id, |_, entry| !entry.is_live_at(now)).map(|(_, entry)| entry.info())
            })
            .collect()
    }

    pub fn get(&self, memory_id: &str) -> Option<ClaimLockInfo> {
        self.get_at(memory_id, Instant::now())
    }

    pub fn get_at(&self, memory_id: &str, now: Instant) -> Option<ClaimLockInfo> {
        self.live_lock_at(memory_id, now).map(|entry| entry.info())
    }

    pub fn active_locks(&self) -> Vec<ClaimLockInfo> {
        self.active_locks_at(Instant::now())
    }

    pub fn active_locks_at(&self, now: Instant) -> Vec<ClaimLockInfo> {
        self.locks.iter().filter(|entry| entry.is_live_at(now)).map(|entry| entry.info()).collect()
    }

    fn live_lock_at(&self, memory_id: &str, now: Instant) -> Option<ClaimLockEntry> {
        let entry = self.locks.get(memory_id)?;
        entry.is_live_at(now).then(|| entry.clone())
    }
}

fn contention(
    memory_id: String,
    holder: ClaimLockInfo,
    contender_harness: String,
    contender_session_id: String,
) -> ClaimLockContention {
    let holder_label = holder_label(&holder.holder_harness, &holder.holder_session_id);
    ClaimLockContention {
        warning_code: CLAIM_LOCK_CONTENTION_CODE,
        message: format!(
            "Memory {memory_id} has an active claim lock held by {holder_label}. This lock is advisory; your write has not been blocked, but you should coordinate with that session before proceeding."
        ),
        memory_id,
        holder,
        contender_harness,
        contender_session_id,
    }
}

fn holder_label(harness: &str, session_id: &str) -> String {
    format!("{harness}:{session_id}")
}

fn expires_at_utc(now: DateTime<Utc>, ttl: Duration) -> DateTime<Utc> {
    let Ok(ttl) = chrono::Duration::from_std(ttl) else {
        return DateTime::<Utc>::MAX_UTC;
    };

    now.checked_add_signed(ttl).unwrap_or(DateTime::<Utc>::MAX_UTC)
}
