use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::{ClaimLockInfo, PeerSessionStatus, RequestPayload, ResponsePayload, ResponseResult};
use serde::Serialize;

use crate::routes::status::{daemon_error, SyncStatus};
use crate::server::{backend_unavailable, WebState};

#[derive(Clone, Debug, Serialize)]
pub struct SyncDashboardResponse {
    pub sync: SyncStatus,
    pub last_commit: Option<String>,
    pub peer_presence: PeerPresenceSummary,
    pub claim_locks: ClaimLockSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerPresenceSummary {
    pub coordination_level: u8,
    pub active_session_count: usize,
    pub active_sessions: Vec<PeerSessionStatus>,
    pub recent_delivery_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClaimLockSummary {
    pub active_count: usize,
    pub locks: Vec<ClaimLockInfo>,
}

pub async fn sync_dashboard(State(state): State<WebState>) -> impl IntoResponse {
    if let Some(data) = state.dashboard_data() {
        return Json(SyncDashboardResponse {
            sync: data.status.sync.clone(),
            last_commit: Some("fixture-sync-commit".to_owned()),
            peer_presence: PeerPresenceSummary {
                coordination_level: 3,
                active_session_count: data.status.active_sessions.len(),
                active_sessions: data
                    .status
                    .active_sessions
                    .iter()
                    .map(|session| PeerSessionStatus {
                        session_id: session.session_id.clone(),
                        harness: session.harness.clone(),
                        namespace: "project:agent-memory".to_owned(),
                        salient_entities: Vec::new(),
                        started_at: None,
                        last_heartbeat_age_seconds: 0,
                    })
                    .collect(),
                recent_delivery_count: data.status.recall.peer_update_total as usize,
            },
            claim_locks: ClaimLockSummary {
                active_count: data
                    .audit_artifact
                    .sync_state
                    .claim_lock_status
                    .iter()
                    .filter(|status| status.starts_with("held by "))
                    .count(),
                locks: Vec::new(),
            },
        })
        .into_response();
    }

    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("sync_dashboard").into_response();
    };

    let status = match memoryd::client::request(socket_path, "web-sync-dashboard-status", RequestPayload::Status).await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::Status(status)) => {
                crate::routes::status::StatusDashboardResponse::from_daemon(status).sync
            }
            ResponseResult::Error(error) => {
                return daemon_error("sync_dashboard", error.code, error.message).into_response()
            }
            other => {
                return daemon_error("sync_dashboard", "unexpected_response", format!("{other:?}")).into_response()
            }
        },
        Err(error) => return daemon_error("sync_dashboard", "daemon_unavailable", error.to_string()).into_response(),
    };

    match memoryd::client::request(socket_path, "web-sync-dashboard-peers", RequestPayload::PeerStatus).await {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::PeerStatus(peer_status)) => {
                let active_count = peer_status.claim_locks.len();
                Json(SyncDashboardResponse {
                    sync: status,
                    last_commit: None,
                    peer_presence: PeerPresenceSummary {
                        coordination_level: peer_status.coordination_level,
                        active_session_count: peer_status.active_sessions.len(),
                        active_sessions: peer_status.active_sessions,
                        recent_delivery_count: peer_status.recent_deliveries.len(),
                    },
                    claim_locks: ClaimLockSummary { active_count, locks: peer_status.claim_locks },
                })
                .into_response()
            }
            ResponseResult::Error(error) => daemon_error("sync_dashboard", error.code, error.message).into_response(),
            other => daemon_error("sync_dashboard", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("sync_dashboard", "daemon_unavailable", error.to_string()).into_response(),
    }
}
