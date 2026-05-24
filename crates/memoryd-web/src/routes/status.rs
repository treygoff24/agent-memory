use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use memoryd::protocol::{ResponsePayload, ResponseResult};
use serde::Serialize;
use serde_json::json;

use crate::state::{backend_unavailable, WebState};

#[derive(Clone, Debug, Serialize)]
pub struct StatusDashboardResponse {
    pub degraded: bool,
    pub warnings: Vec<String>,
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
    pub uptime_seconds: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct IndexStatus {
    pub active_memories: u64,
    pub last_reindex: Option<DateTime<Utc>>,
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
    pub next_run: Option<DateTime<Utc>>,
    pub last_run: DreamRunSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct DreamRunSummary {
    pub at: Option<DateTime<Utc>>,
    pub promoted: Option<u32>,
    pub queued: Option<u32>,
    pub dropped: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecallStatus {
    pub startup_total: u32,
    pub delta_total: u32,
    pub peer_update_snapshot_count: u32,
}

impl StatusDashboardResponse {
    pub fn fixture(now: DateTime<Utc>) -> Self {
        Self {
            degraded: false,
            warnings: Vec::new(),
            daemon: DaemonStatus { version: "0.1.0-test".to_owned(), pid: 7137, uptime_seconds: Some(302_440) },
            socket: "ok".to_owned(),
            index: IndexStatus { active_memories: 1_204, last_reindex: Some(now) },
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
                next_run: Some(now + chrono::Duration::hours(15)),
                last_run: DreamRunSummary {
                    at: Some(now - chrono::Duration::hours(9)),
                    promoted: Some(3),
                    queued: Some(1),
                    dropped: Some(0),
                },
            },
            recall: RecallStatus { startup_total: 42, delta_total: 119, peer_update_snapshot_count: 8 },
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
        let mut warnings = status.dashboard_warnings;
        let index_stats = status.index_stats;
        if index_stats.is_none() {
            warnings.push("index_stats_unavailable".to_owned());
        }
        let review_counts = status.review_queue_counts;
        if review_counts.is_none() {
            warnings.push("review_queue_counts_unavailable".to_owned());
        }
        let dream_status = status.compact_dream_status;
        if dream_status.is_none() {
            warnings.push("compact_dream_status_unavailable".to_owned());
        }
        if status.conflicts_count.is_none() {
            warnings.push("conflicts_count_unavailable".to_owned());
        }
        let active_sessions = status
            .peer_sessions
            .into_iter()
            .map(|session| ActiveSession { harness: session.harness, session_id: session.session_id })
            .collect();
        let dream_status_label = match dream_status.as_ref().map(|status| status.enabled) {
            Some(true) => "enabled",
            Some(false) => "disabled",
            None => "unknown",
        };
        let daemon = status.daemon.unwrap_or_else(|| {
            warnings.push("daemon_process_status_unavailable".to_owned());
            memoryd::protocol::DaemonProcessStatus { version: "unknown".to_owned(), pid: 0, uptime_seconds: None }
        });
        let next_run = dream_status.as_ref().and_then(|status| status.next_scheduled_at);
        let last_run_at = dream_status.as_ref().and_then(|status| status.last_run_at);
        Self {
            degraded: status.state != "ready" || !warnings.is_empty(),
            warnings,
            daemon: DaemonStatus { version: daemon.version, pid: daemon.pid, uptime_seconds: daemon.uptime_seconds },
            socket: status.state,
            index: IndexStatus {
                active_memories: index_stats.as_ref().map_or(0, |stats| stats.active_memories),
                last_reindex: index_stats.and_then(|stats| stats.last_reindex),
            },
            sync: SyncStatus { ahead: 0, behind: 0, last_push: now, remote: "daemon".to_owned() },
            review: ReviewStatus {
                candidate: review_counts.as_ref().map_or(0, |counts| saturating_u32(counts.candidate)),
                quarantined: review_counts.as_ref().map_or(0, |counts| saturating_u32(counts.quarantined)),
                dream_low_confidence: review_counts
                    .as_ref()
                    .map_or(0, |counts| saturating_u32(counts.dream_low_confidence)),
            },
            conflicts: status.conflicts_count.unwrap_or(0),
            active_sessions,
            dreaming: DreamingStatus {
                status: dream_status_label.to_owned(),
                next_run,
                last_run: DreamRunSummary { at: last_run_at, promoted: None, queued: None, dropped: None },
            },
            recall: RecallStatus {
                startup_total: status.recall.startup_invoked_total as u32,
                delta_total: status.recall.delta_invoked_total as u32,
                peer_update_snapshot_count: status.peer_update_count.map_or(0, saturating_u32),
            },
        }
    }
}

fn saturating_u32(value: u64) -> u32 {
    value.try_into().unwrap_or(u32::MAX)
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use memoryd::protocol::{CompactDreamStatus, IndexStats, PeerSessionStatus, ReviewQueueCounts, StatusResponse};
    use memoryd::recall::RecallStatusCounters;

    use super::StatusDashboardResponse;

    #[test]
    fn from_daemon_uses_live_status_fields() {
        let last_reindex = chrono::Utc.with_ymd_and_hms(2026, 5, 22, 10, 0, 0).unwrap();
        let last_dream_run = chrono::Utc.with_ymd_and_hms(2026, 5, 22, 11, 0, 0).unwrap();
        let response = StatusDashboardResponse::from_daemon(StatusResponse {
            state: "ready".to_owned(),
            guidance: "live daemon".to_owned(),
            recall: RecallStatusCounters { startup_invoked_total: 3, delta_invoked_total: 5, ..Default::default() },
            index_stats: Some(IndexStats { active_memories: 17, last_reindex: Some(last_reindex) }),
            review_queue_counts: Some(ReviewQueueCounts { candidate: 2, quarantined: 4, dream_low_confidence: 6 }),
            conflicts_count: Some(4),
            peer_sessions: vec![PeerSessionStatus {
                session_id: "session-1".to_owned(),
                harness: "codex".to_owned(),
                namespace: "project:agent-memory".to_owned(),
                salient_entities: Vec::new(),
                started_at: None,
                last_heartbeat_age_seconds: 1,
            }],
            peer_update_count: Some(9),
            daemon: Some(memoryd::protocol::DaemonProcessStatus {
                version: "0.1.0-daemon".to_owned(),
                pid: 4242,
                uptime_seconds: Some(99),
            }),
            compact_dream_status: Some(CompactDreamStatus {
                enabled: true,
                last_run_at: Some(last_dream_run),
                last_run_outcome: None,
                next_scheduled_at: Some(last_dream_run + chrono::Duration::hours(6)),
                active_leases: vec!["agent".to_owned()],
            }),
            ..Default::default()
        });

        assert!(!response.degraded);
        assert!(response.warnings.iter().all(|warning| !warning.contains("unavailable")));
        assert_eq!(response.daemon.version, "0.1.0-daemon");
        assert_eq!(response.daemon.pid, 4242);
        assert_eq!(response.daemon.uptime_seconds, Some(99));
        assert_eq!(response.socket, "ready");
        assert_eq!(response.index.active_memories, 17);
        assert_eq!(response.index.last_reindex, Some(last_reindex));
        assert_eq!(response.review.candidate, 2);
        assert_eq!(response.review.quarantined, 4);
        assert_eq!(response.review.dream_low_confidence, 6);
        assert_eq!(response.conflicts, 4);
        assert_eq!(response.active_sessions.len(), 1);
        assert_eq!(response.active_sessions[0].harness, "codex");
        assert_eq!(response.dreaming.status, "enabled");
        assert_eq!(response.dreaming.next_run, Some(last_dream_run + chrono::Duration::hours(6)));
        assert_eq!(response.dreaming.last_run.at, Some(last_dream_run));
        assert_eq!(response.recall.startup_total, 3);
        assert_eq!(response.recall.delta_total, 5);
        assert_eq!(response.recall.peer_update_snapshot_count, 9);
    }
}
