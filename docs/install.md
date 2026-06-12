# Installing Memorum

This page covers build and install paths for a fresh machine or a fresh agent
session: getting the `memoryd` binaries onto the box. Use the installer when you
want Memorum built, a local memory repo/runtime prepared, the daemon started,
lifecycle commands printed, and MCP wiring snippets shown in one pass.

Once the binaries are installed, bootstrap the substrate and wire MCP with
`memoryd init` (the unified first-run entrypoint):

- **Human operator:** [`docs/getting-started.md`](getting-started.md) — interactive `memoryd init` plus verify and MCP wiring.
- **AI agent installing for a user:** [`docs/agent-onboarding.md`](agent-onboarding.md) — the scripted `memoryd init --non-interactive --json` loop.
- **Something broke:** [`docs/troubleshooting.md`](troubleshooting.md).

The `scripts/install-memorum.sh` path below also starts the daemon for you
(via `memoryd serve --init` internally), so it doubles as a one-shot
install-and-bootstrap for the local repo.

## Option 1: cargo install from Git

Install the daemon package directly from the Git repository:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true \
  cargo install --git https://github.com/treygoff24/agent-memory.git memoryd --locked --bin memoryd
memoryd --version
```

For reproducible automation, pin the exact revision you reviewed:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true \
  cargo install --git https://github.com/treygoff24/agent-memory.git --rev <COMMIT_SHA> memoryd --locked --bin memoryd
```

Cargo installs one package per invocation. To install the companion binaries,
run one command per package:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git https://github.com/treygoff24/agent-memory.git memoryd-tui --locked
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git https://github.com/treygoff24/agent-memory.git memoryd-web --locked
CARGO_NET_GIT_FETCH_WITH_CLI=true cargo install --git https://github.com/treygoff24/agent-memory.git memory-merge-driver --locked
```

### Verification note

On 2026-06-01, this was checked from the `onboarding/task-06-bootstrap`
worktree with isolated install roots and target directories.

The originally proposed package-selection form is not accepted by Cargo 1.90.0:

```bash
cargo install --git https://github.com/treygoff24/agent-memory.git -p memoryd --locked
```

It failed before dependency resolution with:

```text
error: unexpected argument '-p' found
```

The supported package-selection form is to pass the package name as the crate
argument:

```bash
CARGO_NET_GIT_FETCH_WITH_CLI=true \
  cargo install --git https://github.com/treygoff24/agent-memory.git memoryd --locked --bin memoryd
```

That command completed successfully and compiled the workspace path dependencies
from Cargo's git checkout. `--bin memoryd` keeps the daemon install scoped to
the daemon binary instead of also installing benchmark binaries from the same
package. It installed `memoryd v0.1.0` from
`https://github.com/treygoff24/agent-memory.git#c223c906`.

Without `CARGO_NET_GIT_FETCH_WITH_CLI=true`, Cargo's default git backend reached
the remote but failed to authenticate in the verification environment. The git
CLI path used the local credential setup successfully.

## Option 2: clone and run the bootstrap installer

Use this path when you want the full current binary set (`memoryd`,
`memoryd-tui`, `memoryd-web`, and `memory-merge-driver`) plus daemon startup
and MCP snippets:

```bash
git clone https://github.com/treygoff24/agent-memory.git
cd agent-memory
bash scripts/install-memorum.sh
```

The installer defaults to:

- repo: `$HOME/memorum`
- runtime: `$HOME/memorum/.memoryd`
- socket: `$HOME/memorum/.memoryd/memoryd.sock`

Override those paths when needed:

```bash
bash scripts/install-memorum.sh \
  --repo "$HOME/memorum" \
  --runtime "$HOME/memorum/.memoryd" \
  --socket "$HOME/memorum/.memoryd/memoryd.sock"
```

Use `--dry-run` to inspect the commands without installing or starting
anything:

```bash
bash scripts/install-memorum.sh --dry-run
```

## Agent bootstrap mode

Agents should pass `--agent` when they need a stable machine-readable summary at
the end of installer output:

```bash
bash scripts/install-memorum.sh --agent
```

`--agent` preserves the normal installer output and appends one final line
prefixed with `MEMORUM_AGENT_SUMMARY_JSON=`. The value is JSON with absolute
`repo`, `runtime`, and `socket` paths, plus both a shell-escaped pasteable
`next_command` and an argv-safe `next_command_argv` array.

Example shape:

```text
MEMORUM_AGENT_SUMMARY_JSON={"mode":"agent","repo":"/home/alice/memorum","runtime":"/home/alice/memorum/.memoryd","socket":"/home/alice/memorum/.memoryd/memoryd.sock","next_command":"claude mcp add memorum -- memoryd mcp --socket /home/alice/memorum/.memoryd/memoryd.sock","next_command_argv":["claude","mcp","add","memorum","--","memoryd","mcp","--socket","/home/alice/memorum/.memoryd/memoryd.sock"]}
```

The Claude MCP one-liner uses the same argument grammar as `setup::mcp_wire`:

```bash
claude mcp add memorum -- memoryd mcp --socket "/absolute/path/to/memorum/.memoryd/memoryd.sock"
```

For config-file based clients, use the JSON snippet printed by the installer:

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

Restart the agent or harness session after wiring MCP so the new server is
loaded.
