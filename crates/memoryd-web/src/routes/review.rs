use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::{RequestPayload, ResponseResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

use crate::routes::daemon::daemon_call;
use crate::routes::status::daemon_error;
use crate::state::{backend_unavailable, Backend, ReviewActionRecord, WebState};

#[derive(Clone, Debug, Deserialize)]
pub struct ReviewQueueQuery {
    pub status: Option<String>,
    pub namespace: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewQueueResponse {
    pub items: Vec<ReviewQueueItem>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewQueueItem {
    pub id: String,
    pub summary: String,
    pub status: String,
    pub namespace: String,
    pub policy_applied: String,
    pub reason: Option<String>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewActionRequest {
    pub id: String,
    pub action: String,
    pub reason: Option<String>,
}

pub async fn review_queue(State(state): State<WebState>, Query(query): Query<ReviewQueueQuery>) -> impl IntoResponse {
    match state.backend() {
        #[cfg(feature = "dev-fixtures")]
        Backend::Fixture(data) => {
            let offset = query.offset.unwrap_or(0);
            let limit = query.limit.unwrap_or(50);
            let status = query.status.unwrap_or_else(|| "candidate".to_owned());
            let namespace = query.namespace.unwrap_or_else(|| "project:agent-memory".to_owned());
            let mut items = data
                .reviewable_memory_ids
                .iter()
                .map(|id| ReviewQueueItem {
                    id: id.clone(),
                    summary: "Review Stream G dashboard contract".to_owned(),
                    status: status.clone(),
                    namespace: namespace.clone(),
                    policy_applied: "project-standard@v2".to_owned(),
                    reason: Some("review_required".to_owned()),
                    next_actions: vec![
                        "approve".to_owned(),
                        "reject".to_owned(),
                        "forget".to_owned(),
                        "quarantine".to_owned(),
                    ],
                })
                .collect::<Vec<_>>();
            items = items.into_iter().skip(offset).take(limit).collect();
            Json(ReviewQueueResponse { items, limit, offset }).into_response()
        }
        Backend::Daemon(socket_path) => {
            match daemon_call::<memoryd::protocol::ReviewQueueResponse>(
                socket_path,
                "review_queue",
                "web-review-queue",
                RequestPayload::ReviewQueue { limit: query.limit },
            )
            .await
            {
                Ok(queue) => Json(ReviewQueueResponse {
                    items: queue
                        .items
                        .into_iter()
                        .map(|item| ReviewQueueItem {
                            id: item.id,
                            summary: item.summary,
                            status: item.status.as_str().to_owned(),
                            namespace: query.namespace.clone().unwrap_or_else(|| "daemon".to_owned()),
                            policy_applied: item.policy_applied,
                            reason: item.reason,
                            next_actions: item.next_actions,
                        })
                        .collect(),
                    limit: query.limit.unwrap_or(50),
                    offset: query.offset.unwrap_or(0),
                })
                .into_response(),
                Err(response) => response,
            }
        }
        Backend::Unavailable => backend_unavailable("review_queue").into_response(),
    }
}

pub async fn review_action(
    State(state): State<WebState>,
    Json(payload): Json<ReviewActionRequest>,
) -> impl IntoResponse {
    if state.dashboard_data().is_none() {
        if let Some(socket_path) = state.daemon_socket() {
            return daemon_review_action(&state, socket_path, payload).await;
        }
        return backend_unavailable("review_action").into_response();
    }
    if !state.is_reviewable(&payload.id) || !state.claim_review_action(&payload.id).await {
        return memory_not_in_review_state().into_response();
    }

    sleep(Duration::from_millis(25)).await;
    state
        .record_review_action(ReviewActionRecord {
            id: payload.id.clone(),
            action: payload.action.clone(),
            reason: payload.reason.clone(),
        })
        .await;
    state.release_review_action(&payload.id).await;

    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "id": payload.id,
            "action": payload.action
        })),
    )
        .into_response()
}

async fn daemon_review_action(
    state: &WebState,
    socket_path: &std::path::Path,
    payload: ReviewActionRequest,
) -> axum::response::Response {
    let request = match payload.action.as_str() {
        "approve" => RequestPayload::ReviewApprove { id: payload.id.clone() },
        "reject" => RequestPayload::ReviewReject {
            id: payload.id.clone(),
            reason: payload.reason.clone().unwrap_or_else(|| "web dashboard rejection".to_owned()),
        },
        "forget" | "quarantine" => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(json!({"error": "review_action_not_implemented", "action": payload.action})),
            )
                .into_response()
        }
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_review_action"}))).into_response(),
    };

    match memoryd::client::request(socket_path, format!("web-review-action-{}", payload.id), request).await {
        Ok(response) => match response.result {
            ResponseResult::Success(_) => {
                state
                    .record_review_action(ReviewActionRecord {
                        id: payload.id.clone(),
                        action: payload.action.clone(),
                        reason: payload.reason.clone(),
                    })
                    .await;
                (
                    StatusCode::OK,
                    Json(json!({
                        "ok": true,
                        "id": payload.id,
                        "action": payload.action
                    })),
                )
                    .into_response()
            }
            ResponseResult::Error(error) if error.code == "invalid_request" => {
                memory_not_in_review_state().into_response()
            }
            ResponseResult::Error(error) => daemon_error("review_action", error.code, error.message).into_response(),
        },
        Err(error) => daemon_error("review_action", "daemon_unavailable", error.to_string()).into_response(),
    }
}

fn memory_not_in_review_state() -> (StatusCode, Json<Value>) {
    (StatusCode::CONFLICT, Json(json!({ "error": "memory_not_in_review_state" })))
}
