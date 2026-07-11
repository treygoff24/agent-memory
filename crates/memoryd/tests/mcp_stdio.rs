use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::mcp::{manifest, stdio_manifest};
use serde_json::{json, Value};
use serial_test::serial;

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
    let expected_manifest = stdio_manifest(false);
    assert_eq!(actual_tools.len(), expected_manifest.tools.len());
    assert!(
        actual_tools.iter().all(|tool| tool["name"] != "memory_reveal"),
        "default stdio bridge must hide memory_reveal"
    );
    for (actual, expected) in actual_tools.iter().zip(expected_manifest.tools.iter()) {
        assert_eq!(actual["name"], expected.name);
        assert_eq!(actual["description"], expected.description);
        assert_eq!(actual["inputSchema"], expected.input_schema);
        assert_eq!(actual["outputSchema"], expected.output_schema);
    }
}

#[test]
fn mcp_stdio_allow_reveal_flag_restores_reveal_tool() {
    let socket = unique_socket_path("mcpstdio", "allow-reveal");
    let mut server = McpServerProcess::spawn_with_args(&socket, &["--allow-reveal"]);
    let _ = server.request(json!({
        "jsonrpc": "2.0",
        "id": "init",
        "method": "initialize",
        "params": {}
    }));

    let tools_list = server.request(json!({
        "jsonrpc": "2.0",
        "id": "tools-list",
        "method": "tools/list",
        "params": {}
    }));

    let actual_tools = tools_list["result"]["tools"].as_array().expect("tools/list returns an array");
    assert_eq!(actual_tools.len(), manifest().tools.len());
    assert!(
        actual_tools.iter().any(|tool| tool["name"] == "memory_reveal"),
        "--allow-reveal should expose memory_reveal for explicit MCP sessions"
    );
}

#[test]
fn mcp_stdio_rejects_reveal_call_when_reveal_is_not_allowed() {
    let socket = unique_socket_path("mcpstdio", "reveal-disabled");
    let mut server = McpServerProcess::spawn(&socket);
    let response = server.request(json!({
        "jsonrpc": "2.0",
        "id": "reveal",
        "method": "tools/call",
        "params": {
            "name": "memory_reveal",
            "arguments": {
                "id": "mem_20260525_abcdef1234567890_000001",
                "reason": "user explicitly asked to reveal"
            }
        }
    }));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "reveal");
    assert_eq!(response["error"]["code"], -32602);
    assert_eq!(response["error"]["data"]["code"], "reveal_disabled_on_mcp");
}

#[test]
fn mcp_stdio_tools_call_rejects_missing_required_arguments_before_daemon_probe() {
    let socket = unique_socket_path("mcpstdio", "startup-missing-args");
    let mut server = McpServerProcess::spawn(&socket);
    let response = server.request(json!({
        "jsonrpc": "2.0",
        "id": "startup-missing-args",
        "method": "tools/call",
        "params": {
            "name": "memory_startup",
            "arguments": {}
        }
    }));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "startup-missing-args");
    assert!(response.get("result").is_none(), "invalid args use JSON-RPC error convention: {response}");
    assert_eq!(response["error"]["code"], -32602);
    assert_eq!(response["error"]["message"], "invalid arguments for memory_startup");
    let details = response["error"]["data"]["message"].as_str().expect("error data message");
    assert!(details.contains("memory_startup"), "error names the tool: {details}");
    assert!(details.contains("required shape"), "error includes required shape: {details}");
    for field in ["cwd", "session_id", "harness"] {
        assert!(details.contains(field), "error names missing {field}: {details}");
    }
    assert!(!details.contains("daemon_not_running"), "argument validation must run before daemon probing: {details}");
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

// Serialize against this binary's other tests and keep this test off the shared
// scheduler when run under nextest (see `.config/nextest.toml`, which reserves
// the full thread budget for it). This is the only test in the file that runs an
// in-process multi-thread tokio daemon AND a subprocess MCP bridge talking back
// over a Unix socket; under full-suite parallel load — heavy now that the
// embedding stack (candle/fastembed/Metal) makes each test binary ~100 MB — the
// round-trip occasionally missed its completion deadline. The forwarding itself
// is correct; the deadline is a "did it finish" bound, not a latency SLO.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn mcp_stdio_tools_call_routes_through_daemon_forwarder() {
    // Disable the production embedding worker for this in-process daemon: loading
    // the ~1.1 GB Qwen3 model would compete with socket bind under the
    // multi-thread test runtime. Set synchronously at the top so it is visible
    // before the daemon task spawns. (Also set in `spawn_daemon`, belt-and-braces.)
    std::env::set_var("MEMORUM_DISABLE_EMBEDDING_WORKER", "1");
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

    // A fresh note is a governance candidate, fenced from search until
    // approved (the pre-W0 FTS-degraded lane leaked candidate status). The MCP
    // surface has no approve tool, so approve through the daemon socket.
    let approve = memoryd::client::request(
        &socket,
        "call-approve",
        memoryd::protocol::RequestPayload::ReviewApprove { id: note_id.clone() },
    )
    .await
    .expect("review approve reaches daemon");
    assert!(
        matches!(
            approve.result,
            memoryd::protocol::ResponseResult::Success(memoryd::protocol::ResponsePayload::ReviewApprove(_))
        ),
        "expected ReviewApprove success"
    );

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
    assert!(hits.iter().any(|hit| hit["id"] == note_id), "search should find the approved note written via stdio MCP");

    let invalid_search = server.request(json!({
        "jsonrpc": "2.0",
        "id": "call-invalid-search",
        "method": "tools/call",
        "params": {
            "name": "memory_search",
            "arguments": {
                "query": "",
                "limit": 5
            }
        }
    }));

    assert_eq!(invalid_search["result"]["isError"], Value::Bool(true));
    assert!(
        invalid_search["result"]["content"][0]["text"]
            .as_str()
            .expect("tool error text")
            .contains("search query must not be empty"),
        "handler error should be visible tool content: {invalid_search}"
    );

    let startup = server.request(json!({
        "jsonrpc": "2.0",
        "id": "call-startup",
        "method": "tools/call",
        "params": {
            "name": "memory_startup",
            "arguments": {
                "cwd": temp.path().join("repo"),
                "session_id": "sess_mcp_stdio",
                "harness": "codex",
                "budget_tokens": 3600
            }
        }
    }));

    assert_eq!(startup["jsonrpc"], "2.0");
    assert_eq!(startup["id"], "call-startup");
    assert_eq!(startup["result"]["isError"], Value::Bool(false));
    for field in ["session_binding", "recall_block", "budget_used_tokens", "recall_explanation", "guidance"] {
        assert!(startup["result"]["structuredContent"].get(field).is_some(), "startup response missing {field}");
    }

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
        Self::spawn_with_args(socket, &[])
    }

    fn spawn_with_args(socket: &Path, extra_args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(["mcp"])
            .args(extra_args)
            .arg("--socket")
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
        // Completion bound for the subprocess round-trip, not a latency SLO: a
        // healthy reply lands in milliseconds, so a fast test never waits this
        // long. The generous ceiling is purely headroom for the worst case under
        // full-suite parallel load, where the ~100 MB embedding-linked test
        // binaries starve the scheduler and a 5 s bound occasionally tripped.
        let line = self.stdout.recv_timeout(Duration::from_secs(30)).expect("MCP server responds before timeout");
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
