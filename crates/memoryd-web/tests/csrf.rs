use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use memoryd_web::config::{WebConfig, WebConfigError};
use memoryd_web::fixture_router;
use serde_json::json;
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;
const REVIEWABLE_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000001";

#[tokio::test]
async fn test_post_without_csrf_header_returns_403() {
    let response =
        fixture_router().oneshot(post_review_action(None, "mem_missing_csrf")).await.expect("request succeeds");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_post_with_wrong_csrf_token_returns_403() {
    let response = fixture_router()
        .oneshot(post_review_action(Some("wrong-token"), "mem_wrong_csrf"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_post_with_correct_csrf_token_succeeds() {
    let app = fixture_router();
    let token = fetch_csrf_token(app.clone()).await;

    let response = app.oneshot(post_review_action(Some(&token), REVIEWABLE_MEMORY_ID)).await.expect("request succeeds");

    assert_ne!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_csrf_token_in_initial_html() {
    let app = fixture_router();
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body(response).await;
    let token = csrf_token_from_html(&body);

    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
}

#[tokio::test]
async fn test_csrf_token_rotates_between_server_states() {
    let first = fetch_csrf_token(fixture_router()).await;
    let second = fetch_csrf_token(fixture_router()).await;

    assert_ne!(first, second);
}

#[test]
fn test_bind_address_0_0_0_0_rejected_at_config() {
    let temp = tempfile::NamedTempFile::new().expect("temp config is created");
    std::fs::write(
        temp.path(),
        r#"
web:
  enabled: true
  bind_address: 0.0.0.0
  port: 7137
"#,
    )
    .expect("config is written");

    let error = WebConfig::from_config_yaml(temp.path()).expect_err("0.0.0.0 is rejected");
    assert!(error.downcast_ref::<WebConfigError>().is_some());
}

#[tokio::test]
async fn test_spec_api_get_routes_return_json() {
    let app = fixture_router();
    let get_routes = [
        "/api/status",
        "/api/entity-graph",
        "/api/entity-graph/ent_memorum",
        "/api/roi",
        "/api/reality-check",
        "/api/reality-check/history",
        "/api/audit/mem_20260501_a1b2c3d4e5f60718_000010",
        "/api/audit/mem_20260501_a1b2c3d4e5f60718_000010/walk",
        "/api/audit/mem_20260501_a1b2c3d4e5f60718_000010/temporal",
        "/api/review",
    ];

    for route in get_routes {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(route).body(Body::empty()).expect("request builds"))
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_json(response, route).await;
    }
}

#[tokio::test]
async fn test_reality_check_post_with_correct_csrf_token_succeeds() {
    let app = fixture_router();
    let token = fetch_csrf_token(app.clone()).await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/reality-check/respond")
                .header("x-memorum-csrf", token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"memory_id": REVIEWABLE_MEMORY_ID, "action": "skip_this_week"}).to_string()))
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    assert_json(response, "/api/reality-check/respond").await;
}

fn post_review_action(csrf_token: Option<&str>, id: &str) -> Request<Body> {
    let mut builder =
        Request::builder().method("POST").uri("/api/review/action").header(header::CONTENT_TYPE, "application/json");

    if let Some(token) = csrf_token {
        builder = builder.header("x-memorum-csrf", token);
    }

    builder
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
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");
    let body = response_body(response).await;
    csrf_token_from_html(&body).to_owned()
}

async fn assert_json(response: axum::response::Response, route: &str) {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response_body(response).await;

    assert!(content_type.starts_with("application/json"), "{route}: {content_type}");
    assert!(!body.contains("memory body"), "{route}");
}

async fn response_body(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), RESPONSE_LIMIT).await.expect("body bytes are collected");
    String::from_utf8(bytes.to_vec()).expect("response is utf8")
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
