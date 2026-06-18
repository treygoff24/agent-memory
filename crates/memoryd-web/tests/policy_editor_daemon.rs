use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::server::{serve_substrate_with, ServerOptions};
use memoryd_web::{router_with_state, WebState, DEV_FIXTURE_DASHBOARD_AUTH_TOKEN};
use serde_json::{json, Value};
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::time::{sleep, timeout, Duration};
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;
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
async fn daemon_get_returns_writable_policy_editor_snapshot() {
    let daemon = TestDaemon::start().await;

    let (status, body) = get_json(&daemon.socket, "/api/policy-editor").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["source"], "disk");
    assert_eq!(body["writable"], true);
    assert_eq!(body["current_file"], "agent-strict.yaml");
    assert!(body["files"].as_array().expect("files array").iter().any(|file| file == "project-standard.yaml"));
    assert!(body["raw_yaml"].as_str().expect("raw yaml").contains("agent-strict"));
    assert!(body["policies"].as_array().expect("policies array").iter().any(|policy| policy["scope"] == "project"));

    daemon.shutdown().await;
}

#[tokio::test]
async fn daemon_get_fresh_repo_returns_writable_builtin_policy_templates() {
    let daemon = TestDaemon::start_fresh().await;

    let (status, body) = get_json(&daemon.socket, "/api/policy-editor").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["source"], "disk");
    assert_eq!(body["writable"], true);
    assert!(body["current_file"].as_str().is_some_and(|file| file.ends_with(".yaml")));
    assert!(body["raw_yaml"].as_str().expect("raw yaml").contains("name:"));
    assert!(body["files"].as_array().expect("files array").iter().any(|file| file == "project-standard.yaml"));
    assert!(daemon.repo.join("policies/project-standard.yaml").is_file());

    daemon.shutdown().await;
}

#[tokio::test]
async fn daemon_post_validates_and_persists_policy_file() {
    let daemon = TestDaemon::start().await;
    let app = router_with_state(WebState::daemon(&daemon.socket));
    let token = fetch_csrf_token(app.clone()).await;
    let updated = PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.72");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/policy-editor")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"file_name": "project-standard.yaml", "raw_yaml": updated}).to_string()))
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["accepted"], true);
    assert_eq!(body["file_name"], "project-standard.yaml");
    assert!(std::fs::read_to_string(daemon.repo.join("policies/project-standard.yaml"))
        .expect("policy readable")
        .contains("confidence_floor: 0.72"));

    daemon.shutdown().await;
}

#[tokio::test]
async fn daemon_post_fresh_repo_creates_complete_policy_set() {
    let daemon = TestDaemon::start_fresh().await;
    let app = router_with_state(WebState::daemon(&daemon.socket));
    let token = fetch_csrf_token(app.clone()).await;
    let updated = PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.72");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/policy-editor")
                .header("x-memorum-dashboard-auth", DEV_FIXTURE_DASHBOARD_AUTH_TOKEN)
                .header("x-memorum-csrf", token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"file_name": "project-standard.yaml", "raw_yaml": updated}).to_string()))
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["accepted"], true);
    assert!(daemon.repo.join("policies/me-strict.yaml").is_file());
    assert!(daemon.repo.join("policies/agent-strict.yaml").is_file());
    assert!(daemon.repo.join("policies/dreaming-strict.yaml").is_file());
    assert!(std::fs::read_to_string(daemon.repo.join("policies/project-standard.yaml"))
        .expect("policy readable")
        .contains("confidence_floor: 0.72"));

    daemon.shutdown().await;
}

#[tokio::test]
async fn daemon_post_invalid_yaml_does_not_mutate_policy_file() {
    let daemon = TestDaemon::start().await;
    let app = router_with_state(WebState::daemon(&daemon.socket));
    let token = fetch_csrf_token(app.clone()).await;
    let original =
        std::fs::read_to_string(daemon.repo.join("policies/project-standard.yaml")).expect("policy readable");

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
                        "raw_yaml": "name: project-standard\nscope: project\nunexpected: nope\n"
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
    assert_eq!(
        std::fs::read_to_string(daemon.repo.join("policies/project-standard.yaml")).expect("policy readable"),
        original
    );

    daemon.shutdown().await;
}

struct TestDaemon {
    _temp: tempfile::TempDir,
    repo: std::path::PathBuf,
    socket: std::path::PathBuf,
    shutdown_tx: watch::Sender<bool>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl TestDaemon {
    async fn start() -> Self {
        Self::start_with_seed(true).await
    }

    async fn start_fresh() -> Self {
        Self::start_with_seed(false).await
    }

    async fn start_with_seed(seed_policies: bool) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let socket = temp.path().join("memoryd.sock");
        let roots = Roots::new(&repo, temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots,
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_webpolicy".to_owned()) },
        )
        .await
        .expect("substrate init");
        if seed_policies {
            seed_policy_dir(&repo.join("policies"));
        }
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = tokio::spawn(serve_substrate_with(
            socket.clone(),
            substrate,
            ServerOptions { idle_frame_timeout: Duration::from_secs(2) },
            shutdown_rx,
        ));
        wait_for_socket(&socket).await;
        Self { _temp: temp, repo, socket, shutdown_tx, server }
    }

    async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        timeout(Duration::from_secs(2), self.server)
            .await
            .expect("server stops before timeout")
            .expect("server task joins")
            .expect("server exits ok");
        let _ = std::fs::remove_file(self.socket);
    }
}

async fn get_json(socket: &std::path::Path, route: &str) -> (StatusCode, Value) {
    // Data-bearing GET reads are gated behind the per-dashboard bearer token.
    let app = router_with_state(WebState::daemon(socket));
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
    let status = response.status();
    (status, json_body(response).await)
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

async fn wait_for_socket(socket: &std::path::Path) {
    for _ in 0..100 {
        if UnixStream::connect(socket).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind socket at {}", socket.display());
}

fn seed_policy_dir(path: &std::path::Path) {
    std::fs::create_dir_all(path).expect("policy dir");
    std::fs::write(path.join("me-strict.yaml"), PolicyFixture::me().to_yaml()).expect("me policy");
    std::fs::write(path.join("project-standard.yaml"), PROJECT_POLICY).expect("project policy");
    std::fs::write(path.join("agent-strict.yaml"), PolicyFixture::agent().to_yaml()).expect("agent policy");
    std::fs::write(path.join("dreaming-strict.yaml"), PolicyFixture::dreaming().to_yaml()).expect("dreaming policy");
}

struct PolicyFixture<'a> {
    name: &'a str,
    version: u32,
    scope: &'a str,
    confidence_floor: &'a str,
    tombstone: &'a str,
    contradiction: &'a str,
    gates: &'a [&'a str],
}

impl<'a> PolicyFixture<'a> {
    fn me() -> Self {
        Self {
            name: "me-strict",
            version: 1,
            scope: "me",
            confidence_floor: "0.85",
            tombstone: "refuse",
            contradiction: "quarantine",
            gates: &["low_confidence", "missing_grounding"],
        }
    }

    fn agent() -> Self {
        Self {
            name: "agent-strict",
            version: 3,
            scope: "agent",
            confidence_floor: "0.82",
            tombstone: "refuse",
            contradiction: "quarantine",
            gates: &["low_confidence", "missing_grounding"],
        }
    }

    fn dreaming() -> Self {
        Self {
            name: "dreaming-strict",
            version: 1,
            scope: "dreaming",
            confidence_floor: "0.95",
            tombstone: "refuse",
            contradiction: "quarantine",
            gates: &["low_confidence", "missing_grounding", "dream_source"],
        }
    }

    fn to_yaml(&self) -> String {
        let review_gates = self.gates.iter().map(|gate| format!("  - {gate}\n")).collect::<String>();
        format!(
            "name: {}\nversion: {}\nscope: {}\nconfidence_floor: {}\nrequires_grounding: true\ntombstone_enforcement: {}\ncontradiction_policy: {}\nreview_gates:\n{}",
            self.name,
            self.version,
            self.scope,
            self.confidence_floor,
            self.tombstone,
            self.contradiction,
            review_gates
        )
    }
}
