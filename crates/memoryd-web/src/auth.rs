use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use rand::RngCore;

use crate::server::WebState;

pub const CSRF_HEADER: &str = "x-memorum-csrf";
const CSRF_TOKEN_BYTES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsrfToken(String);

impl CsrfToken {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; CSRF_TOKEN_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(hex::encode(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches_header(&self, request: &Request<Body>) -> bool {
        request.headers().get(CSRF_HEADER).and_then(|value| value.to_str().ok()).is_some_and(|value| value == self.0)
    }
}

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
