# Stream B — Daemon + MCP Contract v0.1

**Status:** shipped, with post-shipping stdio MCP remediation completed 2026-05-02.  
**Scope:** `crates/memoryd/` daemon socket protocol, agent-facing MCP manifest/forwarder, and the launchable stdio MCP server.

## 1. Contract summary

Stream B provides one local daemon and one MCP-facing bridge:

1. `memoryd serve --socket <socket_path> --repo <repo> --runtime <runtime>` runs the substrate-backed daemon.
2. `memoryd mcp --socket <socket_path>` runs a stdio MCP server process for a harness. It does not own substrate state; it forwards tool calls to the daemon over the socket.
3. All agent-facing write/read operations go through the daemon handlers. Harnesses never write substrate files directly.

## 2. Daemon socket protocol

The daemon listens on a Unix domain socket. The default path is `/tmp/memoryd.sock`.

Frames are newline-delimited JSON. Each request frame is a serialized `RequestEnvelope`:

```json
{"id":"req-1","request":"status"}
```

Each response frame is a serialized `ResponseEnvelope` with the same id and a
`result` tagged as either `success` or `error`. For example, a successful status
response starts with `{"id":"req-1","result":{"success":{"status": ...}}}`.

Normative constraints:

- One request or response per line.
- Maximum frame size is `MAX_FRAME_BYTES` (64 KiB).
- The daemon may keep a connection open for multiple frames.
- Socket permissions are owner-only after bind on Unix platforms.
- Malformed frames return protocol errors when an id can be recovered best-effort.

## 3. MCP stdio server

Launch:

```bash
memoryd mcp --socket /tmp/memoryd.sock
```

Harness config examples:

```json
{
  "mcpServers": {
    "memorum": {
      "command": "memoryd",
      "args": ["mcp", "--socket", "/tmp/memoryd.sock"]
    }
  }
}
```

```toml
[mcp.memorum]
command = "memoryd"
args = ["mcp", "--socket", "/tmp/memoryd.sock"]
```

The stdio server speaks newline-delimited JSON-RPC 2.0 over stdin/stdout. Stdout is reserved for protocol frames; diagnostics go to stderr only. EOF on stdin exits cleanly.

Required methods:

- `initialize` returns protocol version, server info, and `tools` capability.
- `initialized` and `notifications/initialized` are accepted as notifications and produce no response.
- `tools/list` returns the current `mcp::manifest()` descriptors, serialized in MCP field casing (`inputSchema`, `outputSchema`).
- `tools/call` parses `{ "name": <tool_name>, "arguments": { ... } }`, converts arguments through `mcp::request_from_args`, forwards through `mcp::forward_to_daemon`, and wraps the daemon response as an MCP tool result.

Tool-call success shape:

```json
{
  "content": [{ "type": "text", "text": "{\"id\":\"mem_...\"}" }],
  "structuredContent": { "id": "mem_...", "summary": "..." },
  "isError": false
}
```

Daemon-level tool errors are returned as MCP tool results with `isError: true`; JSON-RPC parse, method, and parameter errors use JSON-RPC `error`.

## 4. Agent-facing MCP tools

The MCP surface is exactly the nine tools produced by `memoryd::mcp::manifest()`:

1. `memory_search`
2. `memory_get`
3. `memory_write`
4. `memory_supersede`
5. `memory_forget`
6. `memory_reveal`
7. `memory_startup`
8. `memory_note`
9. `memory_observe`

Admin, UI, dashboard, Reality Check, peer-admin, privacy-admin, device-admin, and test-injection surfaces are not MCP tools. If a daemon payload from those surfaces reaches the MCP forwarder, it returns `method_not_allowed_on_mcp` before socket I/O when possible.

## 5. Test obligations

Required coverage lives in `crates/memoryd/tests/`:

- manifest tests assert the exact tool list and schemas;
- forwarder tests assert tool-to-daemon payload conversion;
- stdio tests spawn the real `memoryd mcp` binary, perform `initialize` + `tools/list`, and route `tools/call` through a live daemon.
