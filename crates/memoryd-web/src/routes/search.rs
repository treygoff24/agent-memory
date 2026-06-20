use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::RequestPayload;
use serde::Deserialize;

use crate::routes::daemon::daemon_call;
use crate::state::{backend_unavailable, WebState};

#[derive(Clone, Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<usize>,
}

pub async fn search(State(state): State<WebState>, Query(query): Query<SearchQuery>) -> impl IntoResponse {
    let limit = query.limit.map(|limit| limit.clamp(1, 50)).or(Some(10));

    let Some(socket_path) = state.daemon_socket() else {
        return backend_unavailable("search").into_response();
    };

    match daemon_call::<memoryd::protocol::SearchResponse>(
        socket_path,
        "search",
        "web-search",
        RequestPayload::Search { query: query.q, limit, include_body: false },
    )
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(response) => response,
    }
}
