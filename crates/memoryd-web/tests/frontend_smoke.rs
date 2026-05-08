use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use flate2::write::GzEncoder;
use flate2::Compression;
use memoryd_web::{embedded_asset_names, fixture_router};
use std::io::Write;
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;
const ASSET_RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const CSS_GZIP_BUDGET: usize = 80 * 1024;
const JS_GZIP_BUDGET: usize = 250 * 1024;

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
    assert_no_inline_script_or_style(&body);

    for asset in assets.iter().filter(|path| path.starts_with("assets/") && path.ends_with(".css")) {
        let bytes = fetch_asset(asset).await;
        let gzip_bytes = gzip_len(&bytes);
        assert!(gzip_bytes <= CSS_GZIP_BUDGET, "{asset} gzip size {gzip_bytes} exceeds CSS budget {CSS_GZIP_BUDGET}");
    }
    for asset in assets.iter().filter(|path| path.starts_with("assets/") && path.ends_with(".js")) {
        let bytes = fetch_asset(asset).await;
        let gzip_bytes = gzip_len(&bytes);
        assert!(gzip_bytes <= JS_GZIP_BUDGET, "{asset} gzip size {gzip_bytes} exceeds JS budget {JS_GZIP_BUDGET}");
    }
}

async fn fetch_asset(asset: &str) -> Vec<u8> {
    let response = fixture_router()
        .oneshot(Request::builder().uri(format!("/{asset}")).body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::OK, "{asset}");
    to_bytes(response.into_body(), ASSET_RESPONSE_LIMIT).await.expect("read asset").to_vec()
}

fn gzip_len(bytes: &[u8]) -> usize {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip write succeeds");
    encoder.finish().expect("gzip finish succeeds").len()
}

fn assert_no_inline_script_or_style(body: &str) {
    assert_no_inline_tag(body, "script");
    assert_no_inline_tag(body, "style");
}

fn assert_no_inline_tag(body: &str, tag: &str) {
    let lower = body.to_lowercase();
    let mut offset = 0;
    while let Some(start) = lower[offset..].find(&format!("<{tag}")) {
        let absolute_start = offset + start;
        let Some(open_end) = lower[absolute_start..].find('>').map(|index| absolute_start + index) else {
            break;
        };
        let open_tag = &lower[absolute_start..=open_end];
        let Some(close_start) = lower[open_end + 1..].find(&format!("</{tag}>")).map(|index| open_end + 1 + index)
        else {
            break;
        };
        let inline_body = body[open_end + 1..close_start].trim();
        let external_script = tag == "script" && open_tag.contains(" src=");
        assert!(external_script || inline_body.is_empty(), "inline {tag} should be absent: {inline_body}");
        offset = close_start + tag.len() + 3;
    }
}
