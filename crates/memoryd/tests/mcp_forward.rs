//! Behavioral coverage for `memoryd::mcp::forward_to_daemon`.
//!
//! The forwarder splits into two regimes:
//!
//!   1. Implemented tools (Search, Get, Note) round-trip through a live daemon
//!      and produce the expected substrate effect.
//!   2. Startup forwards through the live daemon/substrate Stream E path.

use std::path::Path;

use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::mcp::{
    forward_to_daemon, request_from_args, GetRequest, NoteRequest, ObserveKindRequest, ObserveRequest, SearchRequest,
    StartupRequest, ToolName, ToolRequest,
};
use memoryd::protocol::{RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};
use tokio::task::JoinHandle;

mod common;
use common::{shutdown, spawn_daemon, unique_socket_path, wait_for_socket};

#[tokio::test]
async fn forward_memory_note_then_search_then_get_round_trips_through_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("mcpfwd", "roundtrip");
    let substrate = init_substrate(&temp).await;
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    // Step 1: write a note via MemoryNote — this is the substrate-touching path.
    let note = forward_to_daemon(
        &socket,
        "req-note",
        ToolRequest::MemoryNote(NoteRequest { text: "captured pattern about caching".to_string() }),
    )
    .await
    .expect("note forward succeeds");
    let ResponseResult::Success(ResponsePayload::WriteNote(written)) = note.result else {
        panic!("expected WriteNote success, got {:?}", note.result);
    };
    assert!(!written.id.is_empty(), "memory id is assigned");
    assert!(written.summary.contains("captured pattern"), "summary echoes the note prefix");

    // Step 2: search for the note we just wrote.
    let search = forward_to_daemon(
        &socket,
        "req-search",
        ToolRequest::MemorySearch(SearchRequest {
            query: "captured pattern".to_string(),
            limit: Some(5),
            include_body: false,
        }),
    )
    .await
    .expect("search forward succeeds");
    let ResponseResult::Success(ResponsePayload::Search(found)) = search.result else {
        panic!("expected Search success, got {:?}", search.result);
    };
    assert!(found.total >= 1, "the note we just wrote must be searchable");
    let hit = found.hits.iter().find(|hit| hit.id == written.id).expect("hit matches the note we wrote");
    assert!(hit.snippet.len() <= 240, "snippets stay bounded by handler policy");

    // Step 3: read it back via MemoryGet.
    let get = forward_to_daemon(
        &socket,
        "req-get",
        ToolRequest::MemoryGet(GetRequest { id: written.id.clone(), include_provenance: false }),
    )
    .await
    .expect("get forward succeeds");
    let ResponseResult::Success(ResponsePayload::Get(record)) = get.result else {
        panic!("expected Get success, got {:?}", get.result);
    };
    assert_eq!(record.id, written.id);
    assert_eq!(record.body, "captured pattern about caching");
    assert!(!record.truncated, "single short note should not exceed the bounded preview");

    shutdown(shutdown_tx, server, &socket).await;
}

#[tokio::test]
async fn forward_memory_startup_forwards_required_binding_context_to_daemon() {
    let socket = unique_socket_path("mcpfwd", "startup");
    let daemon = spawn_single_request_daemon(&socket, |request| match request.request {
        RequestPayload::Startup(startup) => {
            assert_eq!(startup.cwd, "/tmp/project");
            assert_eq!(startup.session_id, "sess_mcp");
            assert_eq!(startup.harness, "codex");
            assert_eq!(startup.budget_tokens, Some(3_600));
            ResponseEnvelope::error(request.id, "not_implemented", "fixture stops after forwarding assertion", false)
        }
        other => panic!("expected startup request, got {other:?}"),
    })
    .await;

    let response = forward_to_daemon(&socket, "req-startup", ToolRequest::MemoryStartup(startup_request()))
        .await
        .expect("startup forwards to daemon");

    assert_not_implemented(&response, "fixture");
    daemon.await.expect("daemon joins").expect("daemon ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn forward_memory_startup_round_trips_through_live_substrate_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("mcpfwd", "startup-live");
    let substrate = init_substrate(&temp).await;
    let cwd = temp.path().join("repo");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    let response = forward_to_daemon(
        &socket,
        "req-startup-live",
        ToolRequest::MemoryStartup(StartupRequest {
            cwd: cwd.to_string_lossy().into_owned(),
            session_id: "sess_mcp_live".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            include_recent: true,
            since_event_id: None,
            budget_tokens: Some(3_600),
            passive: false,
        }),
    )
    .await
    .expect("startup forwards through live daemon");

    match response.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => {
            assert_eq!(startup.session_binding.session_id, "sess_mcp_live");
            assert!(startup.recall_block.starts_with("<memory-recall version=\"stream-e-v0.7\""));
        }
        other => panic!("expected live startup response, got {other:?}"),
    }

    shutdown(shutdown_tx, server, &socket).await;
}

#[tokio::test]
async fn forward_memory_observe_sends_observe_payload_to_daemon() {
    let socket = unique_socket_path("mcpfwd", "observe");
    let daemon = spawn_single_request_daemon(&socket, |request| match request.request {
        RequestPayload::Observe { text, kind, entities, cwd, session_id, harness, harness_version } => {
            assert_eq!(text, "agent noticed repeated cache invalidation churn");
            assert_eq!(kind, ObserveKindRequest::Pattern);
            assert_eq!(entities.len(), 2);
            assert_eq!(entities[0], "ent_cache");
            assert_eq!(entities[1], "ent_repo");
            assert_eq!(cwd, "/tmp/project");
            assert_eq!(session_id, "sess_mcp");
            assert_eq!(harness, "codex");
            assert_eq!(harness_version.as_deref(), Some("0.0.0"));
            ResponseEnvelope::error(
                request.id,
                "not_implemented",
                "fixture stops after observe forwarding assertion",
                false,
            )
        }
        other => panic!("expected observe request, got {other:?}"),
    })
    .await;

    let response = forward_to_daemon(
        &socket,
        "req-observe",
        ToolRequest::MemoryObserve(ObserveRequest {
            text: "agent noticed repeated cache invalidation churn".to_owned(),
            kind: ObserveKindRequest::Pattern,
            entities: vec!["ent_cache".to_owned(), "ent_repo".to_owned()],
            cwd: "/tmp/project".to_owned(),
            session_id: "sess_mcp".to_owned(),
            harness: "codex".to_owned(),
            harness_version: Some("0.0.0".to_owned()),
        }),
    )
    .await
    .expect("observe forwards to daemon");

    assert_not_implemented(&response, "observe");
    daemon.await.expect("daemon joins").expect("daemon ok");
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn forward_spec_shaped_memory_observe_sends_defaulted_binding_to_daemon() {
    let socket = unique_socket_path("mcpfwd", "observe-defaults");
    let daemon = spawn_single_request_daemon(&socket, |request| match request.request {
        RequestPayload::Observe { text, kind, entities, cwd, session_id, harness, harness_version } => {
            assert_eq!(text, "agent noticed repeated cache invalidation churn");
            assert_eq!(kind, ObserveKindRequest::Pattern);
            assert!(entities.is_empty());
            assert!(!cwd.is_empty());
            assert_eq!(session_id, "synthetic-memory-observe");
            assert_eq!(harness, "unknown");
            assert_eq!(harness_version, None);
            ResponseEnvelope::error(
                request.id,
                "not_implemented",
                "fixture stops after observe forwarding assertion",
                false,
            )
        }
        other => panic!("expected observe request, got {other:?}"),
    })
    .await;

    let request = request_from_args(
        ToolName::Observe,
        serde_json::json!({
            "text": "agent noticed repeated cache invalidation churn",
            "kind": "pattern"
        }),
    )
    .expect("spec-shaped observe request parses");

    let response =
        forward_to_daemon(&socket, "req-observe-defaults", request).await.expect("observe forwards to daemon");

    assert_not_implemented(&response, "observe");
    daemon.await.expect("daemon joins").expect("daemon ok");
    let _ = std::fs::remove_file(socket);
}

fn assert_not_implemented(response: &memoryd::protocol::ResponseEnvelope, tool: &str) {
    match &response.result {
        ResponseResult::Error(err) => {
            assert_eq!(err.code, "not_implemented", "{tool} must surface as not_implemented");
            assert!(!err.retryable, "{tool} not_implemented errors are not retryable");
            assert!(err.message.contains(tool), "error message names the tool");
        }
        other => panic!("expected Error result for {tool}, got {other:?}"),
    }
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_mcpforward".to_string()) })
        .await
        .expect("substrate init")
}

async fn spawn_single_request_daemon<F>(socket: &Path, assert_and_respond: F) -> JoinHandle<anyhow::Result<()>>
where
    F: FnOnce(memoryd::protocol::RequestEnvelope) -> ResponseEnvelope + Send + 'static,
{
    let _ = std::fs::remove_file(socket);
    let listener = tokio::net::UnixListener::bind(socket).expect("bind fake daemon socket");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut reader = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        reader.read_line(&mut line).await?;
        let request = memoryd::protocol::RequestEnvelope::from_json_line(&line)?;
        let response = assert_and_respond(request);
        reader.get_mut().write_all(response.to_json_line()?.as_bytes()).await?;
        Ok(())
    })
}

fn startup_request() -> StartupRequest {
    StartupRequest {
        cwd: "/tmp/project".to_owned(),
        session_id: "sess_mcp".to_owned(),
        harness: "codex".to_owned(),
        harness_version: None,
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(3_600),
        passive: false,
    }
}
