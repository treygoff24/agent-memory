use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

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
