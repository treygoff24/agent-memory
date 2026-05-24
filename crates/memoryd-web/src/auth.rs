use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::WebState;

pub use crate::state::CSRF_HEADER;

pub async fn require_csrf(
    State(state): State<WebState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.csrf_token().matches_header(&request) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
