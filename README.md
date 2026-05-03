# Memorum (`agent-memory`)

Memorum is a local-first, daemon-backed shared memory layer for agent harnesses. It stores canonical Markdown+YAML memories in a user-owned repo, maintains rebuildable SQLite/event indexes, routes writes through governance and privacy checks, and exposes recall/search/write tools over the `memoryd` daemon and MCP.

```text
Claude / Codex / Cursor / any MCP client
              │
              ▼
       memoryd mcp --socket <sock>   (stdio JSON-RPC MCP bridge)
              │
              ▼
        memoryd serve --init         (owner-only Unix socket daemon)
              │
              ├─ governance + privacy + passive recall + dreaming
              ├─ TUI / localhost web observability
              ▼
 canonical memory repo + events JSONL + derived SQLite index
```

## Install from this checkout

```bash
cargo install --path crates/memoryd
cargo install --path crates/memoryd-tui
cargo install --path crates/memoryd-web
cargo install --path crates/memorum-eval
```

For development without installing, use `cargo run -p memoryd -- ...`.

## Quickstart

```bash
mkdir -p ~/memorum
memoryd serve --init --repo ~/memorum --runtime ~/memorum/.memoryd --socket /tmp/memoryd.sock
```

In another shell:

```bash
memoryd status --socket /tmp/memoryd.sock
memoryd doctor --repo ~/memorum --runtime ~/memorum/.memoryd
memoryd mcp --socket /tmp/memoryd.sock
```

Wire an MCP client to launch the stdio bridge:

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

Then ask the client to call `memory_write` with a grounded fact and `memory_search` for the same text. See `docs/getting-started.md` for a step-by-step path and `docs/mcp-wiring.md` for per-harness config snippets.

## Useful local commands

```bash
memoryd recall startup-block --socket /tmp/memoryd.sock --cwd "$PWD" --session-id smoke --harness codex
memoryd web enable --socket /tmp/memoryd.sock --port 7137
memoryd ui --socket /tmp/memoryd.sock
memorum-eval --harness mock --output text
```

## Quality gates

Focused Rust checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

`bash scripts/check.sh` is the canonical full local checkpoint gate; it also runs docs/lint/specgate checks when the local tools are installed.

## Docs map

- System contract: `docs/specs/system-v0.2.md`
- Getting started: `docs/getting-started.md`
- MCP wiring: `docs/mcp-wiring.md`
- API docs: `docs/api/`
- Stream specs: `docs/specs/`
- Review-fix policy: `docs/review-fix-policy.md`
- Bench promotion flow: `bench/README.md`
- Agent-oriented project context: `CLAUDE.md`
