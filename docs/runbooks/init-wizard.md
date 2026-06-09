# Runbook: `memoryd init`

`memoryd init` is the unified first-run bootstrap entrypoint. It detects existing
Claude Code and Codex CLI memory, provisions the daemon, wires MCP config, and
(optionally) imports prior harness memory.

This runbook used to document an older **detect-and-advise** behavior, where
`memoryd init` only reported what it found and printed `memoryd serve --init`
plus `memoryd import` as next steps. That advisory output is now superseded: it
survives only as the no-action-flag fallback on an interactive terminal. The
real bootstrap runs through the shared setup engine on either the interactive
(`--import` / `--print-only`) or non-interactive (`--non-interactive --json`)
path.

For the current, authoritative guidance:

- **AI agent installing Memorum for a user:** [`docs/agent-onboarding.md`](../agent-onboarding.md) — the full detect → consent → run → verify → restart loop, with the complete `memoryd init` flag reference and the `SetupReport` JSON shape.
- **Human operator setting up by hand:** [`docs/getting-started.md`](../getting-started.md) — interactive `memoryd init` bootstrap, daemon verification, and MCP wiring.
- **Build / install on a fresh machine:** [`docs/install.md`](../install.md).
- **First-run failures:** [`docs/troubleshooting.md`](../troubleshooting.md).
