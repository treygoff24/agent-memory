# MCP wiring

Memorum exposes MCP through a stdio bridge that forwards calls to a running `memoryd` daemon over its Unix socket.

Define the private runtime and socket once per shell:

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
```

Start the daemon first:

```bash
memoryd serve --init --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --socket "$MEMORUM_SOCKET"
```

All snippets assume the F-001 stdio bridge invocation:

```bash
memoryd mcp --socket "$MEMORUM_SOCKET"
```

For normal dogfooding, the stdio bridge hides `memory_reveal` by default.
`memory_reveal` returns decrypted encrypted content; expose it only for an
explicit user-authorized reveal session:

```bash
memoryd mcp --socket "$MEMORUM_SOCKET" --allow-reveal
```

Use an absolute socket path in JSON/TOML MCP configs. Most MCP clients do not expand `~` inside JSON/TOML. Replace `/absolute/path/to/memorum` below with your real path, or paste the installer-printed snippet from `scripts/install-memorum.sh`.

## Claude Desktop

Add to the Claude Desktop MCP config JSON:

```json
{
  "mcpServers": {
    "memorum": {
      "command": "memoryd",
      "args": ["mcp", "--socket", "/absolute/path/to/memorum/.memoryd/memoryd.sock"]
    }
  }
}
```

Restart Claude Desktop and verify the `memorum` server lists tools.

## Claude Code

Use the same server shape in the Claude Code MCP config surface for the project or user profile:

```json
{
  "mcpServers": {
    "memorum": {
      "command": "memoryd",
      "args": ["mcp", "--socket", "/absolute/path/to/memorum/.memoryd/memoryd.sock"]
    }
  }
}
```

## Codex CLI

Use the Codex MCP TOML configuration shape exercised by the eval harness:

```toml
[mcp_servers.memorum]
command = "memoryd"
args = ["mcp", "--socket", "/absolute/path/to/memorum/.memoryd/memoryd.sock"]
```

## Verification

1. Start `memoryd serve` and leave it running.
2. Start the MCP client with the config above.
3. Confirm the client sees `memory_search`, `memory_write`, and `memory_capture_source`.
   It should not list `memory_reveal` unless you deliberately added `--allow-reveal`.
4. Call `memory_write` with a harmless grounded fact.
5. Call `memory_search` for a distinctive phrase from the write.
6. Run `memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"` if the client cannot connect.

The MCP process writes protocol frames to stdout only; diagnostics and logs must go to stderr so clients can parse JSON-RPC safely.

## `memory_capture_source` alpha schema

Static HTTP(S) capture:

```json
{
  "source": "https://example.com/report",
  "mode": "http_static",
  "excerpts": ["exact quote present in extracted page text"],
  "note": "optional safe operator note"
}
```

Local text/HTML artifact capture:

```json
{
  "source": "file:///absolute/path/to/exported-report.html",
  "mode": "local_artifact",
  "local_path": "/absolute/path/to/exported-report.html",
  "excerpts": ["exact quote present in the exported artifact"]
}
```

Browser-rendered capture is unsupported in alpha, as are authenticated browser
sessions, screenshots/OCR, client-supplied `key_path`, raw key material, and
privacy bypass flags. Export text/HTML locally and use `local_artifact` when a
browser was needed to access or render the page.
