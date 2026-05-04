use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use memoryd::protocol::{
    MemoryId, RealityCheckAction as ProtocolRealityCheckAction, RealityCheckRequest, RecallHitsResponse,
    RequestPayload, ResponsePayload, StatusResponse,
};

use crate::app::{DaemonCall, RealityCheckAction, ReviewAction};

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

    pub async fn status(&self) -> Result<StatusResponse> {
        match self.request("memoryd-tui-status", RequestPayload::Status).await?.result {
            memoryd::protocol::ResponseResult::Success(ResponsePayload::Status(status)) => Ok(status),
            memoryd::protocol::ResponseResult::Success(other) => {
                Err(anyhow!("daemon returned unexpected response: {other:?}"))
            }
            memoryd::protocol::ResponseResult::Error(error) => {
                Err(anyhow!("daemon error {}: {}", error.code, error.message))
            }
        }
    }

    pub async fn trust_artifact(&self, id: &str) -> Result<memoryd::trust_artifact::TrustArtifact> {
        match self
            .request(format!("memoryd-tui-trust-artifact-{id}"), RequestPayload::TrustArtifact { id: id.to_owned() })
            .await?
            .result
        {
            memoryd::protocol::ResponseResult::Success(ResponsePayload::TrustArtifact(artifact)) => Ok(*artifact),
            memoryd::protocol::ResponseResult::Success(other) => {
                Err(anyhow!("daemon returned unexpected response: {other:?}"))
            }
            memoryd::protocol::ResponseResult::Error(error) => {
                Err(anyhow!("daemon error {}: {}", error.code, error.message))
            }
        }
    }

    pub async fn recall_hits(&self, limit: usize) -> Result<RecallHitsResponse> {
        match self
            .request("memoryd-tui-recall-hits", RequestPayload::RecallHits { since: None, limit: Some(limit) })
            .await?
            .result
        {
            memoryd::protocol::ResponseResult::Success(ResponsePayload::RecallHits(response)) => Ok(response),
            memoryd::protocol::ResponseResult::Success(other) => {
                Err(anyhow!("daemon returned unexpected response: {other:?}"))
            }
            memoryd::protocol::ResponseResult::Error(error) => {
                Err(anyhow!("daemon error {}: {}", error.code, error.message))
            }
        }
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
            ReviewAction::Quarantine
            | ReviewAction::Edit
            | ReviewAction::AcceptLocal
            | ReviewAction::AcceptRemote
            | ReviewAction::Merge => {
                return Err(anyhow!("review action {action:?} is not supported by the daemon protocol"));
            }
        };

        let result =
            self.request(format!("memoryd-tui-review-{memory_id}-{}", review_action_label(action)), request).await?;
        match (action, result.result) {
            (ReviewAction::Approve, memoryd::protocol::ResponseResult::Success(ResponsePayload::ReviewApprove(_)))
            | (ReviewAction::Reject, memoryd::protocol::ResponseResult::Success(ResponsePayload::ReviewReject(_)))
            | (
                ReviewAction::Forget,
                memoryd::protocol::ResponseResult::Success(ResponsePayload::GovernanceForget(_)),
            ) => Ok(()),
            (_, memoryd::protocol::ResponseResult::Success(other)) => {
                Err(anyhow!("daemon returned unexpected response for review action {action:?}: {other:?}"))
            }
            (_, memoryd::protocol::ResponseResult::Error(error)) => {
                Err(anyhow!("daemon error {}: {}", error.code, error.message))
            }
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
            memory_id: memory_id.clone(),
            action,
        });

        let result =
            self.request(format!("memoryd-tui-reality-check-{session_id}-{}", memory_id.as_str()), request).await?;
        match result.result {
            memoryd::protocol::ResponseResult::Success(ResponsePayload::RealityCheck(_)) => Ok(()),
            memoryd::protocol::ResponseResult::Success(other) => {
                Err(anyhow!("daemon returned unexpected response for Reality Check action: {other:?}"))
            }
            memoryd::protocol::ResponseResult::Error(error) => {
                Err(anyhow!("daemon error {}: {}", error.code, error.message))
            }
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

fn review_action_label(action: &ReviewAction) -> &'static str {
    match action {
        ReviewAction::Approve => "approve",
        ReviewAction::Reject => "reject",
        ReviewAction::Forget => "forget",
        ReviewAction::Quarantine => "quarantine",
        ReviewAction::Edit => "edit",
        ReviewAction::AcceptLocal => "accept-local",
        ReviewAction::AcceptRemote => "accept-remote",
        ReviewAction::Merge => "merge",
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
        RealityCheckAction::Correct => {
            Err(anyhow!("Reality Check correct requires replacement text and is not supported by the TUI yet"))
        }
    }
}
