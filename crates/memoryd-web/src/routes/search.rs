use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use serde::Deserialize;

use crate::routes::status::daemon_error;
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

    match memoryd::client::request(
        socket_path,
        "web-search",
        RequestPayload::Search { query: query.q, limit, include_body: false },
    )
    .await
    {
        Ok(response) => match response.result {
            ResponseResult::Success(ResponsePayload::Search(response)) => Json(response).into_response(),
            ResponseResult::Error(error) => daemon_error("search", error.code, error.message).into_response(),
            other => daemon_error("search", "unexpected_response", format!("{other:?}")).into_response(),
        },
        Err(error) => daemon_error("search", "daemon_unavailable", error.to_string()).into_response(),
    }
}
