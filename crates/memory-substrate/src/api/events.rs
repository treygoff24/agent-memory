//! Event recording and the derived events-log mirror surfaces.

use super::*;

impl Substrate {
    /// Read a bounded, kind-filtered page of events from the derived SQLite
    /// mirror (newest-first), instead of parsing the entire canonical JSONL log.
    ///
    /// The mirror is derived; the canonical JSONL log remains the source of
    /// truth. `kind_labels` filters on the stored kind column. The page is scoped
    /// to the local device (peer-device events arrive via sync but are not part of
    /// this device's dashboard/ROI views). `since_event_id` is a chronological
    /// cursor: the page returns rows strictly older than that event in canonical
    /// `(ts, seq, event_id)` order.
    pub fn events_log_page(
        &self,
        kind_labels: Option<&[&str]>,
        since_event_id: Option<&str>,
        limit: usize,
    ) -> SubstrateResult<Vec<crate::index::MirrorEvent>> {
        let page =
            crate::index::EventsLogPage { kind_labels, device: Some(self.device_id.as_str()), since_event_id, limit };
        lock_index(&self.index).events_log_page(&page)
    }

    /// Read events from the derived SQLite mirror within a time window, optionally
    /// kind-restricted, instead of parsing and filtering the whole JSONL log.
    /// Scoped to the local device (see [`Substrate::events_log_page`]).
    pub fn events_log_window(
        &self,
        kind_labels: Option<&[&str]>,
        since: DateTime<Utc>,
    ) -> SubstrateResult<Vec<crate::index::MirrorEvent>> {
        lock_index(&self.index).events_log_window(kind_labels, Some(self.device_id.as_str()), since)
    }

    /// Most recent event timestamp for a given kind label from the derived mirror.
    pub fn latest_event_ts_for_kind(&self, kind_label: &str) -> SubstrateResult<Option<DateTime<Utc>>> {
        lock_index(&self.index).latest_event_ts_for_kind(kind_label)
    }

    /// Timestamp of a single event looked up by canonical event id in the mirror.
    pub fn event_ts_by_id(&self, event_id: &str) -> SubstrateResult<Option<DateTime<Utc>>> {
        lock_index(&self.index).event_ts_by_id(event_id)
    }

    /// Read event log.
    pub fn events(&self) -> std::io::Result<Vec<Event>> {
        read_events(&self.event_log)
    }

    /// Rebuild the derived SQLite events-log mirror from canonical JSONL logs.
    pub fn doctor_reindex_events_log(&self) -> SubstrateResult<usize> {
        let events = self.read_all_event_logs().map_err(|source| SubstrateError::Io {
            path: self.roots.repo.join("events").display().to_string(),
            source,
        })?;
        self.index
            .lock()
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?
            .rebuild_events_log_mirror(&events)?;
        Ok(events.len())
    }

    /// Return derived SQLite mirror lag against canonical JSONL event logs.
    pub fn events_log_mirror_health(&self) -> SubstrateResult<EventsLogMirrorHealth> {
        let events = self.read_all_event_logs().map_err(|source| SubstrateError::Io {
            path: self.roots.repo.join("events").display().to_string(),
            source,
        })?;
        self.index
            .lock()
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?
            .events_log_mirror_health(&events)
            .map_err(Into::into)
    }

    /// Guard the persisted event-sequence state before a best-effort append.
    ///
    /// In the durable tiers every event is allocated through
    /// [`reserve_event_sequence`], so a cheap "state file exists/valid" guard
    /// ([`ensure_event_sequence_state`]) is sufficient — the authoritative
    /// high-water reconcile happened at substrate open and `reserve` keeps
    /// `event-seq.json` monotonic thereafter.
    ///
    /// `DurabilityTier::BestEffort` is the exception: there [`Self::record_event`]
    /// allocates from the in-memory `best_effort_event_seq` counter and appends
    /// seqs that `event-seq.json` does not track, so a cheap guard would let a
    /// later `reserve` reuse a seq the atomic-counter path already wrote. In that
    /// tier we keep the pre-refactor full-log high-water reconcile
    /// ([`sync_event_sequence_state`]) so the two allocators stay disjoint. The
    /// full-log scan is confined to this degraded tier; the hot path in the
    /// durable tiers stays scan-free.
    fn guard_event_sequence_state(&self, device: &DeviceId) -> std::io::Result<()> {
        if matches!(self.durability, DurabilityTier::BestEffort) {
            sync_event_sequence_state(&self.roots.runtime, &self.event_log, device)
        } else {
            ensure_event_sequence_state(&self.roots.runtime, &self.event_log, device)
        }
    }

    /// Record a best-effort observability event through Stream A's central
    /// sequence allocator and incremental SQLite mirror path.
    pub fn record_event_best_effort(&self, kind: EventKind) -> std::io::Result<()> {
        let device = DeviceId::try_new(&self.device_id)
            .map_err(|err| std::io::Error::other(format!("invalid device_id in Substrate: {err}")))?;
        self.guard_event_sequence_state(&device)?;
        let event = self.build_recorded_event(kind, &new_operation_id())?;
        self.best_effort_event_seq.fetch_max(event.seq.saturating_add(1), Ordering::Relaxed);
        self.append_event_and_mirror(&event, true)
    }

    /// Record that a memory was included in a rendered recall response.
    pub fn record_recall_hit(&self, id: MemoryId) -> std::io::Result<()> {
        self.record_event_best_effort(EventKind::RecallHit { id, recalled_at: Utc::now() })
    }

    /// Record recall hits for many memories in one pass.
    ///
    /// The sequence-state guard runs once for the whole batch instead of once
    /// per id (the prior `emit_recall_hits` loop drove `record_recall_hit` —
    /// hence the full sequence-sync — once per recalled memory). Each id still
    /// reserves its own sequence number and appends one canonical event; errors
    /// are returned per id so callers can log without aborting the batch.
    pub fn record_recall_hits(&self, ids: &[MemoryId]) -> Vec<(MemoryId, std::io::Error)> {
        if ids.is_empty() {
            return Vec::new();
        }
        let device = match DeviceId::try_new(&self.device_id) {
            Ok(device) => device,
            Err(err) => {
                let error = std::io::Error::other(format!("invalid device_id in Substrate: {err}"));
                // Surface the same failure once per id so the caller's logging is
                // unchanged versus the prior per-id loop.
                return ids
                    .iter()
                    .map(|id| (id.clone(), std::io::Error::new(error.kind(), error.to_string())))
                    .collect();
            }
        };
        if let Err(err) = self.guard_event_sequence_state(&device) {
            return ids.iter().map(|id| (id.clone(), std::io::Error::new(err.kind(), err.to_string()))).collect();
        }
        let recalled_at = Utc::now();
        let seqs = match reserve_event_sequences(&self.roots.runtime, &self.event_log, &device, ids.len()) {
            Ok(seqs) => seqs,
            Err(err) => return ids.iter().map(|id| (id.clone(), copy_io_error(&err))).collect(),
        };
        let events = ids
            .iter()
            .zip(seqs)
            .map(|(id, seq)| Event {
                schema: crate::SUBSTRATE_SCHEMA_VERSION,
                id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
                at: Utc::now(),
                device: device.clone(),
                seq,
                operation_id: Some(new_operation_id()),
                kind: EventKind::RecallHit { id: id.clone(), recalled_at },
                crc32c: 0,
            })
            .collect::<Vec<_>>();
        for event in &events {
            self.best_effort_event_seq.fetch_max(event.seq.saturating_add(1), Ordering::Relaxed);
        }
        if let Err(err) = self.append_events_and_mirror(&events, true) {
            return ids.iter().map(|id| (id.clone(), copy_io_error(&err))).collect();
        }
        Vec::new()
    }

    fn append_events_and_mirror(&self, events: &[Event], best_effort: bool) -> std::io::Result<()> {
        if best_effort {
            append_events_best_effort(&self.event_log, events)?;
        } else {
            append_events(&self.event_log, events)?;
        }
        self.mirror_events_fail_soft(events);
        Ok(())
    }

    fn mirror_events_fail_soft(&self, events: &[Event]) {
        let mut index = lock_index(&self.index);
        for event in events {
            if let Err(err) = index.mirror_event(event) {
                tracing::warn!(event_id = event.id.as_str(), "events_log SQLite mirror write failed: {err}");
            }
        }
    }

    /// Record that encrypted content was intentionally revealed without
    /// persisting the revealed plaintext.
    pub fn record_encrypted_content_revealed(&self, id: MemoryId, reason: String) -> std::io::Result<()> {
        self.record_event(EventKind::EncryptedContentRevealed { id, reason }, &new_operation_id())
    }

    pub(super) fn build_recorded_event(&self, kind: EventKind, operation_id: &OperationId) -> std::io::Result<Event> {
        let device = DeviceId::try_new(&self.device_id)
            .map_err(|err| std::io::Error::other(format!("invalid device_id in Substrate: {err}")))?;
        let seq = reserve_event_sequence(&self.roots.runtime, &self.event_log, &device)?;
        Ok(Event {
            schema: crate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
            at: Utc::now(),
            device,
            seq,
            operation_id: Some(operation_id.clone()),
            kind,
            crc32c: 0,
        })
    }

    pub(super) fn record_event(&self, kind: EventKind, operation_id: &OperationId) -> std::io::Result<()> {
        if matches!(self.durability, DurabilityTier::BestEffort) {
            let device = DeviceId::try_new(&self.device_id).map_err(std::io::Error::other)?;
            let event = Event {
                schema: crate::SUBSTRATE_SCHEMA_VERSION,
                id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
                at: Utc::now(),
                device,
                seq: self.best_effort_event_seq.fetch_add(1, Ordering::Relaxed),
                operation_id: Some(operation_id.clone()),
                kind,
                crc32c: 0,
            };
            return self.append_event_and_mirror(&event, true);
        }
        let event = self.build_recorded_event(kind, operation_id)?;
        self.append_event_and_mirror(&event, false)
    }

    pub(super) fn append_event_and_mirror(&self, event: &Event, best_effort: bool) -> std::io::Result<()> {
        if best_effort {
            append_event_best_effort(&self.event_log, event)?;
        } else {
            append_event(&self.event_log, event)?;
        }
        self.mirror_event_fail_soft(event);
        Ok(())
    }

    fn mirror_event_fail_soft(&self, event: &Event) {
        let mut index = lock_index(&self.index);
        if let Err(err) = index.mirror_event(event) {
            tracing::warn!(event_id = event.id.as_str(), "events_log SQLite mirror write failed: {err}");
        }
    }

    fn read_all_event_logs(&self) -> std::io::Result<Vec<Event>> {
        read_all_event_logs_from_repo(&self.roots.repo)
    }
}
