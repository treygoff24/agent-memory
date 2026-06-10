use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};

#[cfg(feature = "dev-fixtures")]
use memoryd::protocol::RecallHitSummary;
use serde::Deserialize;
use serde_json::json;

use crate::routes::status::daemon_error;
use crate::state::{backend_unavailable, WebState};

#[derive(Clone, Debug, Deserialize)]
pub struct RecallHitsQuery {
    pub since: Option<String>,
    pub limit: Option<usize>,
}

pub async fn recall_hits(State(state): State<WebState>, Query(query): Query<RecallHitsQuery>) -> impl IntoResponse {
    let since = match parse_since(query.since.as_deref()) {
        Ok(since) => since,
        Err(message) => return invalid_query(message).into_response(),
    };
    if let Some(data) = state.dashboard_data() {
        let mut hits = data.recall_hits.clone();
        if let Some(since) = since {
            hits.retain(|hit| hit.recalled_at > since);
        }
        let limit = query.limit.unwrap_or(hits.len()).clamp(1, 500);
        hits.truncate(limit);
        return Json(memoryd::protocol::RecallHitsResponse { since, limit, hits }).into_response();
    }

    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("recall_hits").into_response();
    };
    match memoryd::client::request(
        socket_path,
        "web-recall-hits",
        RequestPayload::RecallHits { since, limit: query.limit },
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::RecallHits(response)) => Json(response).into_response(),
            ResponseResult::Error(error) => daemon_error("recall_hits", error.code, error.message).into_response(),
            other => daemon_error("recall_hits", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("recall_hits", "daemon_unavailable", error.to_string()).into_response(),
    }
}

#[cfg(feature = "dev-fixtures")]
pub fn fixture_recall_hit(memory_id: &str, recalled_at: DateTime<Utc>, summary: &str) -> RecallHitSummary {
    RecallHitSummary {
        event_id: format!("evt_fixture_recall_{memory_id}"),
        device: "dev_web_fixture".to_owned(),
        seq: 42,
        memory_id: memoryd::protocol::MemoryId::new(memory_id),
        recalled_at,
        summary: Some(summary.to_owned()),
    }
}

fn parse_since(raw: Option<&str>) -> Result<Option<DateTime<Utc>>, String> {
    raw.map(|value| {
        DateTime::parse_from_rfc3339(value)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| format!("since must be an RFC3339 timestamp, got `{value}`"))
    })
    .transpose()
}

fn invalid_query(message: String) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "invalid_query",
            "route": "recall_hits",
            "message": message
        })),
    )
}
