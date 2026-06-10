use std::path::Path;
use std::process::Command;

use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::client;
use memoryd::protocol::{RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::task::JoinHandle;

mod common;
use common::{shutdown, spawn_daemon, wait_for_socket};

#[tokio::test]
async fn recall_cli_startup_and_delta_print_only_xml_and_update_daemon_counters() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let socket = runtime.join("memoryd.sock");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_recallcli".to_owned()) },
    )
    .await
    .expect("substrate init");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    let startup = run_memoryd_async([
        "recall",
        "startup-block",
        "--repo",
        repo.to_string_lossy().as_ref(),
        "--runtime",
        runtime.to_string_lossy().as_ref(),
        "--cwd",
        repo.to_string_lossy().as_ref(),
        "--session-id",
        "sess_cli",
        "--harness",
        "codex",
        "--budget-tokens",
        "512",
    ])
    .await;
    assert!(startup.status.success(), "startup stderr: {}", String::from_utf8_lossy(&startup.stderr));
    let startup_stdout = String::from_utf8(startup.stdout).expect("startup stdout utf8");
    assert!(startup_stdout.starts_with("<memory-recall version=\"stream-e-v0.6\""));
    assert!(String::from_utf8_lossy(&startup.stderr).is_empty(), "success diagnostics stay off stderr");

    let delta = run_memoryd_async([
        "recall",
        "delta-block",
        "--repo",
        repo.to_string_lossy().as_ref(),
        "--runtime",
        runtime.to_string_lossy().as_ref(),
        "--cwd",
        repo.to_string_lossy().as_ref(),
        "--session-id",
        "sess_cli",
        "--harness",
        "codex",
        "--message",
        "definitely-no-match",
        "--budget-tokens",
        "512",
    ])
    .await;
    assert!(delta.status.success(), "delta stderr: {}", String::from_utf8_lossy(&delta.stderr));
    assert_eq!(String::from_utf8(delta.stdout).expect("delta stdout utf8"), "<memory-delta empty=\"true\" />\n");
    assert!(String::from_utf8_lossy(&delta.stderr).is_empty(), "success diagnostics stay off stderr");

    let status = client::request(&socket, "status-after-cli", RequestPayload::Status).await.expect("status request");
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.startup_invoked_total, 1);
            assert_eq!(status.recall.delta_invoked_total, 1);
        }
        other => panic!("expected status success, got {other:?}"),
    }

    shutdown(shutdown_tx, server, &socket).await;
}

#[test]
fn recall_cli_without_daemon_fails_fast_with_recall_unavailable_exit_2_and_no_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let output = run_memoryd([
        "recall",
        "delta-block",
        "--repo",
        temp.path().to_string_lossy().as_ref(),
        "--runtime",
        temp.path().join("runtime").to_string_lossy().as_ref(),
        "--cwd",
        temp.path().to_string_lossy().as_ref(),
        "--session-id",
        "sess_cli",
        "--harness",
        "codex",
        "--message",
        "definitely-no-match",
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "failure stdout must be empty");
    assert!(String::from_utf8_lossy(&output.stderr).contains("recall_unavailable"));
}

#[tokio::test]
async fn recall_cli_maps_daemon_errors_to_exit_codes_1_3_4() {
    let cases = [("invalid_request", 1), ("privacy_error", 3), ("not_implemented", 4)];

    for (code, expected_exit) in cases {
        let temp = tempfile::tempdir().expect("tempdir");
        let socket = temp.path().join(format!("{code}.sock"));
        let server = spawn_single_error_daemon(&socket, code).await;

        let output = run_memoryd_async([
            "recall",
            "startup-block",
            "--repo",
            temp.path().to_string_lossy().as_ref(),
            "--runtime",
            temp.path().join("runtime").to_string_lossy().as_ref(),
            "--socket",
            socket.to_string_lossy().as_ref(),
            "--cwd",
            temp.path().to_string_lossy().as_ref(),
            "--session-id",
            "sess_cli",
            "--harness",
            "codex",
        ])
        .await;

        assert_eq!(output.status.code(), Some(expected_exit), "{code} must map to exit {expected_exit}");
        assert!(output.stdout.is_empty(), "{code} failure stdout must be empty");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{code} failure stderr must include daemon error code"
        );
        server.await.expect("fake daemon joins").expect("fake daemon returns ok");
    }
}

fn run_memoryd<const N: usize>(args: [&str; N]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_memoryd")).args(args).output().expect("memoryd command runs")
}

async fn run_memoryd_async<const N: usize>(args: [&str; N]) -> std::process::Output {
    let args = args.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_memoryd")).args(args).output().expect("memoryd command runs")
    })
    .await
    .expect("memoryd blocking task joins")
}

async fn spawn_single_error_daemon(socket: &Path, code: &str) -> JoinHandle<anyhow::Result<()>> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("bind fake daemon socket");
    let code = code.to_owned();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let request = memoryd::protocol::RequestEnvelope::from_json_line(&line)?;
        let response = ResponseEnvelope::error(request.id, code.clone(), format!("{code} fixture"), false);
        reader.get_mut().write_all(response.to_json_line()?.as_bytes()).await?;
        Ok(())
    })
}
