# Getting started with Memorum

This guide is for a human operator setting Memorum up by hand. It uses `memoryd init` to bootstrap, verifies the daemon, and confirms the passive-recall lifecycle hooks that `init` wires by default. The default agent surface is the `memoryd` CLI plus the `using-memorum` skill plus those auto-wired hooks; the MCP bridge is an opt-in compatibility path covered at the end.

Related guides — each says who it is for:

- **You are an AI agent installing Memorum for a user?** Follow [`docs/agent-onboarding.md`](agent-onboarding.md) instead — the scripted detect → consent → run → verify → restart loop built on `memoryd init --non-interactive --json`. ([`llms-install.md`](../llms-install.md) is the short pointer to it.)
- **Installing on a fresh machine from Git, or want the full binary set built in one pass?** See [`docs/install.md`](install.md) for `cargo install` and `scripts/install-memorum.sh`.
- **Something went wrong?** [`docs/troubleshooting.md`](troubleshooting.md) covers `dream_disabled`, socket errors, MCP listing empty, and the other first-run failure modes.

## 0. Bootstrap with `memoryd init`

`memoryd init` is the unified first-run entrypoint. It detects your existing harness memory, provisions the daemon, wires passive-recall lifecycle hooks by default, and (optionally) imports prior Claude Code and Codex CLI memory — all from one command. MCP wiring is opt-in: it happens only when you pass `--wire-mcp <harness>`. This guide uses `init` as the bootstrap path; steps 1–4 below also work as a fully manual alternative if you would rather drive each piece yourself.

How much it does depends on how you invoke it:

**Interactive wizard (the default on a terminal)** — a bare `memoryd init` opens with a detection summary (what prior memory was found and how it was discovered), then walks through import, daemon arrangement, and hook wiring, and closes with a setup summary plus next steps. It offers optional MCP wiring only if you ask for it:

```bash
memoryd init   # full wizard: detect → import → daemon → recall hooks → verify
```

Declining every prompt is a guaranteed no-op — nothing is created or modified unless you opt in. Explicit selector flags (`--import`, `--harness`, `--wire-mcp`, `--daemon`, `--non-git-cwd-default`) pre-answer their prompt instead of being re-asked, and `--print-only` runs the whole wizard as a dry run.

**Agent/CI non-interactive mode** — all decisions via flags, machine-readable JSON report (`SetupReport`) on stdout:

```bash
memoryd init --non-interactive --json --daemon on-demand
```

This wires the passive-recall hooks by default and leaves MCP unwired (`--wire-mcp none`). Add `--wire-mcp current` (or `claude`/`codex`/`all`) only if you also want the opt-in MCP bridge. Add `--import --harness current` to bring in prior harness memory. Use `--detect-only` to inspect what is present without mutating anything. The full flag reference and the `SetupReport` JSON shape live in [`docs/agent-onboarding.md`](agent-onboarding.md).

**Note on pipes and CI:** when stdin is not a terminal, a bare `memoryd init` refuses with guidance instead of provisioning anything. Scripted callers must opt in explicitly with `--non-interactive` (or `--json` / `--detect-only`).

When `memoryd init` has provisioned the daemon and wired the recall hooks for you, skip ahead to [step 3 (verify)](#3-verify-daemon-health) to confirm — then continue from there. [Step 4 (wire MCP)](#4-wire-mcp-optional-compatibility-path) is only needed if you opted into MCP. The manual steps below are the alternative path if you are not using `memoryd init`.

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

Expected result: `status` prints the v1 agent envelope (`{"ok":true,"data":{...},"meta":{"schema_version":"1.0","warnings":[]}}`) on stdout and exits 0 when the daemon is reachable, and `doctor` reports either healthy or actionable findings (`doctor` keeps its own raw daemon frame and 0/1 exit — it is not enveloped). If doctor reports `events_log_mirror_lag`, run the reindex repair it prints.

Once the daemon is up and the recall hooks are wired, the default agent surface is ready: an agent reads and writes memory through the `memoryd` CLI (`search`, `get`, `write`, `write-note`, `supersede`, `forget`, `observe`) following `skills/using-memorum/SKILL.md`, and each session gets recall injected automatically. The MCP step below is only for shell-less or MCP-only clients.

## 4. Wire MCP (optional compatibility path)

Wire MCP only if you have a shell-less or MCP-only harness that cannot use the CLI + hooks surface. Add this to your MCP-capable client config. Replace the placeholder socket path with the output of `echo "$MEMORUM_SOCKET"` (or the absolute path printed by `scripts/install-memorum.sh`). Most MCP clients do not expand `~` inside JSON/TOML.

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

The default (CLI) smoke path — `write-note` then `search`, both enveloped on stdout:

```bash
memoryd write-note --socket "$MEMORUM_SOCKET" "Memorum local smoke note from getting-started."
memoryd search --socket "$MEMORUM_SOCKET" "local smoke note"
```

A successful round-trip returns the new memory id in the search hits. If you wired the opt-in MCP bridge, the same round-trip works from an MCP client: call `memory_write` with a grounded project fact, then `memory_search` for a distinctive phrase from it.

## 6. Optional observability

```bash
memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137
# Open the launch URL printed by the command.
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
