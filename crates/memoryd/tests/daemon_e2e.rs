//! End-to-end coverage for the CLI client → server → handlers → substrate
//! path. Exercises the same `client::request` calls that `main.rs` makes for
//! the agent-facing subcommands, then proves the side effects landed by
//! shutting the daemon down and reopening the substrate from disk.

use std::time::Duration;

use memory_substrate::{InitOptions, MemoryId, Roots, Substrate};
use memoryd::client;
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use tokio::net::UnixStream;
use tokio::time::timeout;

mod common;
use common::{shutdown, spawn_daemon, unique_socket_path, wait_for_socket};

#[tokio::test]
async fn cli_client_write_note_then_search_then_get_through_live_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("e2e", "write-search-get");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(&roots).await;
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    // Step 1: write through the same path `memoryd write-note` uses.
    let response = client::request(
        &socket,
        "cli-write-note",
        RequestPayload::WriteNote { text: "live daemon end-to-end note".to_string(), meta: serde_json::Value::Null },
    )
    .await
    .expect("client::request reaches daemon");
    let ResponseResult::Success(ResponsePayload::WriteNote(write)) = response.result else {
        panic!("expected WriteNote success, got {:?}", response.result);
    };
    assert_eq!(response.id, "cli-write-note");
    let note_id = write.id;

    // Step 2a: a fresh note is a governance candidate — search must NOT
    // surface it before review approval. (The pre-W0 FTS-degraded path leaked
    // candidate-status memories because it never filtered status; the unified
    // two-stage lane fences candidates exactly like the fused lane.)
    let response = client::request(
        &socket,
        "cli-search-candidate",
        RequestPayload::Search { query: "live daemon end-to-end".to_string(), limit: Some(5), include_body: false },
    )
    .await
    .expect("search reaches daemon");
    let ResponseResult::Success(ResponsePayload::Search(found)) = response.result else {
        panic!("expected Search success, got {:?}", response.result);
    };
    assert!(
        !found.hits.iter().any(|hit| hit.id == note_id),
        "candidate-status note must be fenced from search before approval"
    );

    // Step 2b: approve the note, then search must find it.
    let response = client::request(&socket, "cli-approve", RequestPayload::ReviewApprove { id: note_id.clone() })
        .await
        .expect("review approve reaches daemon");
    let ResponseResult::Success(ResponsePayload::ReviewApprove(_)) = response.result else {
        panic!("expected ReviewApprove success, got {:?}", response.result);
    };
    let response = client::request(
        &socket,
        "cli-search",
        RequestPayload::Search { query: "live daemon end-to-end".to_string(), limit: Some(5), include_body: false },
    )
    .await
    .expect("search reaches daemon");
    let ResponseResult::Success(ResponsePayload::Search(found)) = response.result else {
        panic!("expected Search success, got {:?}", response.result);
    };
    assert!(found.hits.iter().any(|hit| hit.id == note_id), "search must find the approved note");

    // Step 3: read back through the same path `memoryd get` uses.
    let response = client::request(
        &socket,
        "cli-get",
        RequestPayload::Get { id: note_id.clone(), include_provenance: false, full_body: false },
    )
    .await
    .expect("get reaches daemon");
    let ResponseResult::Success(ResponsePayload::Get(record)) = response.result else {
        panic!("expected Get success, got {:?}", response.result);
    };
    assert_eq!(record.id, note_id);
    assert_eq!(record.body, "live daemon end-to-end note");

    // Step 4: shut down cleanly. This is the only path that drops the
    // substrate's runtime lock; without it the reopen below would block.
    shutdown(shutdown_tx, server, &socket).await;

    // Step 5: prove the side effect actually persisted to disk by reopening
    // the substrate fresh and reading the memory directly. This catches a
    // class of bug — daemon caching lying to us — that no through-protocol
    // assertion can catch on its own.
    let reopened = Substrate::open(roots.clone()).await.expect("substrate reopens after shutdown");
    let memory = reopened.read_memory(&MemoryId::new(&note_id)).await.expect("memory persisted");
    assert_eq!(memory.body, "live daemon end-to-end note", "body matches what we wrote");
    assert_eq!(memory.frontmatter.id.as_str(), note_id);
}

#[tokio::test]
async fn server_shuts_down_promptly_when_signal_fires_with_in_flight_idle_connection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("e2e", "shutdown");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(&roots).await;
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    // Open a connection but send nothing. Without graceful shutdown, this
    // connection task would sit on `read_frame` until the idle timeout fires.
    // With graceful shutdown, the connection's read loop selects against the
    // shutdown receiver and exits immediately when the signal lands.
    let _idle = UnixStream::connect(&socket).await.expect("connection establishes");

    let started = std::time::Instant::now();
    shutdown_tx.send(true).expect("shutdown signal lands");
    timeout(Duration::from_secs(1), server)
        .await
        .expect("server stops well under the idle timeout window")
        .expect("server task joins")
        .expect("server returns Ok");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(1), "graceful shutdown must not wait for idle timeout (took {elapsed:?})");

    let _ = std::fs::remove_file(&socket);
}

async fn init_substrate(roots: &Roots) -> Substrate {
    Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_e2e".to_string()) },
    )
    .await
    .expect("substrate init")
}
