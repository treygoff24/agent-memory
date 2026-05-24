use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::mcp::manifest;
use serde_json::{json, Value};

mod common;
use common::{shutdown, spawn_daemon, unique_socket_path, wait_for_socket};

#[test]
fn mcp_stdio_initialize_and_tools_list_round_trip_through_subprocess() {
    let socket = unique_socket_path("mcpstdio", "list");
    let mut server = McpServerProcess::spawn(&socket);

    let initialize = server.request(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "memoryd-test", "version": "0.0.0" }
        }
    }));

    assert_eq!(initialize["jsonrpc"], "2.0");
    assert_eq!(initialize["id"], 1);
    assert_eq!(initialize["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(initialize["result"]["serverInfo"]["name"], "memoryd");
    assert_eq!(initialize["result"]["capabilities"]["tools"], json!({}));

    server.notify(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    }));

    let tools_list = server.request(json!({
        "jsonrpc": "2.0",
        "id": "tools-list",
        "method": "tools/list",
        "params": {}
    }));

    let actual_tools = tools_list["result"]["tools"].as_array().expect("tools/list returns an array");
    let expected_manifest = manifest();
    assert_eq!(actual_tools.len(), expected_manifest.tools.len());
    for (actual, expected) in actual_tools.iter().zip(expected_manifest.tools.iter()) {
        assert_eq!(actual["name"], expected.name);
        assert_eq!(actual["description"], expected.description);
        assert_eq!(actual["inputSchema"], expected.input_schema);
        assert_eq!(actual["outputSchema"], expected.output_schema);
    }
}

#[test]
fn mcp_stdio_unknown_notifications_do_not_emit_responses() {
    let socket = unique_socket_path("mcpstdio", "notification");
    let mut server = McpServerProcess::spawn(&socket);

    server.notify(json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": "req-old" }
    }));

    assert!(
        server.recv_timeout(Duration::from_millis(250)).is_none(),
        "notifications must not receive JSON-RPC responses"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_stdio_tools_call_routes_through_daemon_forwarder() {
    let temp = tempfile::tempdir().expect("tempdir");
    let socket = unique_socket_path("mcpstdio", "call");
    let substrate = init_substrate(&temp).await;
    let (shutdown_tx, daemon) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    let mut server = McpServerProcess::spawn(&socket);
    let _ = server.request(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "memoryd-test", "version": "0.0.0" }
        }
    }));
    server.notify(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let note = server.request(json!({
        "jsonrpc": "2.0",
        "id": "call-note",
        "method": "tools/call",
        "params": {
            "name": "memory_note",
            "arguments": { "text": "MCP stdio fixture note about protocol handshakes" }
        }
    }));

    assert_eq!(note["jsonrpc"], "2.0");
    assert_eq!(note["id"], "call-note");
    assert_eq!(note["result"]["isError"], Value::Bool(false));
    let note_content = note["result"]["structuredContent"].as_object().expect("structured note object");
    let note_id = note_content["id"].as_str().expect("note id").to_owned();
    assert!(note_content["summary"].as_str().expect("summary").contains("MCP stdio fixture"));

    let search = server.request(json!({
        "jsonrpc": "2.0",
        "id": "call-search",
        "method": "tools/call",
        "params": {
            "name": "memory_search",
            "arguments": {
                "query": "protocol handshakes",
                "limit": 5,
                "include_body": false
            }
        }
    }));

    assert_eq!(search["result"]["isError"], Value::Bool(false));
    let hits = search["result"]["structuredContent"]["hits"].as_array().expect("search hits array");
    assert!(hits.iter().any(|hit| hit["id"] == note_id), "search should find the note written via stdio MCP");

    drop(server);
    shutdown(shutdown_tx, daemon, &socket).await;
}

struct McpServerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: mpsc::Receiver<String>,
}

impl McpServerProcess {
    fn spawn(socket: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(["mcp", "--auto-start", "false", "--socket"])
            .arg(socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn memoryd mcp");

        let stdout = child.stdout.take().expect("stdout is piped");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let _ = tx.send(line);
                    }
                    Err(_) => break,
                }
            }
        });

        Self { stdin: child.stdin.take().expect("stdin is piped"), child, stdout: rx }
    }

    fn request(&mut self, request: Value) -> Value {
        self.notify(request);
        let line = self.stdout.recv_timeout(Duration::from_secs(5)).expect("MCP server responds before timeout");
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("MCP response is JSON ({error}): {line}"))
    }

    fn notify(&mut self, notification: Value) {
        writeln!(self.stdin, "{}", serde_json::to_string(&notification).expect("request serializes"))
            .expect("write MCP request");
        self.stdin.flush().expect("flush MCP request");
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Option<String> {
        self.stdout.recv_timeout(timeout).ok()
    }
}

impl Drop for McpServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some("dev_mcpstdio".to_string()) })
        .await
        .expect("substrate init")
}
