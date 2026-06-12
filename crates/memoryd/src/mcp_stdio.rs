use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

use crate::mcp::{self, ToolDescriptor, ToolName};
use crate::protocol::{ProtocolError, ResponsePayload, ResponseResult};
use crate::socket::{probe_live_socket, SocketProbe};

const PROTOCOL_VERSION: &str = "2025-11-25";
const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Deserialize)]
struct JsonRpcMessage {
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(flatten)]
    outcome: JsonRpcOutcome,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum JsonRpcOutcome {
    Result(Value),
    Error(JsonRpcError),
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpToolDescriptor<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a Value,
    output_schema: &'a Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StdioOptions {
    pub allow_reveal: bool,
}

/// Run the newline-delimited JSON-RPC MCP stdio server.
///
/// The MCP stdio transport reserves stdout for protocol frames. Diagnostics
/// from this loop therefore go to stderr; normal EOF is not logged.
pub async fn serve_stdio_with_options(socket_path: &Path, options: StdioOptions) -> Result<()> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).await.context("read MCP stdio frame")?;
        if read == 0 {
            // EOF on stdin: client closed the pipe. Exit cleanly, matching the
            // blocking `lines()` loop which terminated on the same condition.
            break;
        }
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcMessage>(&line) {
            Ok(message) => handle_message(socket_path, options, message).await,
            Err(error) => {
                Some(error_response(None, -32700, "parse error", Some(json!({ "message": error.to_string() }))))
            }
        };

        if let Some(response) = response {
            let mut frame = serde_json::to_string(&response).context("serialize MCP stdio response")?;
            frame.push('\n');
            stdout.write_all(frame.as_bytes()).await.context("write MCP stdio response")?;
            stdout.flush().await.context("flush MCP stdio response")?;
        }
    }

    Ok(())
}

async fn handle_message(socket_path: &Path, options: StdioOptions, message: JsonRpcMessage) -> Option<JsonRpcResponse> {
    let id = message.id.clone();
    let is_notification = id.is_none();
    if message.jsonrpc.as_deref() != Some(JSONRPC_VERSION) {
        return notification_or_error(
            is_notification,
            error_response(id, -32600, "invalid request: jsonrpc must be \"2.0\"", None),
        );
    }

    match message.method.as_str() {
        "initialize" => notification_or_success(is_notification, id, initialize_result()),
        "initialized" | "notifications/initialized" => None,
        "tools/list" => notification_or_success(is_notification, id, tools_list_result(options)),
        "tools/call" if is_notification => None,
        "tools/call" => Some(handle_tools_call(socket_path, options, id, message.params).await),
        method => notification_or_error(
            is_notification,
            error_response(id, -32601, format!("method not found: {method}"), None),
        ),
    }
}

fn notification_or_success(is_notification: bool, id: Option<Value>, result: Value) -> Option<JsonRpcResponse> {
    (!is_notification).then(|| success_response(id, result))
}

fn notification_or_error(is_notification: bool, response: JsonRpcResponse) -> Option<JsonRpcResponse> {
    (!is_notification).then_some(response)
}

async fn handle_tools_call(
    socket_path: &Path,
    options: StdioOptions,
    id: Option<Value>,
    params: Value,
) -> JsonRpcResponse {
    let call = match serde_json::from_value::<ToolCallParams>(params) {
        Ok(call) => call,
        Err(error) => {
            return error_response(
                id,
                -32602,
                "invalid tools/call params",
                Some(json!({ "message": error.to_string() })),
            )
        }
    };

    let tool_name = match ToolName::try_from(call.name.as_str()) {
        Ok(tool_name) => tool_name,
        Err(error) => return error_response(id, -32602, error.to_string(), None),
    };
    if tool_name == ToolName::Reveal && !options.allow_reveal {
        return error_response(
            id,
            -32602,
            "memory_reveal is disabled for MCP stdio unless memoryd mcp --allow-reveal is set",
            Some(json!({ "code": "reveal_disabled_on_mcp" })),
        );
    }
    let request = match mcp::request_from_args(tool_name, call.arguments) {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                id,
                -32602,
                format!("invalid arguments for {tool_name}"),
                Some(json!({ "message": error.to_string() })),
            )
        }
    };

    if !matches!(probe_live_socket(socket_path), SocketProbe::Live) {
        return daemon_not_running_response(id, socket_path);
    }

    let daemon_id = id.as_ref().map(jsonrpc_id_to_daemon_id).unwrap_or_else(|| format!("mcp-{}", tool_name.as_str()));
    match mcp::forward_to_daemon(socket_path, daemon_id, request).await {
        Ok(envelope) => success_response(id, call_result(envelope.result)),
        Err(error) => {
            error_response(id, -32000, "daemon request failed", Some(json!({ "message": format!("{error:#}") })))
        }
    }
}

fn daemon_not_running_response(id: Option<Value>, socket_path: &Path) -> JsonRpcResponse {
    error_response(
        id,
        -32001,
        "daemon_not_running",
        Some(json!({
            "code": "daemon_not_running",
            "socket": socket_path.display().to_string(),
            "guidance": "Start memoryd or run memoryd doctor to inspect daemon health."
        })),
    )
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "memoryd",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools_list_result(options: StdioOptions) -> Value {
    let manifest = mcp::stdio_manifest(options.allow_reveal);
    let tools: Vec<_> = manifest.tools.iter().map(mcp_tool_descriptor).collect();
    json!({ "tools": tools })
}

fn mcp_tool_descriptor(tool: &ToolDescriptor) -> McpToolDescriptor<'_> {
    McpToolDescriptor {
        name: &tool.name,
        description: &tool.description,
        input_schema: &tool.input_schema,
        output_schema: &tool.output_schema,
    }
}

fn call_result(result: ResponseResult) -> Value {
    match result {
        ResponseResult::Success(payload) => {
            let structured_content = response_payload_value(payload);
            json!({
                "content": [{ "type": "text", "text": structured_content.to_string() }],
                "structuredContent": structured_content,
                "isError": false
            })
        }
        ResponseResult::Error(error) => tool_error_result(error),
    }
}

fn response_payload_value(payload: ResponsePayload) -> Value {
    let value = serde_json::to_value(payload).expect("ResponsePayload serializes");
    let Some(object) = value.as_object() else {
        return value;
    };
    if object.len() == 1 {
        object.values().next().expect("single value exists").clone()
    } else {
        value
    }
}

fn tool_error_result(error: ProtocolError) -> Value {
    let structured_content = json!({
        "code": error.code,
        "message": error.message,
        "retryable": error.retryable
    });
    json!({
        "content": [{ "type": "text", "text": structured_content.to_string() }],
        "structuredContent": structured_content,
        "isError": true
    })
}

fn success_response(id: Option<Value>, result: Value) -> JsonRpcResponse {
    JsonRpcResponse { jsonrpc: JSONRPC_VERSION, id: id.unwrap_or(Value::Null), outcome: JsonRpcOutcome::Result(result) }
}

fn error_response(id: Option<Value>, code: i64, message: impl Into<String>, data: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: JSONRPC_VERSION,
        id: id.unwrap_or(Value::Null),
        outcome: JsonRpcOutcome::Error(JsonRpcError { code, message: message.into(), data }),
    }
}

fn jsonrpc_id_to_daemon_id(id: &Value) -> String {
    match id {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}
