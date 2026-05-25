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

For dogfooding, prefer the installer because it starts the daemon with the same
runtime/socket layout used by the docs:

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
bash scripts/install-memorum.sh --force-reinstall --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
```

The installer installs the operator-facing dogfood binaries:
`memoryd`, `memoryd-tui`, `memoryd-web`, and `memoryd-merge-driver`.
`memorum-eval` is a development/eval binary; install it separately only when
you are running evals.

Manual equivalent:

```bash
cargo install --path crates/memoryd --locked
cargo install --path crates/memoryd-tui --locked
cargo install --path crates/memoryd-web --locked
cargo install --path crates/memory-merge-driver --locked

# Optional eval harness for development/release validation:
cargo install --path crates/memorum-eval --locked
```

For development without installing, use `cargo run --bin memoryd -- ...`.

## Quickstart

Define the private runtime and socket once per shell:

```bash
export MEMORUM_REPO="$HOME/memorum"
export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"
```

```bash
mkdir -p "$MEMORUM_REPO"
memoryd serve --init --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --socket "$MEMORUM_SOCKET"
```

In another shell (reuse the same exports, or source them from your shell profile):

```bash
memoryd status --socket "$MEMORUM_SOCKET"
memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
memoryd mcp --socket "$MEMORUM_SOCKET"
```

Wire an MCP client to launch the stdio bridge. Use the absolute socket path printed by `scripts/install-memorum.sh`; most MCP clients do not expand `~` inside JSON/TOML.

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

Replace `/absolute/path/to/memorum` with your real path, or paste the installer snippet that already contains the canonicalized socket.

Then ask the client to call `memory_write` with a grounded fact and `memory_search` for the same text. See `docs/getting-started.md` for a step-by-step path and `docs/mcp-wiring.md` for per-harness config snippets.

## Alpha limits that are explicit

- Source grounding supports deterministic static HTTP(S) capture and local
  text/HTML artifact capture. Browser-rendered capture is unsupported, as are
  screenshots/OCR, authenticated browser/cookie capture, and client-supplied
  key paths or privacy bypass flags.
- The model privacy filter remains unsupported in alpha. Memorum uses
  deterministic privacy checks and fails closed rather than promising semantic
  model classification.
- Dashboard ROI is not full business ROI. It is an alpha operational metrics
  surface over promotion, refusal, dream, and Reality Check adherence signals.
- Device pairing is unsupported unless a daemon route is present; visible pair
  controls should be disabled with explanatory copy instead of silently doing
  nothing.

## Useful local commands

```bash
memoryd recall startup-block --socket "$MEMORUM_SOCKET" --cwd "$PWD" --session-id smoke --harness codex
memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137
memoryd ui --socket "$MEMORUM_SOCKET"
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

- Web source grounding: see `docs/api/web-source-grounding-api.md` and `docs/runbooks/web-source-grounding.md` for `memory_capture_source`, `memoryd source capture`, and `webcap:` refs.
