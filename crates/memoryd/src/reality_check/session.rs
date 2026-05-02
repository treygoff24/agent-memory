use chrono::{DateTime, Utc};
use memory_substrate::{MemoryId, MemoryStatus, RecallIndexQuery, Scope, Substrate, SubstrateResult};

use crate::protocol::{RealityCheckCompletion, RealityCheckItem, RealityCheckResponse, RespondRefusalKind};
use crate::reality_check::{score_memories_at, ScoredMemory, ScoringConfig, DEFAULT_TOP_N};
use crate::state::{DaemonState, RcPendingCache, RcSessionState, RcSessionStore};

pub struct RcSessionHandler<'a> {
    substrate: &'a Substrate,
    store: RcSessionStore,
}

pub struct RcRunRequest {
    pub requested_session_id: Option<String>,
    pub namespace: Option<String>,
    pub limit: Option<usize>,
    pub now: DateTime<Utc>,
}

struct PendingForSessionRequest {
    session: RcSessionState,
    namespace: Option<String>,
    limit: Option<usize>,
    now: DateTime<Utc>,
}

impl<'a> RcSessionHandler<'a> {
    pub fn new(substrate: &'a Substrate) -> Self {
        Self { substrate, store: RcSessionStore::new(&substrate.roots().runtime) }
    }

    pub async fn list(
        &self,
        namespace: Option<String>,
        limit: Option<usize>,
        now: DateTime<Utc>,
    ) -> SubstrateResult<RealityCheckResponse> {
        let RcSessionItems { items, total_scored } = self.scored_items(namespace, limit, now).await?;
        Ok(RealityCheckResponse::Pending {
            session_id: None,
            items,
            total_scored,
            last_completed_at: DaemonState::load(&self.substrate.roots().runtime).reality_check.last_completed_at,
        })
    }

    pub async fn run(&self, request: RcRunRequest) -> Result<RealityCheckResponse, std::io::Error> {
        let RcRunRequest { requested_session_id, namespace, limit, now } = request;
        if let Some(session) = self.load_resumable_session(requested_session_id.as_deref(), now)? {
            return self.pending_for_session(PendingForSessionRequest { session, namespace, limit, now }).await;
        }

        let scored =
            self.scored_items(namespace, limit, now).await.map_err(|error| std::io::Error::other(error.to_string()))?;
        let session_id = requested_session_id.unwrap_or_else(|| mint_session_id(now));
        let session = RcSessionState {
            session_id: session_id.clone(),
            started_at: now,
            items_total: scored.items.len(),
            items_remaining: scored.items.iter().map(|item| item.memory_id.as_str().to_owned()).collect(),
            ..RcSessionState::default()
        };
        self.store.save(&session)?;
        RcPendingCache {
            computed_at: now,
            items: scored.items.iter().filter_map(|item| serde_json::to_value(item).ok()).collect(),
            ..RcPendingCache::default()
        }
        .save(&self.substrate.roots().runtime)?;

        Ok(RealityCheckResponse::Pending {
            session_id: Some(session_id),
            items: scored.items,
            total_scored: scored.total_scored,
            last_completed_at: DaemonState::load(&self.substrate.roots().runtime).reality_check.last_completed_at,
        })
    }

    pub fn load_session_for_response(
        &self,
        session_id: &str,
        memory_id: &MemoryId,
        now: DateTime<Utc>,
    ) -> Result<RcSessionState, Box<RealityCheckResponse>> {
        let session = self.store.load_if_recent(now).map_err(|error| {
            Box::new(refused(session_id, memory_id, error.to_string(), RespondRefusalKind::SessionExpired))
        })?;
        match session {
            Some(session)
                if session.session_id == session_id
                    && session.items_remaining.iter().any(|id| id == memory_id.as_str()) =>
            {
                Ok(session)
            }
            _ => Err(Box::new(refused(
                session_id,
                memory_id,
                "reality check session expired or item is not pending",
                RespondRefusalKind::SessionExpired,
            ))),
        }
    }

    pub async fn advance(&self, request: RcAdvanceRequest) -> Result<RealityCheckResponse, std::io::Error> {
        let RcAdvanceRequest { mut session, memory_id, advance, now } = request;
        session.items_remaining.retain(|id| id != memory_id.as_str());
        match advance {
            RcSessionAdvance::Reviewed => push_unique(&mut session.items_reviewed, memory_id.as_str()),
            RcSessionAdvance::Deferred => push_unique(&mut session.items_deferred, memory_id.as_str()),
        }
        session.current_index = session.items_reviewed.len() + session.items_deferred.len();

        let deferred = session.items_deferred.len();
        if session.items_remaining.is_empty() {
            let completed_at = now;
            let reviewed = session.items_reviewed.len();
            let mut daemon_state = DaemonState::load(&self.substrate.roots().runtime);
            daemon_state.reality_check.last_completed_at = Some(completed_at);
            daemon_state.save(&self.substrate.roots().runtime)?;
            self.store.delete()?;
            RcPendingCache::delete(&self.substrate.roots().runtime)?;
            return Ok(RealityCheckResponse::RespondAccepted {
                session_id: session.session_id,
                memory_id,
                next_item: None,
                completion: RealityCheckCompletion::Complete { reviewed, deferred, completed_at },
            });
        }

        self.store.save(&session)?;
        let next_item = self
            .next_item_for_remaining(&session, now)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(RealityCheckResponse::RespondAccepted {
            session_id: session.session_id,
            memory_id,
            next_item,
            completion: RealityCheckCompletion::Progress { remaining: session.items_remaining.len(), deferred },
        })
    }

    async fn pending_for_session(
        &self,
        request: PendingForSessionRequest,
    ) -> Result<RealityCheckResponse, std::io::Error> {
        let PendingForSessionRequest { session, namespace, limit, now } = request;
        let scored =
            self.scored_items(namespace, limit, now).await.map_err(|error| std::io::Error::other(error.to_string()))?;
        let remaining = scored
            .items
            .into_iter()
            .filter(|item| session.items_remaining.iter().any(|id| id == item.memory_id.as_str()))
            .collect::<Vec<_>>();
        Ok(RealityCheckResponse::Pending {
            session_id: Some(session.session_id),
            items: remaining,
            total_scored: scored.total_scored,
            last_completed_at: DaemonState::load(&self.substrate.roots().runtime).reality_check.last_completed_at,
        })
    }

    async fn next_item_for_remaining(
        &self,
        session: &RcSessionState,
        now: DateTime<Utc>,
    ) -> SubstrateResult<Option<RealityCheckItem>> {
        let scored = self.scored_items(None, None, now).await?;
        Ok(scored.items.into_iter().find(|item| session.items_remaining.iter().any(|id| id == item.memory_id.as_str())))
    }

    async fn scored_items(
        &self,
        namespace: Option<String>,
        limit: Option<usize>,
        now: DateTime<Utc>,
    ) -> SubstrateResult<RcSessionItems> {
        let rows = self
            .substrate
            .query_recall_index_including_metadata_only(RecallIndexQuery {
                namespace_prefix: namespace,
                statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned],
                passive_recall_only: true,
                ..RecallIndexQuery::default()
            })
            .await?;
        let total_scored = rows.len();
        let config = ScoringConfig::with_top_n(limit.unwrap_or(DEFAULT_TOP_N));
        let scored = score_memories_at(&rows, self.substrate, &config, now)?;
        Ok(RcSessionItems { total_scored, items: scored.into_iter().map(scored_item_to_wire).collect() })
    }

    fn load_resumable_session(
        &self,
        requested_session_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<RcSessionState>, std::io::Error> {
        let Some(session) = self.store.load_if_recent(now)? else {
            return Ok(None);
        };
        if requested_session_id.is_none_or(|id| id == session.session_id) {
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RcSessionAdvance {
    Reviewed,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcAdvanceRequest {
    pub session: RcSessionState,
    pub memory_id: MemoryId,
    pub advance: RcSessionAdvance,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RcSessionItems {
    pub items: Vec<RealityCheckItem>,
    pub total_scored: usize,
}

fn scored_item_to_wire(item: ScoredMemory) -> RealityCheckItem {
    RealityCheckItem {
        memory_id: item.memory_id,
        title: if item.encrypted { String::new() } else { item.summary },
        namespace: namespace_label(item.scope, item.canonical_namespace_id.as_deref()),
        status: item.status,
        sensitivity: Some(item.sensitivity),
        score: item.score,
        component_scores: item.component_scores,
        encrypted: item.encrypted,
        last_observed_at: item.last_observed_at,
        recall_count_30d: item.recall_count_30d,
        last_recalled_at: item.last_recalled_at,
    }
}

fn namespace_label(scope: Scope, canonical_namespace_id: Option<&str>) -> String {
    match scope {
        Scope::User => "me".to_owned(),
        Scope::Project => canonical_namespace_id.map_or_else(|| "project".to_owned(), |id| format!("project:{id}")),
        Scope::Org => canonical_namespace_id.map_or_else(|| "org".to_owned(), |id| format!("org:{id}")),
        Scope::Agent | Scope::Subagent => "agent".to_owned(),
    }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

fn refused(
    session_id: &str,
    memory_id: &MemoryId,
    reason: impl Into<String>,
    kind: RespondRefusalKind,
) -> RealityCheckResponse {
    RealityCheckResponse::RespondRefused {
        session_id: session_id.to_owned(),
        memory_id: memory_id.clone(),
        reason: reason.into(),
        kind,
    }
}

fn mint_session_id(now: DateTime<Utc>) -> String {
    format!("rcs_{}", now.timestamp_micros())
}
