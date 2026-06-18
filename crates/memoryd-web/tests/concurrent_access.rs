// The concurrent-claim path needs the reviewable fixture memory to exist, so this
// suite only compiles/runs with `--features dev-fixtures`.
#![cfg(feature = "dev-fixtures")]

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use memoryd_web::{fixture_router, DEV_FIXTURE_DASHBOARD_AUTH_TOKEN};
use serde_json::{json, Value};
use tower::ServiceExt;

const REVIEWABLE_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000001";

#[tokio::test]
async fn test_concurrent_post_same_memory_second_returns_409() {
    let app = fixture_router();
    let token = fetch_csrf_token(app.clone()).await;

    let first = app.clone().oneshot(post_review_action(&token, REVIEWABLE_MEMORY_ID));
    let second = app.oneshot(post_review_action(&token, REVIEWABLE_MEMORY_ID));
    let (first, second) = tokio::join!(first, second);

    let first = first.expect("first request succeeds");
    let second = second.expect("second request succeeds");
    let first_status = first.status();
    let second_status = second.status();
    let mut statuses = [first_status, second_status];
    statuses.sort();

    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);

    let conflict = if first_status == StatusCode::CONFLICT { first } else { second };
    let body = json_body(conflict).await;
    assert_eq!(body, json!({ "error": "memory_not_in_review_state" }));
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.expect("body bytes are collected");
    serde_json::from_slice(&bytes).expect("response is json")
}

fn post_review_action(csrf_token: &str, id: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/review/action")
        .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
        .header("x-memorum-csrf", csrf_token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "id": id,
                "action": "approve",
                "reason": "test"
            })
            .to_string(),
        ))
        .expect("request builds")
}

async fn fetch_csrf_token(app: axum::Router) -> String {
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.expect("body bytes are collected");
    let html = String::from_utf8(bytes.to_vec()).expect("response is utf8");
    csrf_token_from_html(&html).to_owned()
}

fn csrf_token_from_html(html: &str) -> &str {
    let name_at = html.find(r#"name="csrf-token""#).expect("csrf meta tag exists");
    let tag_start = html[..name_at].rfind("<meta").expect("csrf tag starts with meta");
    let tag_end = html[name_at..].find('>').expect("csrf meta tag closes") + name_at;
    let tag = &html[tag_start..tag_end];
    let marker = r#"content=""#;
    let start = tag.find(marker).expect("csrf meta tag has content") + marker.len();
    let tail = &tag[start..];
    let end = tail.find('"').expect("csrf meta content closes");
    &tail[..end]
}
