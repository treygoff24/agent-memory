use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::server::{serve_substrate_with, ServerOptions};
use memoryd_web::{router_with_state, WebState};
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::time::{sleep, timeout, Duration};
use tower::ServiceExt;

const RESPONSE_LIMIT: usize = 64 * 1024;

#[tokio::test]
async fn test_daemon_backed_entity_graph_returns_empty_graph_not_deferred_stub() {
    let daemon = TestDaemon::start().await;

    let (status, body) = get_json(&daemon.socket, "/api/entity-graph").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["nodes"].as_array().expect("nodes array").len(), 0);
    assert_eq!(body["edges"].as_array().expect("edges array").len(), 0);
    assert_ne!(body["status"], "not_implemented");

    daemon.shutdown().await;
}

#[tokio::test]
async fn test_daemon_backed_entity_graph_and_detail_return_live_entities() {
    let daemon = TestDaemon::start().await;
    let memory_id = MemoryId::new("mem_20260503_cccccccccccccccc_000001");
    write_entity_memory(&daemon.substrate, memory_id.clone()).await;

    let (graph_status, graph) = get_json(&daemon.socket, "/api/entity-graph").await;
    assert_eq!(graph_status, StatusCode::OK);
    let nodes = graph["nodes"].as_array().expect("nodes array");
    assert!(nodes.iter().any(|node| {
        node["id"] == "ent_agent_memory" && node["label"] == "agent-memory" && node["memory_count"] == 1
    }));
    assert!(graph["edges"].as_array().expect("edges array").iter().any(|edge| {
        edge["source"] == "ent_agent_memory" && edge["target"] == "ent_stream_g" && edge["kind"] == "co_mentioned"
    }));

    let (detail_status, detail) = get_json(&daemon.socket, "/api/entity-graph/ent_agent_memory").await;
    assert_eq!(detail_status, StatusCode::OK);
    assert_eq!(detail["entity_id"], "ent_agent_memory");
    assert_eq!(detail["label"], "agent-memory");
    assert!(detail["mentions"].as_array().expect("mentions array").iter().any(|mention| mention == memory_id.as_str()));
    assert!(detail["related_memories"]
        .as_array()
        .expect("related memories array")
        .iter()
        .any(|memory| memory["id"] == memory_id.as_str()));
    assert_ne!(detail["status"], "not_implemented");

    daemon.shutdown().await;
}

struct TestDaemon {
    _temp: tempfile::TempDir,
    socket: std::path::PathBuf,
    substrate: Substrate,
    shutdown_tx: watch::Sender<bool>,
    server: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl TestDaemon {
    async fn start() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let socket = temp.path().join("memoryd.sock");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_webentity01".to_owned()) },
        )
        .await
        .expect("substrate init");
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server = tokio::spawn(serve_substrate_with(
            socket.clone(),
            substrate.clone(),
            ServerOptions { idle_frame_timeout: Duration::from_secs(5), ..ServerOptions::default() },
            shutdown_rx,
        ));
        wait_for_socket(&socket).await;

        Self { _temp: temp, socket, substrate, shutdown_tx, server }
    }

    async fn shutdown(self) {
        self.shutdown_tx.send(true).expect("shutdown signal lands");
        timeout(Duration::from_secs(2), self.server)
            .await
            .expect("server stops before timeout")
            .expect("server task joins")
            .expect("server exits ok");
        let _ = std::fs::remove_file(self.socket);
    }
}

async fn get_json(socket: &std::path::Path, route: &str) -> (StatusCode, Value) {
    let response = router_with_state(WebState::daemon(socket))
        .oneshot(Request::builder().uri(route).body(Body::empty()).expect("request builds"))
        .await
        .expect("request succeeds");
    let status = response.status();
    (status, serde_json::from_str(&response_body(response).await).expect("response is json"))
}

async fn response_body(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), RESPONSE_LIMIT).await.expect("body bytes are collected");
    String::from_utf8(bytes.to_vec()).expect("response is utf8")
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

async fn write_entity_memory(substrate: &Substrate, id: MemoryId) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: Memory {
                frontmatter: Frontmatter {
                    schema_version: 1,
                    id: id.clone(),
                    memory_type: MemoryType::Project,
                    scope: Scope::User,
                    summary: "Entity graph web fixture".to_owned(),
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
                        session_id: Some("sess_web_entity".to_owned()),
                        subagent_id: None,
                        phase: None,
                        component: None,
                    },
                    namespace: None,
                    canonical_namespace_id: None,
                    tags: Vec::new(),
                    entities: vec![
                        Entity {
                            id: "ent_agent_memory".to_owned(),
                            label: "agent-memory".to_owned(),
                            aliases: vec!["memorum".to_owned()],
                        },
                        Entity { id: "ent_stream_g".to_owned(), label: "Stream G".to_owned(), aliases: Vec::new() },
                    ],
                    aliases: Vec::new(),
                    source: Source {
                        kind: SourceKind::AgentPrimary,
                        reference: None,
                        harness: Some("codex".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_web_entity".to_owned()),
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
                        policy_applied: "web-entity-live-test".to_owned(),
                        expected_base_hash: None,
                    },
                    merge_diagnostics: None,
                    extras: BTreeMap::new(),
                },
                body: "Entity graph web fixture body".to_owned(),
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

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
