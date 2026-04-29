//! Behavioral coverage for `memoryd::mcp::forward_to_daemon`.
//!
//! The forwarder splits into two regimes:
//!
//!   1. Implemented tools (Search, Get, Note) round-trip through a live daemon
//!      and produce the expected substrate effect.
//!   2. Unimplemented tools (Write, Supersede, Forget, Startup) short-circuit
//!      with a structured `not_implemented` envelope and never contact the
//!      daemon, so they can be tested without spinning anything up.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::mcp::{
    forward_to_daemon, ForgetRequest, GetRequest, NoteRequest, SearchRequest, StartupRequest, SupersedeRequest,
    ToolRequest, WriteRequest as McpWriteRequest,
};
use memoryd::protocol::{ResponsePayload, ResponseResult};
use memoryd::server::{serve_substrate_with, ServerOptions};
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn forward_memory_note_then_search_then_get_round_trips_through_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("mcp-roundtrip");
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

/// MemoryWrite refuses with `not_implemented` because the daemon's WriteNote
/// payload cannot represent structured title/tags. The forwarder must
/// short-circuit before touching the socket — verified here by passing a
/// path that points nowhere; if the forwarder did attempt to connect, the
/// connect would fail and the test would surface that as an `Err`.
#[tokio::test]
async fn forward_memory_write_short_circuits_with_not_implemented() {
    let response = forward_to_daemon(
        std::path::Path::new("/nonexistent/socket/should/never/be/touched.sock"),
        "req-write",
        ToolRequest::MemoryWrite(McpWriteRequest {
            body: "structured body".to_string(),
            title: Some("a title".to_string()),
            tags: vec!["alpha".to_string()],
        }),
    )
    .await
    .expect("forward returns Ok with structured error, not Err");

    assert_not_implemented(&response, "memory_write");
    assert_eq!(response.id, "req-write");
}

#[tokio::test]
async fn forward_memory_supersede_short_circuits_with_not_implemented() {
    let response = forward_to_daemon(
        std::path::Path::new("/nonexistent/socket/should/never/be/touched.sock"),
        "req-supersede",
        ToolRequest::MemorySupersede(SupersedeRequest {
            old_id: "mem_old".to_string(),
            new_body: "replacement".to_string(),
            reason: "outdated".to_string(),
        }),
    )
    .await
    .expect("forward returns Ok with structured error, not Err");

    assert_not_implemented(&response, "memory_supersede");
}

#[tokio::test]
async fn forward_memory_forget_short_circuits_with_not_implemented() {
    let response = forward_to_daemon(
        std::path::Path::new("/nonexistent/socket/should/never/be/touched.sock"),
        "req-forget",
        ToolRequest::MemoryForget(ForgetRequest {
            id: "mem_to_forget".to_string(),
            reason: "user requested".to_string(),
        }),
    )
    .await
    .expect("forward returns Ok with structured error, not Err");

    assert_not_implemented(&response, "memory_forget");
}

#[tokio::test]
async fn forward_memory_startup_short_circuits_with_not_implemented() {
    let response = forward_to_daemon(
        std::path::Path::new("/nonexistent/socket/should/never/be/touched.sock"),
        "req-startup",
        ToolRequest::MemoryStartup(StartupRequest { include_recent: true }),
    )
    .await
    .expect("forward returns Ok with structured error, not Err");

    assert_not_implemented(&response, "memory_startup");
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

fn spawn_daemon(socket: &Path, substrate: Substrate) -> (watch::Sender<bool>, JoinHandle<anyhow::Result<()>>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket = socket.to_path_buf();
    // Tight idle timeout so a misbehaving test cannot hang the runtime.
    let options = ServerOptions { idle_frame_timeout: Duration::from_secs(5) };
    let task = tokio::spawn(serve_substrate_with(socket, substrate, options, shutdown_rx));
    (shutdown_tx, task)
}

async fn shutdown(shutdown_tx: watch::Sender<bool>, server: JoinHandle<anyhow::Result<()>>, socket: &Path) {
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

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_mcpforward".to_string()) })
        .await
        .expect("substrate init")
}

fn unique_socket_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after epoch").as_nanos();
    std::env::temp_dir().join(format!("memoryd-{test_name}-{nonce}.sock"))
}
