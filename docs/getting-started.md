# Getting started with Memorum

This guide starts a local memory daemon, verifies it, and wires an MCP client to the stdio bridge.

## 1. Build or install

From the repo root:

```bash
bash scripts/install-memorum.sh --force-reinstall --repo "$HOME/memorum" --runtime "$HOME/memorum/.memoryd"
```

The installer builds the dogfood operator binaries (`memoryd`, `memoryd-tui`,
`memoryd-web`, and `memoryd-merge-driver`) and prints MCP client snippets with
an absolute socket path. If you prefer a manual install, use the same binary set:

```bash
cargo install --path crates/memoryd --locked
cargo install --path crates/memoryd-tui --locked
cargo install --path crates/memoryd-web --locked
cargo install --path crates/memory-merge-driver --locked
```

`memorum-eval` is separate development/eval tooling; install it only when you
are running evals or release validation.

For checkout-only development, prefix commands with `cargo run --bin memoryd --` instead of installing.

## 2. Initialize and start the daemon

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

Keep this process running. The socket path is what CLIs, the web dashboard, TUI, and MCP bridge use.

## 3. Verify daemon health

In another shell (with the same `MEMORUM_*` exports):

```bash
memoryd status --socket "$MEMORUM_SOCKET"
memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
```

Expected result: `status` returns a ready daemon response, and `doctor` reports either healthy or actionable findings. If doctor reports `events_log_mirror_lag`, run the reindex repair it prints.

## 4. Wire MCP

Add this to your MCP-capable client config. Replace the placeholder socket path with the output of `echo "$MEMORUM_SOCKET"` (or the absolute path printed by `scripts/install-memorum.sh`). Most MCP clients do not expand `~` inside JSON/TOML.

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

Restart the client. In the default dogfood-safe mode it should list Memorum
tools such as `memory_search`, `memory_get`, `memory_write`,
`memory_supersede`, `memory_forget`, `memory_startup`, `memory_note`,
`memory_observe`, and `memory_capture_source`. `memory_reveal` returns
decrypted encrypted content, so the stdio bridge exposes it only when launched
with `--allow-reveal` for an explicit, user-authorized reveal session.

## 5. First write/search round-trip

From the MCP client, call `memory_write` with a grounded project fact. Then call `memory_search` for a distinctive phrase from that fact. A successful round-trip returns the new memory id in the search results.

CLI-only smoke path:

```bash
memoryd write-note --socket "$MEMORUM_SOCKET" "Memorum local smoke note from getting-started."
memoryd search --socket "$MEMORUM_SOCKET" "local smoke note"
```

## 6. Optional observability

```bash
memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137
open http://localhost:7137
memoryd ui --socket "$MEMORUM_SOCKET"
```

The web dashboard exposes status, Reality Check, review, audit, and `/api/recall-hits` for recent recall-hit events.
Alpha dashboard ROI is operational telemetry over promotion/refusal/dream/Reality
Check signals, not full business ROI. Device pairing is unsupported alpha scope,
and browser-rendered source capture is unsupported alpha scope, unless a later
daemon route explicitly lands them; controls for unsupported work should be
disabled rather than inert.

## 7. Optional source grounding smoke

For a public static HTTP(S) page:

```bash
memoryd source capture \
  --socket "$MEMORUM_SOCKET" \
  --url https://example.com/report \
  --excerpt 'exact quote present in extracted page text'
```

For a local text/HTML export:

```bash
memoryd source capture \
  --socket "$MEMORUM_SOCKET" \
  --file /absolute/path/to/exported-report.html \
  --mode local-artifact \
  --excerpt 'exact quote present in the exported artifact'
```

Source grounding does not support browser-rendered capture, authenticated
browser sessions, screenshots/OCR, client-supplied key paths, or privacy bypass
flags in alpha. The model privacy filter remains unsupported; deterministic
privacy checks still apply and unsafe plaintext fails closed.
