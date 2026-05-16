use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use memoryd_web::{fixture_router, router_with_state, WebState};
use serde_json::{json, Value};
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

#[tokio::test]
async fn test_get_policy_editor_returns_disk_policy_files_and_raw_yaml() {
    let temp = tempfile::tempdir().expect("temp policy dir");
    seed_policy_dir(temp.path());
    let state = WebState::fixture().with_policy_dir(temp.path());
    let body = get_json(router_with_state(state), "/api/policy-editor").await;

    assert_eq!(body["source"], "disk");
    assert_eq!(body["writable"], true);
    assert_eq!(
        body["files"],
        json!(["agent-strict.yaml", "dreaming-strict.yaml", "me-strict.yaml", "project-standard.yaml"])
    );
    let raw_yaml = body["raw_yaml"].as_str().expect("raw yaml string");
    assert!(raw_yaml.contains("# file: project-standard.yaml"));
    assert!(raw_yaml.contains(PROJECT_POLICY));
    assert_project_policy_summary(&body, json!(0.7));
}

#[tokio::test]
async fn test_get_sync_dashboard_returns_sync_state_not_deferred_stub() {
    let body = get_json(fixture_router(), "/api/sync-dashboard").await;

    assert_eq!(body["sync"]["ahead"], 2);
    assert_eq!(body["sync"]["behind"], 0);
    assert_eq!(body["peer_presence"]["active_session_count"], 2);
    assert_eq!(body["claim_locks"]["active_count"], 0);
    assert_ne!(body["status"], "not_implemented");
}

#[tokio::test]
async fn test_post_policy_editor_validates_yaml_and_atomically_writes_policy_file() {
    let temp = tempfile::tempdir().expect("temp policy dir");
    seed_policy_dir(temp.path());
    let state = WebState::fixture().with_policy_dir(temp.path());
    let app = router_with_state(state);
    let token = fetch_csrf_token(app.clone()).await;
    let updated_project_policy = PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.72");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/policy-editor")
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
    assert_project_policy_summary(&body, json!(0.72));
    let written = std::fs::read_to_string(temp.path().join("project-standard.yaml")).expect("policy file written");
    assert!(written.contains("confidence_floor: 0.72"));
    assert!(!temp.path().join("project-standard.yaml.tmp").exists());

    let get_body = get_json(app, "/api/policy-editor").await;
    assert_eq!(get_body["source"], "disk");
    assert!(get_body["raw_yaml"].as_str().expect("raw yaml string").contains("confidence_floor: 0.72"));
    assert_project_policy_summary(&get_body, json!(0.72));
}

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

async fn get_json(app: axum::Router, route: &str) -> Value {
    let response = app
        .oneshot(Request::builder().uri(route).body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK, "{route}");
    json_body(response).await
}

async fn fetch_csrf_token(app: axum::Router) -> String {
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).expect("request builds"))
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

fn assert_project_policy_summary(body: &Value, confidence_floor: Value) {
    let policy = policy_summary(body, "project");
    assert_eq!(policy["selected_policy"], "project-standard@v2");
    assert_eq!(policy["policy_source"], "disk");
    assert_eq!(policy["confidence_floor"], confidence_floor);
    assert_eq!(policy["requires_grounding"], true);
}

fn policy_summary<'a>(body: &'a Value, scope: &str) -> &'a Value {
    body["policies"]
        .as_array()
        .expect("policies array")
        .iter()
        .find(|policy| policy["scope"] == scope)
        .unwrap_or_else(|| panic!("policy summary for {scope} exists"))
}

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
