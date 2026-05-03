# Getting started with Memorum

This guide starts a local memory daemon, verifies it, and wires an MCP client to the stdio bridge.

## 1. Build or install

From the repo root:

```bash
cargo install --path crates/memoryd
cargo install --path crates/memoryd-tui
cargo install --path crates/memoryd-web
```

For checkout-only development, prefix commands with `cargo run -p memoryd --` instead of installing.

## 2. Initialize and start the daemon

```bash
mkdir -p ~/memorum
memoryd serve --init --repo ~/memorum --runtime ~/memorum/.memoryd --socket /tmp/memoryd.sock
```

Keep this process running. The socket path is what CLIs, the web dashboard, TUI, and MCP bridge use.

## 3. Verify daemon health

In another shell:

```bash
memoryd status --socket /tmp/memoryd.sock
memoryd doctor --repo ~/memorum --runtime ~/memorum/.memoryd
```

Expected result: `status` returns a ready daemon response, and `doctor` reports either healthy or actionable findings. If doctor reports `events_log_mirror_lag`, run the reindex repair it prints.

## 4. Wire MCP

Add this to your MCP-capable client config:

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

Restart the client. It should list Memorum tools such as `memory_search`, `memory_get`, `memory_write`, `memory_note`, and `memory_startup`.

## 5. First write/search round-trip

From the MCP client, call `memory_write` with a grounded project fact. Then call `memory_search` for a distinctive phrase from that fact. A successful round-trip returns the new memory id in the search results.

CLI-only smoke path:

```bash
memoryd write-note --socket /tmp/memoryd.sock "Memorum local smoke note from getting-started."
memoryd search --socket /tmp/memoryd.sock "local smoke note"
```

## 6. Optional observability

```bash
memoryd web enable --socket /tmp/memoryd.sock --port 7137
open http://localhost:7137
memoryd ui --socket /tmp/memoryd.sock
```

The web dashboard exposes status, Reality Check, review, audit, and `/api/recall-hits` for recent recall-hit events.
