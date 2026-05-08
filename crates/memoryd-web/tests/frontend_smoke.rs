use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use memoryd_web::{embedded_asset_names, fixture_router};
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;

#[tokio::test]
async fn embedded_frontend_serves_vite_index_and_hashed_assets() {
    let assets = embedded_asset_names();
    assert!(assets.iter().any(|path| path == "index.html"), "assets: {assets:#?}");
    assert!(assets.iter().any(|path| path.starts_with("assets/") && path.ends_with(".js")), "assets: {assets:#?}");
    assert!(assets.iter().any(|path| path.starts_with("assets/") && path.ends_with(".css")), "assets: {assets:#?}");

    let response = fixture_router()
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(to_bytes(response.into_body(), RESPONSE_LIMIT).await.expect("read body").to_vec())
        .expect("utf8 body");

    assert!(body.contains("<title>Memorum Dashboard</title>"), "body: {body}");
    // Vite ships HTML formatted by prettier, so meta-tag attributes can split
    // across lines. Normalize whitespace so the substring assertion is robust
    // to formatting without weakening intent.
    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(normalized.contains("name=\"csrf-token\" content=\""), "body: {body}");
    assert!(!body.contains("__MEMORUM_CSRF_TOKEN__"), "CSRF placeholder must be rewritten: {body}");
    assert!(!body.contains("<script>") && !body.contains("<style>"), "inline script/style should be absent: {body}");
}
