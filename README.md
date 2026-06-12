# Memorum (`agent-memory`)

Memorum is one shared memory layer for every coding assistant you use. If you bounce between Claude Code and Codex CLI — or want to — you've noticed each tool keeps its own private notes about your projects, your preferences, and what worked last time. Switch tools and the new one starts cold. Memorum is the missing piece: a local-first daemon that holds the source of truth, and an MCP server every harness can read from and write to.

You install it once. Your existing notes from Claude Code's `~/.claude/projects/.../memory/` and Codex CLI's `~/.codex/memories/MEMORY.md` get backfilled on first run so nothing's lost. Every new memory either harness writes lands in one place. Recall hits the same store from every session.

What's different from `CLAUDE.md` and `AGENTS.md`: those files are instructions _you write_ to direct the agent. Memorum is the memory _the agent accumulates_ across sessions — observations, decisions, fixes that worked, contradictions it caught. They complement each other. Memorum doesn't touch your `CLAUDE.md` or `AGENTS.md`.

The repo lives on your disk under `$MEMORUM_REPO` (default `~/memorum`). It's plain Markdown + YAML frontmatter, version-controlled with git. No cloud component, no telemetry, no shared multi-tenant store.

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
`memoryd`, `memoryd-tui`, `memoryd-web`, and `memory-merge-driver`.
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

The fastest path is the interactive wizard — it detects prior Claude Code /
Codex memory, imports it with your consent, arranges the daemon, and wires the
`memorum` MCP server into your agents, then prints next steps:

```bash
memoryd init
```

Declining every prompt is a guaranteed no-op, so it is always safe to run and
look. (In scripts/CI, use `memoryd init --non-interactive --json` — a bare
`init` without a terminal refuses rather than guessing. AI agents should follow
`docs/agent-onboarding.md`.)

The manual equivalent of what the wizard does:

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

For Claude Code, use user-scope wiring:

```bash
claude mcp add --scope user memorum -- memoryd mcp --socket "/absolute/path/to/memorum/.memoryd/memoryd.sock"
```

Or add this at the top-level `mcpServers` key of the user config (`$CLAUDE_CONFIG_DIR/.claude.json` or `~/.claude/.claude.json`):

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
- Importing prior memories: `docs/importer.md`
- Troubleshooting: `docs/troubleshooting.md`
- MCP wiring: `docs/mcp-wiring.md`
- API docs: `docs/api/`
- Stream specs: `docs/specs/`
- Review-fix policy: `docs/review-fix-policy.md`
- Bench promotion flow: `bench/README.md`
- Agent-oriented project context: `CLAUDE.md`
- Agent onboarding guide (AI installs Memorum for a user): `docs/agent-onboarding.md`

- Web source grounding: see `docs/api/web-source-grounding-api.md` and `docs/runbooks/web-source-grounding.md` for `memory_capture_source`, `memoryd source capture`, and `webcap:` refs.
