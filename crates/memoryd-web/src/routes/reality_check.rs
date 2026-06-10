use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::{
    RealityCheckAction, RealityCheckCompletion, RealityCheckHistorySession, RealityCheckItem, RealityCheckRequest,
    RealityCheckResponse, RequestPayload, ResponsePayload, ResponseResult,
};
use serde::{Deserialize, Serialize};

use crate::routes::status::daemon_error;
use crate::routes::REALITY_CHECK_SESSION_ID;
use crate::state::{backend_unavailable, RealityCheckActionRecord, WebState};

#[derive(Clone, Debug, Deserialize)]
pub struct RealityCheckHistoryQuery {
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RealityCheckRespondRequest {
    pub memory_id: String,
    pub action: String,
    pub correction: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RealityCheckActionResponse {
    pub accepted: bool,
    pub session_id: String,
    pub memory_id: String,
    pub action: String,
    pub completion: RealityCheckCompletion,
}

#[derive(Clone, Debug, Serialize)]
pub struct RealityCheckHistoryResponse {
    pub sessions: Vec<RealityCheckHistorySession>,
}

#[cfg(feature = "dev-fixtures")]
impl RealityCheckHistoryResponse {
    pub fn fixture(now: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            sessions: vec![RealityCheckHistorySession {
                session_id: "fixture".to_owned(),
                started_at: now - chrono::Duration::days(7) - chrono::Duration::minutes(5),
                completed_at: now - chrono::Duration::days(7),
                items_total: 7,
                reviewed: 7,
                confirmed: 5,
                corrected: 1,
                forgotten: 0,
                not_relevant: 1,
                deferred: 0,
                remaining: 0,
            }],
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RealityCheckStatusResponse {
    pub kind: String,
    pub session_id: String,
    pub items: Vec<RealityCheckItem>,
    pub total_scored: usize,
    pub last_completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn reality_check(State(state): State<WebState>) -> impl IntoResponse {
    let Some(data) = state.dashboard_data() else {
        if let Some(socket_path) = state.daemon_socket() {
            return match memoryd::client::request(
                socket_path,
                "web-reality-check",
                RequestPayload::RealityCheck(RealityCheckRequest::Run {
                    session_id: Some(REALITY_CHECK_SESSION_ID.to_owned()),
                    namespace: None,
                    limit: None,
                }),
            )
            .await
            {
                Ok(response) => match response.result {
                    ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::Pending {
                        session_id,
                        items,
                        total_scored,
                        last_completed_at,
                    })) => Json(RealityCheckStatusResponse {
                        kind: "pending".to_owned(),
                        session_id: session_id.unwrap_or_else(|| REALITY_CHECK_SESSION_ID.to_owned()),
                        items,
                        total_scored,
                        last_completed_at,
                    })
                    .into_response(),
                    ResponseResult::Error(error) => {
                        daemon_error("reality_check", error.code, error.message).into_response()
                    }
                    other => daemon_error("reality_check", "unexpected_response", format!("{other:?}")).into_response(),
                },
                Err(error) => daemon_error("reality_check", "daemon_unavailable", error.to_string()).into_response(),
            };
        }
        return backend_unavailable("reality_check").into_response();
    };
    Json(RealityCheckStatusResponse {
        kind: "pending".to_owned(),
        session_id: REALITY_CHECK_SESSION_ID.to_owned(),
        items: data.reality_check_items.clone(),
        total_scored: data.reality_check_items.len(),
        last_completed_at: None,
    })
    .into_response()
}

pub async fn reality_check_history(
    State(state): State<WebState>,
    Query(query): Query<RealityCheckHistoryQuery>,
) -> impl IntoResponse {
    let Some(data) = state.dashboard_data() else {
        if let Some(socket_path) = state.daemon_socket() {
            return match memoryd::client::request(
                socket_path,
                "web-reality-check-history",
                RequestPayload::RealityCheck(RealityCheckRequest::History { limit: query.limit }),
            )
            .await
            {
                Ok(response) => match response.result {
                    ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::History {
                        sessions,
                    })) => Json(RealityCheckHistoryResponse { sessions }).into_response(),
                    ResponseResult::Error(error) => {
                        daemon_error("reality_check_history", error.code, error.message).into_response()
                    }
                    other => daemon_error("reality_check_history", "unexpected_response", format!("{other:?}"))
                        .into_response(),
                },
                Err(error) => {
                    daemon_error("reality_check_history", "daemon_unavailable", error.to_string()).into_response()
                }
            };
        }
        return backend_unavailable("reality_check_history").into_response();
    };
    let mut history = data.reality_check_history.clone();
    if let Some(limit) = query.limit {
        history.sessions.truncate(limit);
    }
    Json(history).into_response()
}

pub async fn reality_check_respond(
    State(state): State<WebState>,
    Json(payload): Json<RealityCheckRespondRequest>,
) -> impl IntoResponse {
    if state.dashboard_data().is_none() {
        if let Some(socket_path) = state.daemon_socket() {
            return daemon_reality_check_respond(&state, socket_path, payload).await;
        }
        return backend_unavailable("reality_check_respond").into_response();
    }
    state
        .record_reality_check_action(RealityCheckActionRecord {
            memory_id: payload.memory_id.clone(),
            action: payload.action.clone(),
            correction: payload.correction.clone(),
        })
        .await;

    (
        StatusCode::OK,
        Json(RealityCheckActionResponse {
            accepted: true,
            session_id: REALITY_CHECK_SESSION_ID.to_owned(),
            memory_id: payload.memory_id,
            action: payload.action,
            completion: RealityCheckCompletion::Progress { remaining: 0, deferred: 0 },
        }),
    )
        .into_response()
}

async fn daemon_reality_check_respond(
    state: &WebState,
    socket_path: &std::path::Path,
    payload: RealityCheckRespondRequest,
) -> axum::response::Response {
    let session_id = payload.session_id.clone().unwrap_or_else(|| REALITY_CHECK_SESSION_ID.to_owned());
    let Ok(memory_id) = memoryd::protocol::MemoryId::try_new(payload.memory_id.clone()) else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid_memory_id"}))).into_response();
    };
    let action = match payload.action.as_str() {
        "confirm" => RealityCheckAction::Confirm,
        "correct" => RealityCheckAction::Correct { new_body: payload.correction.clone().unwrap_or_default() },
        "forget" => RealityCheckAction::Forget {
            reason: payload.correction.clone().unwrap_or_else(|| "web reality check".to_owned()),
        },
        "not_relevant" => RealityCheckAction::NotRelevant,
        "skip_this_week" => RealityCheckAction::SkipThisWeek,
        _ => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid_reality_check_action"})))
                .into_response()
        }
    };

    match memoryd::client::request(
        socket_path,
        format!("web-reality-check-respond-{}", memory_id.as_str()),
        RequestPayload::RealityCheck(RealityCheckRequest::Respond {
            session_id: session_id.clone(),
            memory_id,
            action,
        }),
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::RespondAccepted {
                session_id,
                memory_id,
                completion,
                ..
            })) => {
                state
                    .record_reality_check_action(RealityCheckActionRecord {
                        memory_id: payload.memory_id.clone(),
                        action: payload.action.clone(),
                        correction: payload.correction.clone(),
                    })
                    .await;
                (
                    StatusCode::OK,
                    Json(RealityCheckActionResponse {
                        accepted: true,
                        session_id,
                        memory_id: memory_id.as_str().to_owned(),
                        action: payload.action,
                        completion,
                    }),
                )
                    .into_response()
            }
            ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::RespondRefused {
                session_id,
                memory_id,
                reason,
                kind,
            })) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "accepted": false,
                    "error": "reality_check_refused",
                    "session_id": session_id,
                    "memory_id": memory_id.as_str(),
                    "action": payload.action,
                    "reason": reason,
                    "kind": kind,
                })),
            )
                .into_response(),
            ResponseResult::Error(error) => {
                daemon_error("reality_check_respond", error.code, error.message).into_response()
            }
            other => daemon_error("reality_check_respond", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("reality_check_respond", "daemon_unavailable", error.to_string()).into_response(),
    }
}
