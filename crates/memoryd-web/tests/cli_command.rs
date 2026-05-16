use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

#[test]
fn memoryd_web_command_binds_and_uses_daemon_socket_args() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = temp.path().join("missing-memoryd.sock");
    let port = free_port();
    let mut child = Command::new(memoryd_web_bin())
        .arg("--socket")
        .arg(&socket)
        .arg("--repo")
        .arg(temp.path())
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn memoryd-web");

    let response = wait_for_status_response(&mut child, port);
    stop_child(child);

    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
    let body = json_body(&response);
    assert_eq!(body["error"], "daemon_request_failed");
    assert_eq!(body["code"], "daemon_unavailable");
}

#[test]
fn memoryd_web_command_rejects_missing_socket_value() {
    let output = Command::new(memoryd_web_bin()).arg("--socket").output().expect("run memoryd-web");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--socket requires a value"), "{stderr}");
}

#[test]
fn memoryd_web_command_rejects_privileged_port() {
    let output = Command::new(memoryd_web_bin()).arg("--port").arg("80").output().expect("run memoryd-web");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--port must be in 1024..=65535"), "{stderr}");
}

fn memoryd_web_bin() -> &'static str {
    env!("CARGO_BIN_EXE_memoryd-web")
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port").local_addr().expect("local addr").port()
}

fn wait_for_status_response(child: &mut Child, port: u16) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_error = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("poll child") {
            panic!("memoryd-web exited before binding: {status}");
        }
        match request_status(port) {
            Ok(response) => return response,
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("memoryd-web did not bind on port {port}: {last_error:?}");
}

fn request_status(port: u16) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(b"GET /api/status HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn json_body(response: &str) -> Value {
    let body = response.split("\r\n\r\n").nth(1).expect("HTTP body");
    serde_json::from_str(body).expect("json body")
}

fn stop_child(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}
