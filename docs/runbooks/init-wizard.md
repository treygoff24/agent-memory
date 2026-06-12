# Runbook: `memoryd init`

`memoryd init` is the unified first-run bootstrap entrypoint. It detects existing
Claude Code and Codex CLI memory, provisions the daemon, wires MCP config, and
(optionally) imports prior harness memory.

On a terminal, a bare `memoryd init` runs the full interactive wizard:
detection summary (including *how* each memory root was discovered — env var,
settings file, or default path), import, daemon arrangement, MCP wiring, and a
closing summary with next steps. Declining every prompt is a guaranteed no-op.
Explicit selector flags pre-answer their prompt; `--print-only` makes the whole
run a dry run.

When stdin is **not** a terminal, a bare `memoryd init` refuses with guidance
rather than provisioning anything — scripted callers must pass
`--non-interactive` (or `--json` / `--detect-only`) explicitly. The older
**detect-and-advise** advisory output is fully removed.

For the current, authoritative guidance:

- **AI agent installing Memorum for a user:** [`docs/agent-onboarding.md`](../agent-onboarding.md) — the full detect → consent → run → verify → restart loop, with the complete `memoryd init` flag reference and the `SetupReport` JSON shape.
- **Human operator setting up by hand:** [`docs/getting-started.md`](../getting-started.md) — interactive `memoryd init` bootstrap, daemon verification, and MCP wiring.
- **Build / install on a fresh machine:** [`docs/install.md`](../install.md).
- **First-run failures:** [`docs/troubleshooting.md`](../troubleshooting.md).
