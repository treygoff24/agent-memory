# Getting started with Memorum

This guide is for a human operator setting Memorum up by hand. It uses `memoryd init` to bootstrap, verifies the daemon, and wires an MCP client to the stdio bridge.

Related guides — each says who it is for:

- **You are an AI agent installing Memorum for a user?** Follow [`docs/agent-onboarding.md`](agent-onboarding.md) instead — the scripted detect → consent → run → verify → restart loop built on `memoryd init --non-interactive --json`. ([`llms-install.md`](../llms-install.md) is the short pointer to it.)
- **Installing on a fresh machine from Git, or want the full binary set built in one pass?** See [`docs/install.md`](install.md) for `cargo install` and `scripts/install-memorum.sh`.
- **Something went wrong?** [`docs/troubleshooting.md`](troubleshooting.md) covers `dream_disabled`, socket errors, MCP listing empty, and the other first-run failure modes.

## 0. Bootstrap with `memoryd init`

`memoryd init` is the unified first-run entrypoint. It detects your existing harness memory, provisions the daemon, wires MCP config, and (optionally) imports prior Claude Code and Codex CLI memory — all from one command. This guide uses it as the bootstrap path; steps 1–4 below also work as a fully manual alternative if you would rather drive each piece yourself.

How much it does depends on how you invoke it:

**Interactive wizard (the default on a terminal)** — a bare `memoryd init` opens with a detection summary (what prior memory was found and how it was discovered), then walks through import, daemon arrangement, and MCP wiring, and closes with a setup summary plus next steps:

```bash
memoryd init   # full wizard: detect → import → daemon → MCP wiring → verify
```

Declining every prompt is a guaranteed no-op — nothing is created or modified unless you opt in. Explicit selector flags (`--import`, `--harness`, `--wire-mcp`, `--daemon`, `--non-git-cwd-default`) pre-answer their prompt instead of being re-asked, and `--print-only` runs the whole wizard as a dry run.

**Agent/CI non-interactive mode** — all decisions via flags, machine-readable JSON report (`SetupReport`) on stdout:

```bash
memoryd init --non-interactive --json --wire-mcp current --daemon on-demand
```

Add `--import --harness current` to bring in prior harness memory. Use `--detect-only` to inspect what is present without mutating anything. The full flag reference and the `SetupReport` JSON shape live in [`docs/agent-onboarding.md`](agent-onboarding.md).

**Note on pipes and CI:** when stdin is not a terminal, a bare `memoryd init` refuses with guidance instead of provisioning anything. Scripted callers must opt in explicitly with `--non-interactive` (or `--json` / `--detect-only`).

When `memoryd init` has provisioned the daemon and wired MCP for you, skip ahead to [step 3 (verify)](#3-verify-daemon-health) and [step 4 (wire MCP)](#4-wire-mcp) to confirm — then continue from there. The manual steps below are the alternative path if you are not using `memoryd init`.

## 1. Build or install

From the repo root:

```bash
bash scripts/install-memorum.sh --force-reinstall --repo "$HOME/memorum" --runtime "$HOME/memorum/.memoryd"
```

The installer builds the dogfood operator binaries (`memoryd`, `memoryd-tui`,
`memoryd-web`, and `memory-merge-driver`) and prints MCP client snippets with
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

## 2. Initialize and start the daemon (manual alternative)

If you bootstrapped with `memoryd init` in step 0, the daemon is already provisioned — skip to step 3. This section is the manual path for operators who want to start the daemon directly or run a custom arrangement.

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

`memoryd serve --init` runs the daemon directly and initializes the substrate on first start. It is the low-level start command — `scripts/install-memorum.sh` uses it under the hood, and you can use it for manual or custom arrangements. Keep this process running. The socket path is what CLIs, the web dashboard, TUI, and MCP bridge use.

## 3. Verify daemon health

In another shell (with the same `MEMORUM_*` exports):

```bash
memoryd status --socket "$MEMORUM_SOCKET"
memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
```

Expected result: `status` returns a ready daemon response, and `doctor` reports either healthy or actionable findings. If doctor reports `events_log_mirror_lag`, run the reindex repair it prints.

## 4. Wire MCP

Add this to your MCP-capable client config. Replace the placeholder socket path with the output of `echo "$MEMORUM_SOCKET"` (or the absolute path printed by `scripts/install-memorum.sh`). Most MCP clients do not expand `~` inside JSON/TOML.

For Claude Code, prefer user-scope wiring so Memorum is available in every project:

```bash
claude mcp add --scope user memorum -- memoryd mcp --socket "/absolute/path/to/memorum/.memoryd/memoryd.sock"
```

Or merge this JSON at the top-level `mcpServers` key of the user config (`$CLAUDE_CONFIG_DIR/.claude.json` or `~/.claude.json`):

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

## 8. Uninstalling

When you want a clean exit, `memoryd uninstall` reverses what `memoryd init` and `scripts/install-memorum.sh` set up. It stops the daemon, removes the launchd plist (macOS), and unwires the `memorum` MCP entry from your harness configs — leaving every sibling server and unrelated setting in place.

```bash
memoryd uninstall --print-only   # preview every step without changing anything
memoryd uninstall                # interactive confirm, then run
```

Your data is preserved by default. To also delete the repo and runtime directories, add `--purge` (you will be asked to confirm the resolved absolute paths on a terminal):

```bash
memoryd uninstall --purge
```

Installed binaries are never removed for you — the final `verify` step prints the `cargo uninstall memoryd memoryd-tui memoryd-web memory-merge-driver` line if you want them gone too.
