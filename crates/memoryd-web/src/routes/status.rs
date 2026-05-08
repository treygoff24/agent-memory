use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use memoryd::protocol::{ResponsePayload, ResponseResult};
use serde::Serialize;
use serde_json::json;

use crate::server::{backend_unavailable, WebState};

#[derive(Clone, Debug, Serialize)]
pub struct StatusDashboardResponse {
    pub daemon: DaemonStatus,
    pub socket: String,
    pub index: IndexStatus,
    pub sync: SyncStatus,
    pub review: ReviewStatus,
    pub conflicts: u32,
    pub active_sessions: Vec<ActiveSession>,
    pub dreaming: DreamingStatus,
    pub recall: RecallStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct DaemonStatus {
    pub version: String,
    pub pid: u32,
    pub uptime_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct IndexStatus {
    pub active_memories: u64,
    pub last_reindex: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncStatus {
    pub ahead: u32,
    pub behind: u32,
    pub last_push: DateTime<Utc>,
    pub remote: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewStatus {
    pub candidate: u32,
    pub quarantined: u32,
    pub dream_low_confidence: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActiveSession {
    pub harness: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DreamingStatus {
    pub status: String,
    pub next_run: DateTime<Utc>,
    pub last_run: DreamRunSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct DreamRunSummary {
    pub at: DateTime<Utc>,
    pub promoted: u32,
    pub queued: u32,
    pub dropped: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecallStatus {
    pub startup_total: u32,
    pub delta_total: u32,
    pub peer_update_total: u32,
}

impl StatusDashboardResponse {
    pub fn fixture(now: DateTime<Utc>) -> Self {
        Self {
            daemon: DaemonStatus { version: "0.1.0-test".to_owned(), pid: 7137, uptime_seconds: 302_440 },
            socket: "ok".to_owned(),
            index: IndexStatus { active_memories: 1_204, last_reindex: now },
            sync: SyncStatus {
                ahead: 2,
                behind: 0,
                last_push: now,
                remote: "git@github.com:trey/memory.git".to_owned(),
            },
            review: ReviewStatus { candidate: 3, quarantined: 2, dream_low_confidence: 2 },
            conflicts: 1,
            active_sessions: vec![
                ActiveSession { harness: "claude-code".to_owned(), session_id: "session_claude_fixture".to_owned() },
                ActiveSession { harness: "codex-cli".to_owned(), session_id: "session_codex_fixture".to_owned() },
            ],
            dreaming: DreamingStatus {
                status: "scheduled".to_owned(),
                next_run: now + chrono::Duration::hours(15),
                last_run: DreamRunSummary { at: now - chrono::Duration::hours(9), promoted: 3, queued: 1, dropped: 0 },
            },
            recall: RecallStatus { startup_total: 42, delta_total: 119, peer_update_total: 8 },
        }
    }
}

pub async fn status(State(state): State<WebState>) -> impl IntoResponse {
    if let Some(data) = state.dashboard_data() {
        return Json(data.status.clone()).into_response();
    }
    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("status").into_response();
    };
    match memoryd::client::request(socket_path, "web-status", memoryd::protocol::RequestPayload::Status).await {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::Status(status)) => {
                Json(StatusDashboardResponse::from_daemon(status)).into_response()
            }
            ResponseResult::Error(error) => daemon_error("status", error.code, error.message).into_response(),
            other => daemon_error("status", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("status", "daemon_unavailable", error.to_string()).into_response(),
    }
}

pub async fn notifications_stream(State(state): State<WebState>) -> Response {
    let notifications = if let Some(data) = state.dashboard_data() {
        data.notifications.clone()
    } else if state.daemon_socket().is_some() {
        Vec::new()
    } else {
        return backend_unavailable("notifications_stream").into_response();
    };
    let payload = json!({
        "kind": "heartbeat",
        "notifications": notifications,
    });
    let body = format!("event: heartbeat\ndata: {payload}\n\n");
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream; charset=utf-8"));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

impl StatusDashboardResponse {
    pub(crate) fn from_daemon(status: memoryd::protocol::StatusResponse) -> Self {
        let now = Utc::now();
        Self {
            daemon: DaemonStatus {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                pid: std::process::id(),
                uptime_seconds: 0,
            },
            socket: status.state,
            index: IndexStatus { active_memories: 0, last_reindex: now },
            sync: SyncStatus { ahead: 0, behind: 0, last_push: now, remote: "daemon".to_owned() },
            review: ReviewStatus { candidate: 0, quarantined: 0, dream_low_confidence: 0 },
            conflicts: 0,
            active_sessions: Vec::new(),
            dreaming: DreamingStatus {
                status: "daemon".to_owned(),
                next_run: now,
                last_run: DreamRunSummary { at: now, promoted: 0, queued: 0, dropped: 0 },
            },
            recall: RecallStatus {
                startup_total: status.recall.startup_invoked_total as u32,
                delta_total: status.recall.delta_invoked_total as u32,
                peer_update_total: 0,
            },
        }
    }
}

pub fn daemon_error(
    route: &'static str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": "daemon_request_failed",
            "route": route,
            "code": code.into(),
            "message": message.into()
        })),
    )
}
