use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use memoryd::protocol::{
    ConflictsListResponse, EventsLogPageResponse, GovernancePolicySnapshot, InspectEntitiesResponse, MemoryId,
    NamespaceTreeResponse, RealityCheckAction as ProtocolRealityCheckAction, RealityCheckRequest, RecallHitsResponse,
    RequestPayload, ResponsePayload, ResponseResult, ReviewQueueResponse, StatusResponse,
};

use crate::app::{DaemonCall, DaemonSnapshot, RealityCheckAction, ReviewAction};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self { socket_path: socket_path.into() }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub async fn fetch_snapshot(&self) -> Result<DaemonSnapshot> {
        let status = self.status().await?;
        let mut snapshot = DaemonSnapshot::empty();
        snapshot.daemon_state = status.state;
        if let Ok(review) = self.review_queue(50).await {
            snapshot.review_queue = review
                .items
                .into_iter()
                .map(|item| crate::app::ReviewQueueRow {
                    id: item.id,
                    title: item.summary,
                    namespace: "review".to_string(),
                    status: item.status,
                    reason: item.reason,
                    body: item.body,
                    body_truncated: item.body_truncated,
                })
                .collect();
        }
        if let Ok(conflicts) = self.conflicts(50).await {
            snapshot.conflicts = conflicts
                .conflicts
                .into_iter()
                .map(|item| crate::app::ConflictRow {
                    id: item.id.to_string(),
                    title: item.summary,
                    namespace: "conflict".to_string(),
                    reason: item.reason,
                })
                .collect();
        }
        if let Ok(recall) = self.recall_hits(50).await {
            snapshot.recall = recall
                .hits
                .into_iter()
                .map(|hit| crate::app::RecallHitRow {
                    id: hit.memory_id.to_string(),
                    title: hit.summary.unwrap_or_else(|| "recalled memory".to_string()),
                    namespace: "recall".to_string(),
                    age: hit.recalled_at.format("%H:%M").to_string(),
                })
                .collect();
        }
        Ok(snapshot)
    }

    pub async fn status(&self) -> Result<StatusResponse> {
        expect_response(self.request("memoryd-tui-status", RequestPayload::Status).await?, "status")
    }

    pub async fn review_queue(&self, limit: usize) -> Result<ReviewQueueResponse> {
        expect_response(
            self.request("memoryd-tui-review-queue", RequestPayload::ReviewQueue { limit: Some(limit) }).await?,
            "review_queue",
        )
    }

    pub async fn conflicts(&self, limit: usize) -> Result<ConflictsListResponse> {
        expect_response(
            self.request("memoryd-tui-conflicts", RequestPayload::ConflictsList { limit: Some(limit) }).await?,
            "conflicts_list",
        )
    }

    pub async fn recall_hits(&self, limit: usize) -> Result<RecallHitsResponse> {
        expect_response(
            self.request("memoryd-tui-recall-hits", RequestPayload::RecallHits { since: None, limit: Some(limit) })
                .await?,
            "recall_hits",
        )
    }

    pub async fn reality_check_session_progress(&self, session_id: &str) -> Result<crate::state::RealityCheckState> {
        let response: memoryd::protocol::RealityCheckResponse = expect_response(
            self.request(
                format!("memoryd-tui-reality-check-progress-{session_id}"),
                RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::List {
                    namespace: None,
                    limit: Some(50),
                }),
            )
            .await?,
            "reality_check",
        )?;
        let memoryd::protocol::RealityCheckResponse::Pending { session_id, items, .. } = response else {
            return Ok(crate::state::RealityCheckState::default());
        };
        Ok(crate::state::RealityCheckState {
            active_session_id: session_id,
            items_total: items.len(),
            items_reviewed: 0,
            current_title: items.first().map(|item| item.title.clone()),
            ..Default::default()
        })
    }

    pub async fn inspect_entities(&self, limit: usize) -> Result<InspectEntitiesResponse> {
        expect_response(
            self.request("memoryd-tui-entities", RequestPayload::InspectEntities { limit: Some(limit), prefix: None })
                .await?,
            "inspect_entities",
        )
    }

    pub async fn events_log_page(&self, limit: usize) -> Result<EventsLogPageResponse> {
        expect_response(
            self.request("memoryd-tui-events", RequestPayload::EventsLogPage { since: None, limit, kind_filter: None })
                .await?,
            "events_log_page",
        )
    }

    pub async fn namespace_tree(&self) -> Result<NamespaceTreeResponse> {
        expect_response(
            self.request("memoryd-tui-namespace", RequestPayload::NamespaceTree { root: None, depth: Some(2) }).await?,
            "namespace_tree",
        )
    }

    pub async fn governance_policy_dump(&self) -> Result<GovernancePolicySnapshot> {
        expect_response(
            self.request("memoryd-tui-policy", RequestPayload::GovernancePolicyDump).await?,
            "governance_policy_dump",
        )
    }

    pub async fn trust_artifact(&self, id: &str) -> Result<memoryd::trust_artifact::TrustArtifact> {
        expect_response::<Box<memoryd::trust_artifact::TrustArtifact>>(
            self.request(
                format!("memoryd-tui-trust-artifact-{id}"),
                RequestPayload::TrustArtifact { id: id.to_owned() },
            )
            .await?,
            "trust_artifact",
        )
        .map(|artifact| *artifact)
    }

    pub async fn dispatch_daemon_call(&self, call: &DaemonCall) -> Result<()> {
        match call {
            DaemonCall::Review { action, memory_id } => self.review_action(action, memory_id).await,
            DaemonCall::RealityCheck { action, session_id, memory_id } => {
                self.reality_check_action(action, session_id, memory_id).await
            }
            DaemonCall::ForceRefresh => self.status().await.map(|_| ()),
        }
    }

    pub async fn review_action(&self, action: &ReviewAction, memory_id: &str) -> Result<()> {
        let request = match action {
            ReviewAction::Approve => RequestPayload::ReviewApprove { id: memory_id.to_owned() },
            ReviewAction::Reject => RequestPayload::ReviewReject {
                id: memory_id.to_owned(),
                reason: "rejected from memoryd-tui".to_owned(),
            },
            ReviewAction::Forget => RequestPayload::Forget {
                id: memory_id.to_owned(),
                reason: "forgotten from memoryd-tui review queue".to_owned(),
            },
        };
        let response = self.request(format!("memoryd-tui-review-{memory_id}"), request).await?;
        match response.result {
            ResponseResult::Success(_) => Ok(()),
            ResponseResult::Error(error) => Err(anyhow!("daemon error {}: {}", error.code, error.message)),
        }
    }

    pub async fn reality_check_action(
        &self,
        action: &RealityCheckAction,
        session_id: &str,
        memory_id: &str,
    ) -> Result<()> {
        let memory_id = MemoryId::try_new(memory_id.to_owned())
            .with_context(|| format!("invalid Reality Check memory id {memory_id}"))?;
        let action = protocol_reality_check_action(action)?;
        let request = RequestPayload::RealityCheck(RealityCheckRequest::Respond {
            session_id: session_id.to_owned(),
            memory_id,
            action,
        });
        let response = self.request(format!("memoryd-tui-reality-check-{session_id}"), request).await?;
        match response.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(_)) => Ok(()),
            ResponseResult::Success(other) => {
                Err(anyhow!("daemon returned unexpected response for Reality Check action: {other:?}"))
            }
            ResponseResult::Error(error) => Err(anyhow!("daemon error {}: {}", error.code, error.message)),
        }
    }

    pub async fn correct(&self, session_id: &str, memory_id: MemoryId, new_body: String) -> Result<()> {
        let request = RequestPayload::RealityCheck(RealityCheckRequest::Respond {
            session_id: session_id.to_owned(),
            memory_id,
            action: ProtocolRealityCheckAction::Correct { new_body },
        });
        let response = self.request(format!("memoryd-tui-reality-check-correct-{session_id}"), request).await?;
        match response.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(_)) => Ok(()),
            ResponseResult::Success(other) => {
                Err(anyhow!("daemon returned unexpected response for Reality Check correction: {other:?}"))
            }
            ResponseResult::Error(error) => Err(anyhow!("daemon error {}: {}", error.code, error.message)),
        }
    }

    async fn request(
        &self,
        request_id: impl Into<String>,
        request: RequestPayload,
    ) -> Result<memoryd::protocol::ResponseEnvelope> {
        memoryd::client::request(&self.socket_path, request_id, request)
            .await
            .with_context(|| format!("send daemon request through {}", self.socket_path.display()))
    }
}

fn expect_response<T>(response: memoryd::protocol::ResponseEnvelope, expected: &str) -> Result<T>
where
    T: FromPayload,
{
    match response.result {
        ResponseResult::Success(payload) => {
            T::from_payload(payload).ok_or_else(|| anyhow!("daemon returned unexpected response for {expected}"))
        }
        ResponseResult::Error(error) => Err(anyhow!("daemon error {}: {}", error.code, error.message)),
    }
}

fn protocol_reality_check_action(action: &RealityCheckAction) -> Result<ProtocolRealityCheckAction> {
    match action {
        RealityCheckAction::Confirm => Ok(ProtocolRealityCheckAction::Confirm),
        RealityCheckAction::Forget => {
            Ok(ProtocolRealityCheckAction::Forget { reason: "forgotten from memoryd-tui Reality Check".to_owned() })
        }
        RealityCheckAction::NotRelevant => Ok(ProtocolRealityCheckAction::NotRelevant),
        RealityCheckAction::SkipWeek => Ok(ProtocolRealityCheckAction::SkipThisWeek),
        RealityCheckAction::Correct { new_body } => {
            Ok(ProtocolRealityCheckAction::Correct { new_body: new_body.clone() })
        }
    }
}

trait FromPayload {
    fn from_payload(value: ResponsePayload) -> Option<Self>
    where
        Self: Sized;
}

macro_rules! impl_payload_from {
    ($type:ty, $variant:path) => {
        impl FromPayload for $type {
            fn from_payload(value: ResponsePayload) -> Option<Self> {
                match value {
                    $variant(value) => Some(value),
                    _ => None,
                }
            }
        }
    };
}

impl_payload_from!(StatusResponse, ResponsePayload::Status);
impl_payload_from!(ReviewQueueResponse, ResponsePayload::ReviewQueue);
impl_payload_from!(ConflictsListResponse, ResponsePayload::ConflictsList);
impl_payload_from!(RecallHitsResponse, ResponsePayload::RecallHits);
impl_payload_from!(InspectEntitiesResponse, ResponsePayload::InspectEntities);
impl_payload_from!(EventsLogPageResponse, ResponsePayload::EventsLogPage);
impl_payload_from!(NamespaceTreeResponse, ResponsePayload::NamespaceTree);
impl_payload_from!(GovernancePolicySnapshot, ResponsePayload::GovernancePolicyDump);
impl_payload_from!(Box<memoryd::trust_artifact::TrustArtifact>, ResponsePayload::TrustArtifact);
impl_payload_from!(memoryd::protocol::RealityCheckResponse, ResponsePayload::RealityCheck);
