use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use chrono::{DateTime, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::StartupRequest;
use memoryd::server::{serve_substrate_with, ServerOptions};
use memoryd_web::{fixture_router, router, router_with_state, WebState};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::time::{sleep, timeout, Duration};
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;
const REVIEWABLE_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000001";
const NON_REVIEW_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000099";
const AUDIT_MEMORY_ID: &str = "mem_20260501_a1b2c3d4e5f60718_000010";
const AUDIT_BODY: &str = "Task 14 audit-only fixture body";

#[tokio::test]
async fn test_get_status_returns_correct_shape() {
    let response = get_json("/api/status").await;

    assert_eq!(response["daemon"]["version"], "0.1.0-test");
    assert_eq!(response["socket"], "ok");
    assert!(response["index"]["active_memories"].as_u64().is_some());
    assert!(response["sync"].is_object());
    assert!(response["review"].is_object());
    assert!(response["active_sessions"].is_array());
    assert!(response["dreaming"].is_object());
    assert!(response["recall"].is_object());
}

#[tokio::test]
async fn test_default_router_does_not_serve_fixture_dashboard_data() {
    let response = router()
        .oneshot(Request::builder().uri("/api/status").body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = json_body(response).await;
    assert_eq!(body["error"], "dashboard_backend_unavailable");
}

#[tokio::test]
async fn test_daemon_router_attempts_socket_backend_instead_of_backendless_router() {
    let missing_socket = tempfile::NamedTempFile::new().expect("missing socket placeholder is created");
    let response = router_with_state(WebState::daemon(missing_socket.path()))
        .oneshot(Request::builder().uri("/api/status").body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = json_body(response).await;
    assert_eq!(body["error"], "daemon_request_failed");
    assert_eq!(body["code"], "daemon_unavailable");
    assert_ne!(body["error"], "dashboard_backend_unavailable");
}

#[tokio::test]
async fn test_daemon_backed_recall_hits_route_surfaces_live_recall_emission() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let socket = temp.path().join("memoryd.sock");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_webrecall01".to_owned()) },
    )
    .await
    .expect("substrate init");
    let memory_id = MemoryId::new("mem_20260502_bbbbbbbbbbbbbbbb_000001");
    write_recall_hit_memory(&substrate, memory_id.clone()).await;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(serve_substrate_with(
        socket.clone(),
        substrate,
        ServerOptions { idle_frame_timeout: Duration::from_secs(5) },
        shutdown_rx,
    ));
    wait_for_socket(&socket).await;

    let startup = memoryd::client::request(
        &socket,
        "web-recall-hit-startup",
        RequestPayload::Startup(StartupRequest {
            cwd: repo.to_string_lossy().into_owned(),
            session_id: "sess_web_recall_hits".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            include_recent: true,
            since_event_id: None,
            budget_tokens: Some(1024),
        }),
    )
    .await
    .expect("startup request succeeds");
    match startup.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => {
            assert!(startup.recall_block.contains(memory_id.as_str()));
        }
        other => panic!("expected startup success, got {other:?}"),
    }

    let response = router_with_state(WebState::daemon(&socket))
        .oneshot(
            Request::builder()
                .uri("/api/recall-hits?since=2026-05-01T00:00:00Z&limit=10")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let hits = body["hits"].as_array().expect("hits array");
    assert!(
        hits.iter().any(|hit| hit["memory_id"] == memory_id.as_str()),
        "daemon-backed web API should surface the RecallHit emitted by startup recall: {body}"
    );

    shutdown_tx.send(true).expect("shutdown signal lands");
    timeout(Duration::from_secs(2), server)
        .await
        .expect("server stops before timeout")
        .expect("server task joins")
        .expect("server exits ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn test_get_entity_graph_returns_nodes_and_edges() {
    let response = get_json("/api/entity-graph?namespace=project:agent-memory&depth=2").await;

    let nodes = response["nodes"].as_array().expect("nodes array exists");
    let edges = response["edges"].as_array().expect("edges array exists");

    assert!(!nodes.is_empty());
    assert!(!edges.is_empty());
    assert_eq!(nodes[0]["id"], "ent_agent_memory");
    assert_eq!(edges[0]["kind"], "co_mentioned");
}

#[tokio::test]
async fn test_post_review_action_approve_calls_daemon() {
    let state = WebState::fixture();
    let app = router_with_state(state.clone());
    let token = fetch_csrf_token(app.clone()).await;

    let response =
        app.oneshot(review_action_request(&token, REVIEWABLE_MEMORY_ID, "approve")).await.expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["id"], REVIEWABLE_MEMORY_ID);
    assert_eq!(body["action"], "approve");

    let recorded = state.recorded_review_actions().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].id, REVIEWABLE_MEMORY_ID);
    assert_eq!(recorded[0].action, "approve");
}

#[tokio::test]
async fn test_post_review_action_returns_409_on_wrong_state() {
    let app = fixture_router();
    let token = fetch_csrf_token(app.clone()).await;

    let response =
        app.oneshot(review_action_request(&token, NON_REVIEW_MEMORY_ID, "approve")).await.expect("request succeeds");

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = json_body(response).await;
    assert_eq!(body, json!({ "error": "memory_not_in_review_state" }));
}

#[tokio::test]
async fn test_get_audit_returns_full_trust_artifact() {
    let response = get_json(&format!("/api/audit/{AUDIT_MEMORY_ID}")).await;

    assert_eq!(response["memory_id"], AUDIT_MEMORY_ID);
    assert_eq!(response["title"], "Task 14 audit fixture");
    assert_eq!(response["body"], AUDIT_BODY);
    assert_eq!(response["status"], "active");
    assert_eq!(response["namespace"], "project:agent-memory");
    assert_eq!(response["confidence"], 0.95);
    assert_eq!(response["recall_count_total"], 28);
    assert_eq!(response["recall_count_30d"], 12);
    assert_eq!(response["last_recalled"], "2026-05-01T12:00:00Z");
    assert!(response["provenance_chain"].is_array());
    assert!(response["policy_decisions"].is_array());
    assert!(response["privacy_scan"].is_object());
    assert!(response["supersession_history"].is_array());
    assert!(response["sync_state"].is_object());
    assert!(response.get("artifact").is_none());
    assert!(response.get("sections").is_none());
}

#[tokio::test]
async fn test_get_audit_temporal_returns_historical_state() {
    let response = get_json(&format!("/api/audit/{AUDIT_MEMORY_ID}/temporal?at=2026-04-30T12:00:00Z")).await;

    assert_eq!(response["memory_id"], AUDIT_MEMORY_ID);
    assert_eq!(response["at"], "2026-04-30T12:00:00Z");
    assert_eq!(response["viewing_historical_state"], true);
    assert_eq!(response["artifact"]["id"], AUDIT_MEMORY_ID);
}

#[tokio::test]
async fn test_get_audit_walk_returns_provenance_graph_not_deferred_stub() {
    let response = fixture_router()
        .oneshot(
            Request::builder()
                .uri(format!("/api/audit/{AUDIT_MEMORY_ID}/walk?direction=up&depth=2"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["memory_id"], AUDIT_MEMORY_ID);
    assert_eq!(body["direction"], "up");
    assert!(body["nodes"].as_array().expect("nodes array").len() >= 2);
    assert!(body["edges"].as_array().expect("edges array").iter().any(|edge| edge["kind"] == "provenance"));
    assert_ne!(body["status"], "not_implemented");
}

#[tokio::test]
async fn test_get_audit_walk_rejects_invalid_direction() {
    let response = fixture_router()
        .oneshot(
            Request::builder()
                .uri(format!("/api/audit/{AUDIT_MEMORY_ID}/walk?direction=sideways"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["error"], "invalid_query");
    assert_eq!(body["route"], "audit_walk");
}

#[tokio::test]
async fn test_get_audit_walk_accepts_down_direction() {
    let response = get_json(&format!("/api/audit/{AUDIT_MEMORY_ID}/walk?direction=down&depth=2")).await;

    assert_eq!(response["memory_id"], AUDIT_MEMORY_ID);
    assert_eq!(response["direction"], "down");
    assert!(response["edges"].as_array().expect("edges array").iter().any(|edge| edge["kind"] == "supersedes"));
}

#[tokio::test]
async fn test_get_recall_hits_returns_recent_recall_hit_surface() {
    let response = get_json("/api/recall-hits?limit=1").await;

    assert_eq!(response["limit"], 1);
    let hits = response["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["memory_id"], REVIEWABLE_MEMORY_ID);
    assert_eq!(hits[0]["recalled_at"], "2026-05-01T12:00:00Z");
    assert_eq!(hits[0]["summary"], "Review Stream G dashboard contract");
}

#[tokio::test]
async fn test_get_roi_30d_returns_correct_window() {
    let response = get_json("/api/roi?window=30").await;

    assert_eq!(response["window_days"], 30);
    assert!(response["promotion_rate"].as_f64().is_some());
    assert!(response["refusal_breakdown"].is_object());
    assert!(response["dreaming"].is_object());
}

#[tokio::test]
async fn test_get_roi_365d_returns_correct_window() {
    let response = get_json("/api/roi?window=365").await;

    assert_eq!(response["window_days"], 365);
}

#[tokio::test]
async fn test_get_reality_check_returns_pending_list() {
    let response = get_json("/api/reality-check").await;

    assert_eq!(response["kind"], "pending");
    assert_eq!(response["session_id"], "rc_session_task14");
    assert!(!response["items"].as_array().expect("items array exists").is_empty());
    assert_eq!(response["items"][0]["memory_id"], REVIEWABLE_MEMORY_ID);
}

#[tokio::test]
async fn test_post_reality_check_respond_dispatches_to_daemon() {
    let state = WebState::fixture();
    let app = router_with_state(state.clone());
    let token = fetch_csrf_token(app.clone()).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/reality-check/respond")
                .header("x-memorum-csrf", token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "memory_id": REVIEWABLE_MEMORY_ID,
                        "action": "confirm"
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

    let recorded = state.recorded_reality_check_actions().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].memory_id, REVIEWABLE_MEMORY_ID);
    assert_eq!(recorded[0].action, "confirm");
}

#[tokio::test]
async fn test_notifications_stream_returns_sse_heartbeat_snapshot() {
    let response = fixture_router()
        .oneshot(Request::builder().uri("/api/notifications/stream").body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type =
        response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(content_type.starts_with("text/event-stream"));

    let body = response_body(response).await;
    assert!(body.contains("event: heartbeat"));
    assert!(body.contains("review_queue_over"));
}

#[tokio::test]
async fn test_dashboard_future_sections_are_real_json_routes() {
    for route in ["/api/policy-editor", "/api/sync-dashboard"] {
        let response = fixture_router()
            .oneshot(Request::builder().uri(route).body(Body::empty()).expect("request builds"))
            .await
            .expect("request succeeds");

        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_json_content_type(&response, route);
        let body = json_body(response).await;
        assert_ne!(body["status"], "not_implemented");
    }
}

#[tokio::test]
async fn test_non_audit_routes_do_not_leak_audit_body() {
    for route in [
        "/api/status",
        "/api/entity-graph",
        "/api/entity-graph/ent_agent_memory",
        "/api/roi",
        "/api/reality-check",
        "/api/reality-check/history",
        "/api/recall-hits",
        "/api/review",
    ] {
        let response = fixture_router()
            .oneshot(Request::builder().uri(route).body(Body::empty()).expect("request builds"))
            .await
            .expect("request succeeds");
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_json_content_type(&response, route);
        let body = response_body(response).await;
        assert!(!body.contains(AUDIT_BODY), "{route} leaked audit body");
    }
}

async fn get_json(route: &str) -> Value {
    let response = fixture_router()
        .oneshot(Request::builder().uri(route).body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");

    assert_eq!(response.status(), StatusCode::OK, "{route}");
    assert_json_content_type(&response, route);
    json_body(response).await
}

fn review_action_request(csrf_token: &str, id: &str, action: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/review/action")
        .header("x-memorum-csrf", csrf_token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": id, "action": action, "reason": "test" }).to_string()))
        .expect("request builds")
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

fn assert_json_content_type(response: &axum::response::Response, route: &str) {
    let content_type =
        response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(content_type.starts_with("application/json"), "{route}: {content_type}");
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

async fn write_recall_hit_memory(substrate: &Substrate, id: MemoryId) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: Memory {
                frontmatter: Frontmatter {
                    schema_version: 1,
                    id: id.clone(),
                    memory_type: MemoryType::Project,
                    scope: Scope::User,
                    summary: "Live recall-hit web fixture".to_owned(),
                    confidence: 0.9,
                    original_confidence: None,
                    trust_level: TrustLevel::Trusted,
                    sensitivity: Sensitivity::Internal,
                    status: MemoryStatus::Pinned,
                    created_at: instant("2026-05-01T00:00:00Z"),
                    updated_at: instant("2026-05-01T00:00:00Z"),
                    observed_at: None,
                    author: Author {
                        kind: AuthorKind::Agent,
                        user_handle: None,
                        harness: Some("codex".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_web_recall_hits".to_owned()),
                        subagent_id: None,
                        phase: None,
                        component: None,
                    },
                    namespace: None,
                    canonical_namespace_id: None,
                    tags: Vec::new(),
                    entities: Vec::new(),
                    aliases: Vec::new(),
                    source: Source {
                        kind: SourceKind::AgentPrimary,
                        reference: None,
                        harness: Some("codex".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_web_recall_hits".to_owned()),
                        subagent_id: None,
                        device: None,
                    },
                    evidence: Vec::new(),
                    requires_user_confirmation: false,
                    review_state: None,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    related: Vec::new(),
                    tombstone_events: Vec::new(),
                    retrieval_policy: RetrievalPolicy {
                        passive_recall: true,
                        max_scope: Scope::User,
                        mask_personal_for_synthesis: false,
                        index_body: true,
                        index_embeddings: true,
                    },
                    write_policy: WritePolicy {
                        human_review_required: false,
                        policy_applied: "web-recall-hit-live-test".to_owned(),
                        expected_base_hash: None,
                    },
                    merge_diagnostics: None,
                    extras: BTreeMap::new(),
                },
                body: "Live recall-hit web fixture body".to_owned(),
                path: Some(RepoPath::new(format!("me/{}.md", id.as_str()))),
            },
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("fixture write");
}

async fn wait_for_socket(socket: &std::path::Path) {
    for _ in 0..200 {
        if UnixStream::connect(socket).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind socket at {}", socket.display());
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
