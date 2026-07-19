// Depends on dev-fixture auth; gated so plain `cargo check --all-targets`
// compiles without `--features dev-fixtures` (check.sh enables it).
#![cfg(feature = "dev-fixtures")]

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::server::{serve_substrate_with, ServerOptions};
use memoryd_web::{router_with_state, WebState, DEV_FIXTURE_DASHBOARD_AUTH_TOKEN};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::time::{sleep, timeout};
use tower::ServiceExt;

#[cfg(feature = "dev-fixtures")]
use axum::http::header;
#[cfg(feature = "dev-fixtures")]
use memoryd_web::fixture_router;

const RESPONSE_LIMIT: usize = 64 * 1024;

#[cfg(feature = "dev-fixtures")]
const PROJECT_POLICY: &str = r#"name: project-standard
version: 2
scope: project
confidence_floor: 0.7
requires_grounding: true
tombstone_enforcement: review
contradiction_policy: supersede
review_gates:
  - low_confidence
"#;

#[tokio::test]
async fn test_get_roi_daemon_returns_live_metrics_not_deferred_stub() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(temp.path()).await;
    let socket = unique_socket_path("web-roi", "daemon-live");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    let app = router_with_state(WebState::daemon(&socket));
    let token = fetch_csrf_token(app.clone()).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/roi?window=90")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["window_days"], 90);
    assert_eq!(body["promotion_rate"], 0.0);
    assert_eq!(body["promotion_precision"], 0.0);
    assert_eq!(body["refusal_breakdown"], json!({}));
    assert_ne!(body["status"], "not_implemented");

    shutdown(shutdown_tx, server, &socket).await;
}

#[cfg(feature = "dev-fixtures")]
#[tokio::test]
async fn test_get_policy_editor_returns_current_policy_yaml_not_deferred_stub() {
    let body = get_json(fixture_router(), "/api/policy-editor").await;

    assert_eq!(body["source"], "fixture");
    assert!(body["raw_yaml"].as_str().expect("raw yaml string").contains("project-standard"));
    assert!(body["policies"]
        .as_array()
        .expect("policies array")
        .iter()
        .any(|policy| { policy["scope"] == "project" && policy["selected_policy"] == "project-standard@v2" }));
    assert_ne!(body["status"], "not_implemented");
}

#[cfg(feature = "dev-fixtures")]
#[tokio::test]
async fn test_get_sync_dashboard_returns_sync_state_not_deferred_stub() {
    let body = get_json(fixture_router(), "/api/sync-dashboard").await;

    assert_eq!(body["sync"]["ahead"], 2);
    assert_eq!(body["sync"]["behind"], 0);
    assert_eq!(body["peer_presence"]["active_session_count"], 2);
    assert_eq!(body["claim_locks"]["active_count"], 0);
    assert_ne!(body["status"], "not_implemented");
}

#[cfg(feature = "dev-fixtures")]
#[tokio::test]
async fn test_post_policy_editor_validates_yaml_and_atomically_writes_policy_file() {
    let temp = tempfile::tempdir().expect("temp policy dir");
    seed_policy_dir(temp.path());
    let state = WebState::fixture().with_policy_dir(temp.path());
    let app = router_with_state(state);
    let token = fetch_csrf_token(app.clone()).await;
    let updated_project_policy = PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.72");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/policy-editor")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "file_name": "project-standard.yaml",
                        "raw_yaml": updated_project_policy,
                    })
                    .to_string(),
                ))
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["accepted"], true);
    assert_eq!(body["file_name"], "project-standard.yaml");
    assert_eq!(body["policies"].as_array().expect("policies array").len(), 4);
    let written = std::fs::read_to_string(temp.path().join("project-standard.yaml")).expect("policy file written");
    assert!(written.contains("confidence_floor: 0.72"));
    assert!(!temp.path().join("project-standard.yaml.tmp").exists());
}

#[cfg(feature = "dev-fixtures")]
#[tokio::test]
async fn test_post_policy_editor_rejects_invalid_yaml_before_write() {
    let temp = tempfile::tempdir().expect("temp policy dir");
    seed_policy_dir(temp.path());
    let state = WebState::fixture().with_policy_dir(temp.path());
    let app = router_with_state(state);
    let token = fetch_csrf_token(app.clone()).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/policy-editor")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "file_name": "project-standard.yaml",
                        "raw_yaml": "name: project-standard\nscope: project\nunknown: nope\n",
                    })
                    .to_string(),
                ))
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"], "invalid_governance_policy");
    let written = std::fs::read_to_string(temp.path().join("project-standard.yaml")).expect("policy file remains");
    assert_eq!(written, PROJECT_POLICY);
}

#[cfg(feature = "dev-fixtures")]
async fn get_json(app: axum::Router, route: &str) -> Value {
    // Data-bearing GET reads are gated behind the per-dashboard bearer token.
    let token = fetch_csrf_token(app.clone()).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(route)
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK, "{route}");
    json_body(response).await
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
    let html = response_body(response).await;
    csrf_token_from_html(&html).to_owned()
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_str(&response_body(response).await).expect("response is json")
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

#[cfg(feature = "dev-fixtures")]
fn seed_policy_dir(path: &std::path::Path) {
    std::fs::write(
        path.join("agent-strict.yaml"),
        PROJECT_POLICY
            .replace("name: project-standard", "name: agent-strict")
            .replace("version: 2", "version: 3")
            .replace("scope: project", "scope: agent")
            .replace("confidence_floor: 0.7", "confidence_floor: 0.82")
            .replace("tombstone_enforcement: review", "tombstone_enforcement: refuse")
            .replace("contradiction_policy: supersede", "contradiction_policy: quarantine")
            .replace("  - low_confidence\n", "  - low_confidence\n  - missing_grounding\n"),
    )
    .expect("agent policy");
    std::fs::write(
        path.join("dreaming-strict.yaml"),
        PROJECT_POLICY
            .replace("name: project-standard", "name: dreaming-strict")
            .replace("version: 2", "version: 1")
            .replace("scope: project", "scope: dreaming")
            .replace("confidence_floor: 0.7", "confidence_floor: 0.95")
            .replace("tombstone_enforcement: review", "tombstone_enforcement: refuse")
            .replace("contradiction_policy: supersede", "contradiction_policy: quarantine")
            .replace("  - low_confidence\n", "  - low_confidence\n  - missing_grounding\n  - dream_source\n"),
    )
    .expect("dreaming policy");
    std::fs::write(
        path.join("me-strict.yaml"),
        PROJECT_POLICY
            .replace("name: project-standard", "name: me-strict")
            .replace("version: 2", "version: 1")
            .replace("scope: project", "scope: me")
            .replace("confidence_floor: 0.7", "confidence_floor: 0.85")
            .replace("tombstone_enforcement: review", "tombstone_enforcement: refuse")
            .replace("contradiction_policy: supersede", "contradiction_policy: quarantine")
            .replace("  - low_confidence\n", "  - low_confidence\n  - missing_grounding\n"),
    )
    .expect("me policy");
    std::fs::write(path.join("project-standard.yaml"), PROJECT_POLICY).expect("project policy");
}

async fn init_substrate(root: &Path) -> Substrate {
    Substrate::init(
        Roots::new(root.join("repo"), root.join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_webroi".to_owned()) },
    )
    .await
    .expect("init substrate")
}

fn spawn_daemon(
    socket: &Path,
    substrate: Substrate,
) -> (watch::Sender<bool>, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket = socket.to_path_buf();
    let options = ServerOptions { idle_frame_timeout: Duration::from_secs(5) };
    let task = tokio::spawn(serve_substrate_with(socket, substrate, options, shutdown_rx));
    (shutdown_tx, task)
}

async fn shutdown(
    shutdown_tx: watch::Sender<bool>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
    socket: &Path,
) {
    shutdown_tx.send(true).expect("shutdown signal lands");
    timeout(Duration::from_secs(2), server)
        .await
        .expect("server stops before timeout")
        .expect("server task joins")
        .expect("server returns Ok");
    let _ = std::fs::remove_file(socket);
}

async fn wait_for_socket(socket: &Path) {
    for _ in 0..200 {
        if UnixStream::connect(socket).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind socket at {}", socket.display());
}

fn unique_socket_path(prefix: &str, test_name: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after epoch").as_nanos();
    let dir = PathBuf::from(format!("/tmp/memd-{prefix}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create short socket directory");
    dir.join(format!("{test_name}-{nonce}.sock"))
}
