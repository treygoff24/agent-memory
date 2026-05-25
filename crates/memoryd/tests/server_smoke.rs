use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};
use memoryd::server::{serve, MAX_FRAME_BYTES};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::time::{sleep, timeout, Duration};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn server_smoke_serves_status_over_newline_delimited_json() {
    let socket_path = unique_socket_path("status");
    let stale_listener = UnixListener::bind(&socket_path).expect("stale socket binds");
    drop(stale_listener);

    let server = tokio::spawn(serve(socket_path.clone()));
    let stream = connect_with_retry(&socket_path).await;
    let mut stream = BufReader::new(stream);
    assert_owner_only_socket(&socket_path);

    let request = RequestEnvelope::new("req-status", RequestPayload::Status);
    stream
        .get_mut()
        .write_all(request.to_json_line().expect("request serializes").as_bytes())
        .await
        .expect("request writes");

    let mut line = String::new();
    timeout(Duration::from_secs(1), stream.read_line(&mut line))
        .await
        .expect("server responds before timeout")
        .expect("response line reads");

    let response = ResponseEnvelope::from_json_line(&line).expect("response decodes");
    assert_eq!(response.id, "req-status");
    let ResponseResult::Success(ResponsePayload::Status(mut status)) = response.result else {
        panic!("expected status success, got {:?}", response.result);
    };
    let daemon = status.daemon.take().expect("healthy status reports daemon process info");
    assert_eq!(daemon.version, env!("CARGO_PKG_VERSION"));
    assert!(daemon.pid > 0, "daemon pid must be a real process id");
    assert_eq!(
        status,
        memoryd::protocol::StatusResponse {
            state: "healthy".to_owned(),
            guidance: "memoryd local daemon is accepting requests; substrate is not attached yet".to_owned(),
            recall: Default::default(),
            dreams: Default::default(),
            passive_notifications: Default::default(),
            ..Default::default()
        }
    );

    server.abort();
    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn server_refuses_to_replace_regular_file_socket_path() {
    let socket_path = unique_socket_path("regular-file");
    std::fs::write(&socket_path, b"do not delete me").expect("regular file fixture created");

    let result = timeout(Duration::from_secs(1), serve(socket_path.clone()))
        .await
        .expect("regular-file socket path fails fast instead of binding forever");

    let error = result.expect_err("regular file socket path must be rejected");
    let error_text = format!("{error:#}");
    assert!(error_text.contains("refusing to remove non-socket path"), "unexpected error: {error_text}");
    assert_eq!(
        std::fs::read(&socket_path).expect("regular file preserved"),
        b"do not delete me",
        "daemon must not delete arbitrary files passed as --socket"
    );

    let _ = std::fs::remove_file(socket_path);
}

#[tokio::test]
async fn server_does_not_chmod_existing_socket_parent_directory() {
    let socket_path = unique_socket_path("parent-mode");
    let parent = socket_path.parent().expect("socket has parent").to_path_buf();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).expect("set parent mode fixture");

    let server = tokio::spawn(serve(socket_path.clone()));
    let _stream = connect_with_retry(&socket_path).await;

    let parent_mode = std::fs::metadata(&parent).expect("parent metadata").permissions().mode() & 0o777;
    assert_eq!(parent_mode, 0o755, "daemon must not chmod an existing caller-owned directory");

    server.abort();
    let _ = std::fs::remove_file(socket_path);
}

/// Malformed JSON must produce a structured `invalid_request` error envelope, not silence
/// (which would leave the client hanging indefinitely).
#[tokio::test]
async fn server_smoke_returns_error_for_malformed_json() {
    let socket_path = unique_socket_path("malformed");
    let server = tokio::spawn(serve(socket_path.clone()));
    let stream = connect_with_retry(&socket_path).await;
    let mut stream = BufReader::new(stream);

    // Send a line that is not valid JSON.
    stream.get_mut().write_all(b"not valid json at all\n").await.expect("malformed request writes");

    let mut line = String::new();
    timeout(Duration::from_secs(1), stream.read_line(&mut line))
        .await
        .expect("server responds before timeout for malformed request")
        .expect("response line reads");

    let response = ResponseEnvelope::from_json_line(&line).expect("response is a valid envelope");
    match &response.result {
        ResponseResult::Error(err) => {
            assert_eq!(err.code, "invalid_request", "error code must be invalid_request");
            assert!(!err.retryable, "parse errors are not retryable");
        }
        other => panic!("expected Error result for malformed JSON, got {other:?}"),
    }

    // Connection must still be usable after receiving the error.
    let request = RequestEnvelope::new("req-after-malformed", RequestPayload::Status);
    stream
        .get_mut()
        .write_all(request.to_json_line().expect("request serializes").as_bytes())
        .await
        .expect("follow-up request writes");

    let mut line = String::new();
    timeout(Duration::from_secs(1), stream.read_line(&mut line))
        .await
        .expect("server responds to follow-up after malformed request")
        .expect("follow-up response reads");

    let response = ResponseEnvelope::from_json_line(&line).expect("follow-up response decodes");
    assert_eq!(response.id, "req-after-malformed", "connection must stay alive after invalid_request error");

    server.abort();
    let _ = std::fs::remove_file(socket_path);
}

/// Malformed JSON that contains a valid `id` field must echo that id in the error envelope.
#[tokio::test]
async fn server_smoke_echoes_id_from_malformed_json_when_present() {
    let socket_path = unique_socket_path("malformed-id");
    let server = tokio::spawn(serve(socket_path.clone()));
    let stream = connect_with_retry(&socket_path).await;
    let mut stream = BufReader::new(stream);

    // Valid JSON object with an id field but an unrecognized request payload.
    stream
        .get_mut()
        .write_all(b"{\"id\":\"my-req-42\",\"request\":{\"not_a_real_variant\":{}}}\n")
        .await
        .expect("request with id writes");

    let mut line = String::new();
    timeout(Duration::from_secs(1), stream.read_line(&mut line))
        .await
        .expect("server responds before timeout")
        .expect("response line reads");

    let response = ResponseEnvelope::from_json_line(&line).expect("response decodes");
    assert_eq!(response.id, "my-req-42", "id must be echoed from malformed request when extractable");
    match &response.result {
        ResponseResult::Error(err) => assert_eq!(err.code, "invalid_request"),
        other => panic!("expected Error, got {other:?}"),
    }

    server.abort();
    let _ = std::fs::remove_file(socket_path);
}

/// An oversized frame must produce a structured `frame_too_large` error envelope on the same
/// connection, and the connection must remain usable for subsequent requests.
#[tokio::test]
async fn server_smoke_refuses_oversized_lines_without_killing_server() {
    let socket_path = unique_socket_path("oversized");
    let server = tokio::spawn(serve(socket_path.clone()));

    // Send oversized frame and read the error back on the same connection.
    let stream = connect_with_retry(&socket_path).await;
    let mut stream = BufReader::new(stream);

    let oversized_line = format!("{}\n", "x".repeat(MAX_FRAME_BYTES + 1));
    stream.get_mut().write_all(oversized_line.as_bytes()).await.expect("oversized request writes");

    let mut line = String::new();
    timeout(Duration::from_secs(1), stream.read_line(&mut line))
        .await
        .expect("server responds with error for oversized frame")
        .expect("error response line reads");

    let response = ResponseEnvelope::from_json_line(&line).expect("error response decodes");
    match &response.result {
        ResponseResult::Error(err) => {
            assert_eq!(err.code, "frame_too_large", "error code must be frame_too_large");
            assert!(!err.retryable, "oversized frames are not retryable as-is");
        }
        other => panic!("expected Error result for oversized frame, got {other:?}"),
    }

    // Connection must still be usable after the oversized frame error.
    let request = RequestEnvelope::new("req-after-oversized", RequestPayload::Status);
    stream
        .get_mut()
        .write_all(request.to_json_line().expect("request serializes").as_bytes())
        .await
        .expect("follow-up request writes");

    let mut line = String::new();
    timeout(Duration::from_secs(1), stream.read_line(&mut line))
        .await
        .expect("server responds after oversized frame error")
        .expect("follow-up response reads");

    let response = ResponseEnvelope::from_json_line(&line).expect("follow-up response decodes");
    assert_eq!(response.id, "req-after-oversized", "connection must stay alive after frame_too_large error");

    server.abort();
    let _ = std::fs::remove_file(socket_path);
}

#[cfg(unix)]
fn assert_owner_only_socket(socket_path: &PathBuf) {
    let mode = std::fs::metadata(socket_path).expect("socket metadata").permissions().mode();
    assert_eq!(mode & 0o077, 0, "daemon socket must not be group/world accessible");
}

#[cfg(not(unix))]
fn assert_owner_only_socket(_socket_path: &PathBuf) {}

async fn connect_with_retry(socket_path: &PathBuf) -> UnixStream {
    for _ in 0..100 {
        match UnixStream::connect(socket_path).await {
            Ok(stream) => return stream,
            Err(_) => sleep(Duration::from_millis(10)).await,
        }
    }

    panic!("server did not bind socket at {}", socket_path.display());
}

fn unique_socket_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after epoch").as_nanos();
    let dir = PathBuf::from(format!("/tmp/memd-server-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create short socket directory");
    dir.join(format!("{test_name}-{nonce}.sock"))
}
