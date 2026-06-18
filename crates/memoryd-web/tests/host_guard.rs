// DNS-rebinding / cross-origin guard coverage. Uses the fixture router so the
// data-bearing GET routes are backed and would otherwise return 200 with real
// content. The guard must reject any request whose `Host` is non-loopback or
// whose `Origin`/`Referer` resolves off loopback, across every route.
#![cfg(feature = "dev-fixtures")]

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use memoryd_web::{fixture_router, DEV_FIXTURE_DASHBOARD_AUTH_TOKEN};
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;
const DATA_ROUTE: &str = "/api/search?q=memory";

fn get_with_headers(uri: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).expect("request builds")
}

/// Issue a host-guard probe carrying a valid bearer token, so the host guard is
/// the only gate under test. Data-bearing GET reads are also CSRF-gated now; the
/// "allowed" assertions below would otherwise see a 403 from the CSRF layer
/// rather than confirming the host guard let the request through.
async fn status_for(headers: &[(&str, &str)]) -> StatusCode {
    let app = fixture_router();
    let token = fetch_csrf_token(app.clone()).await;
    let mut with_token: Vec<(&str, &str)> = headers.to_vec();
    with_token.push(("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN));
    with_token.push(("x-memorum-csrf", &token));
    app.oneshot(get_with_headers(DATA_ROUTE, &with_token)).await.expect("request succeeds").status()
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
    let bytes = to_bytes(response.into_body(), RESPONSE_LIMIT).await.expect("body bytes");
    let html = String::from_utf8(bytes.to_vec()).expect("html utf8");
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

#[tokio::test]
async fn test_attacker_host_header_is_rejected() {
    // The DNS-rebinding signature: browser navigated to the attacker's domain,
    // which now resolves to 127.0.0.1, so the Host header carries the domain.
    assert_eq!(status_for(&[("host", "attacker.example:7137")]).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_loopback_host_headers_are_allowed() {
    for host in ["127.0.0.1:7137", "127.0.0.1", "localhost:7137", "localhost", "[::1]:7137", "[::1]"] {
        let status = status_for(&[("host", host)]).await;
        assert_ne!(status, StatusCode::FORBIDDEN, "host {host} must be allowed");
    }
}

#[tokio::test]
async fn test_cross_origin_referer_is_rejected() {
    let status = status_for(&[("host", "127.0.0.1:7137"), ("origin", "http://attacker.example")]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_cross_origin_referer_header_is_rejected() {
    let status = status_for(&[("host", "127.0.0.1:7137"), ("referer", "http://attacker.example/page")]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_loopback_origin_is_allowed() {
    let status = status_for(&[("host", "127.0.0.1:7137"), ("origin", "http://127.0.0.1:7137")]).await;
    assert_ne!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_guard_applies_to_every_data_route() {
    let routes = [
        "/api/status",
        "/api/entity-graph",
        "/api/recall-hits",
        "/api/search?q=x",
        "/api/audit/mem_20260501_a1b2c3d4e5f60718_000010",
        "/api/audit/mem_20260501_a1b2c3d4e5f60718_000010/walk",
        "/api/review",
        "/api/sync-dashboard",
    ];
    for route in routes {
        let response = fixture_router()
            .oneshot(get_with_headers(route, &[("host", "attacker.example")]))
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{route} must be host-guarded");
    }
}

#[tokio::test]
async fn test_missing_host_is_allowed_for_loopback_clients() {
    // In-process callers (and HTTP/1.0 loopback clients) without a Host header
    // cannot be a browser fetch to a rebind domain, so they are allowed.
    let app = fixture_router();
    let token = fetch_csrf_token(app.clone()).await;
    let status = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds")
        .status();
    assert_ne!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_csrf_post_still_rejected_under_loopback_host() {
    // The host guard must not weaken the CSRF gate on POST routes.
    let response = fixture_router()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/review/action")
                .header("host", "127.0.0.1:7137")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"id":"x","action":"approve"}"#))
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
