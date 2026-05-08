//! End-to-end coverage for the CLI client → server → handlers → substrate
//! path. Exercises the same `client::request` calls that `main.rs` makes for
//! the agent-facing subcommands, then proves the side effects landed by
//! shutting the daemon down and reopening the substrate from disk.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use memory_substrate::{InitOptions, MemoryId, Roots, Substrate};
use memoryd::client;
use memoryd::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use memoryd::server::{serve_substrate_with, ServerOptions};
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn cli_client_write_note_then_search_then_get_through_live_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("e2e-write-search-get");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(&roots).await;
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    // Step 1: write through the same path `memoryd write-note` uses.
    let response = client::request(
        &socket,
        "cli-write-note",
        RequestPayload::WriteNote { text: "live daemon end-to-end note".to_string() },
    )
    .await
    .expect("client::request reaches daemon");
    let ResponseResult::Success(ResponsePayload::WriteNote(write)) = response.result else {
        panic!("expected WriteNote success, got {:?}", response.result);
    };
    assert_eq!(response.id, "cli-write-note");
    let note_id = write.id;

    // Step 2: search through the same path `memoryd search` uses.
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
    assert!(found.hits.iter().any(|hit| hit.id == note_id), "search must find the just-written note");

    // Step 3: read back through the same path `memoryd get` uses.
    let response =
        client::request(&socket, "cli-get", RequestPayload::Get { id: note_id.clone(), include_provenance: false })
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
    let socket = unique_socket_path("e2e-shutdown");
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

fn spawn_daemon(socket: &Path, substrate: Substrate) -> (watch::Sender<bool>, JoinHandle<anyhow::Result<()>>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket = socket.to_path_buf();
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

async fn init_substrate(roots: &Roots) -> Substrate {
    Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_e2e".to_string()) },
    )
    .await
    .expect("substrate init")
}

fn unique_socket_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after epoch").as_nanos();
    let dir = PathBuf::from(format!("/tmp/memd-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create short socket directory");
    dir.join(format!("{test_name}-{nonce}.sock"))
}
