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
    let mut offset = 0;
    let open_needle = format!("<{tag}");
    let close_needle = format!("</{tag}>");
    while let Some(absolute_start) = find_ascii_case_insensitive(body, &open_needle, offset) {
        let Some(open_end) = body[absolute_start..].find('>').map(|index| absolute_start + index) else {
            break;
        };
        let open_tag = &body[absolute_start..=open_end];
        let content_start = open_end + 1;
        let Some(close_start) = find_ascii_case_insensitive(body, &close_needle, content_start) else {
            break;
        };
        let inline_body = body[content_start..close_start].trim();
        let external_script = tag == "script" && contains_ascii_case_insensitive(open_tag, " src=");
        assert!(external_script || inline_body.is_empty(), "inline {tag} should be absent: {inline_body}");
        offset = close_start + close_needle.len();
    }
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    haystack.as_bytes()[from..]
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
        .map(|index| from + index)
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    find_ascii_case_insensitive(haystack, needle, 0).is_some()
}

#[test]
fn inline_tag_scanner_preserves_offsets_after_non_ascii_text() {
    assert_no_inline_script_or_style("<main>İ</main><script src=\"/assets/app.js\"></script><style></style>");

    let inline = std::panic::catch_unwind(|| {
        assert_no_inline_script_or_style("<main>İ</main><script src=\"/assets/app.js\"></script><style>body{}</style>");
    });
    assert!(inline.is_err(), "inline style content must still be rejected");
}
