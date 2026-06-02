# Runbook: fresh-agent onboarding smoke (human gate)

This is the manual acceptance gate for the agent-driven onboarding feature. A human (Trey) runs it to confirm that a brand-new agent session, dropped into an empty directory with nothing but the published instructions, can stand up Memorum end to end against a **real** harness.

This gate is intentionally manual. It uses a real Claude Code or Codex CLI session, real authentication, real MCP wiring against your live config, and a real daemon — none of which the automated suite touches. The automated counterpart is `crates/memoryd/tests/setup_end_to_end.rs`, which drives the same `memoryd init` spine against an ephemeral daemon and substrate but with `--wire-mcp none` and synthetic fixtures. Passing that test is necessary but not sufficient; this runbook is what proves the human-facing story works.

> This runbook has **not** been executed by the author. It is written to be followed step by step. Do not treat any step as pre-verified.

## What this gate proves

A fresh agent, given only `docs/agent-onboarding.md`, can:

1. detect that there is harness memory to import (or correctly report that there isn't);
2. initialize a Memorum repo and bring up a daemon;
3. wire its own harness's MCP config to point at that daemon;
4. verify the daemon answers and the substrate is healthy;
5. surface a truthful machine-readable report of what happened, including whether the harness needs a restart to pick up the new MCP server.

## Prerequisites

- A release or debug `memoryd` binary on `PATH` (or note its absolute path and substitute it below).
- One real, authenticated harness CLI you are willing to point at a live daemon:
  - **Claude Code** — `claude` on `PATH`, logged in (`claude auth status` shows `loggedIn: true`); or
  - **Codex CLI** — `codex` on `PATH`, authenticated.
- `docs/agent-onboarding.md` present in the repo you hand the agent. (At the time this runbook was written that doc is the agent-facing entry point for onboarding; if it has been renamed, point the agent at its successor.)
- A throwaway empty directory for the agent's working tree and a throwaway repo path for the Memorum substrate, so this gate never mutates your real `~/memorum`.

Decide up front whether you are testing the **import** path. Importing reads your real `~/.claude/projects` or `~/.codex/memories`. If you do not want that, either skip `--import` or pin discovery to a fixture tree with `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (see the importer docs).

## Setup

Pick scratch paths and export them so every command below is copy-pasteable:

```bash
export SMOKE_AGENT_DIR="$(mktemp -d /tmp/memorum-smoke-agent.XXXXXX)"
export SMOKE_REPO="$(mktemp -d /tmp/memorum-smoke-repo.XXXXXX)/memorum"
export SMOKE_RUNTIME="$SMOKE_REPO/.memoryd"
```

Keep these paths short — the daemon binds a Unix-domain socket at `$SMOKE_RUNTIME/memoryd.sock`, and macOS rejects socket paths over ~104 characters.

## Step 1 — start a fresh agent session in the empty dir

Open a new agent session (Claude Code or Codex CLI) with its working directory set to `$SMOKE_AGENT_DIR`. This must be a clean session: no prior context, no memory of this codebase. The point of the gate is that onboarding works from zero.

Give the agent exactly one instruction, nothing more:

> Read `docs/agent-onboarding.md` (in `<path-to-this-repo>`) and follow it to onboard this machine onto Memorum. Use repo path `$SMOKE_REPO` and runtime `$SMOKE_RUNTIME`. Report back the JSON you get from `memoryd init` and what you did with it.

Do **not** hand-hold past this. If the agent has to ask you what to run, that is itself a finding — note it. The onboarding doc is supposed to be self-sufficient for an agent.

## Step 2 — watch the agent run `memoryd init`

The agent should arrive at an invocation shaped like this (the exact harness/daemon/wire flags depend on what `docs/agent-onboarding.md` instructs):

```bash
memoryd init \
  --non-interactive --json \
  --import --harness current \
  --non-git-cwd-default me \
  --wire-mcp current \
  --daemon background \
  --repo "$SMOKE_REPO" \
  --runtime "$SMOKE_RUNTIME"
```

Confirm the contract holds:

- **stdout is pure JSON.** Pipe it through a parser if in doubt: the agent should be able to `memoryd init ... | jq .` with no leading/trailing noise. Every human-readable diagnostic must be on stderr. If stdout has anything that is not the JSON `SetupReport`, that is a hard failure.
- **exit code matches the body.** Exit `0` means no fatal step. A non-zero exit with a JSON body on stdout means a setup step failed fatally (read the body). A non-zero exit with empty stdout means it failed *before* producing a report (reason on stderr).

## Step 3 — read the `SetupReport`

The JSON has one entry per step under `steps[]` (`detect`, `ensure_repo`, `ensure_daemon`, `import`, `wire_mcp`, `verify`) plus a top-level `restart_required`. Check, in order:

1. `ensure_repo` is `succeeded` — the Memorum repo was initialized at `$SMOKE_REPO`.
2. `ensure_daemon` is `succeeded` and its message names a pid and the socket — a real daemon is up.
3. `wire_mcp` is `succeeded` (not `skipped`) for your harness — its MCP config was actually rewritten to point at the daemon. The automated test runs `--wire-mcp none`, so this step is *only* exercised here.
4. `verify` is `succeeded`, and `verify.status_probe` / `verify.doctor_probe` are both `succeeded` — the daemon answered a status request and the in-process doctor found a healthy substrate.
5. `restart_required` reflects reality: if `wire_mcp` rewrote a config your live harness has already loaded, this should be `true`, and the agent must tell you to restart the harness before the MCP server appears.

### Expected import behavior (read this before judging the import step)

Importing through a live daemon **lands** memories as governance candidates. The importer tags writes with a groundable `file:`-prefixed absolute `source_ref`, setup provisions the local privacy key, and the built-in `*-strict` policies accept the write. So with `--import`, expect the `import` step to be `succeeded` and the per-harness counters to show:

- `parsed >= 1` — the source corpus was read,
- `written_candidate >= 1` — each memory landed as a governance candidate (not `written_new`; imports land below hand-written confidence),
- `refused_grounding = 0` and `refused_privacy = 0` — no refusals.

**Fail the gate if you see `refused_grounding >= 1`** (or any `refused_privacy`): that is the regression `setup_end_to_end.rs` exists to catch (a non-groundable `source_ref` or a missing privacy key). On a clean re-run over the unchanged corpus the same sources show as `skipped_idempotent` rather than re-written. Step 5 should then show the imported memories in recall / a substrate query.

If you only want to validate the onboarding spine (repo + daemon + MCP wiring + verify) without the import, drop `--import` for this gate.

## Step 4 — confirm the harness actually talks to the daemon

This is the payoff and the part no automated test covers.

1. If `restart_required` was `true`, restart your harness session so it reloads the MCP config the agent just wrote.
2. In the harness, confirm the Memorum MCP server is connected and its tools are listed (e.g. `memory_search`, `memory_get`, `memory_startup`). The exact surfacing is harness-specific; for Claude Code, the MCP server should appear in the connected-servers list.
3. Ask the agent to run a Memorum tool round-trip — for example, search for anything, or fetch the startup recall block. A clean empty result is success; a transport error is a failure.

If the tools are present and answer, MCP wiring is genuinely correct against your real config. If they are absent, check `restart_required` handling (Step 3.5) and the `wire_mcp` step message.

## Step 5 — confirm onboarding state on disk

Independently of the agent, verify what actually landed:

```bash
# The repo was initialized (canonical namespaces, policies, git).
ls "$SMOKE_REPO"

# The daemon is healthy and the substrate is clean.
memoryd doctor --repo "$SMOKE_REPO" --runtime "$SMOKE_RUNTIME"
```

`memoryd doctor` should report a healthy substrate. It may also emit a `harness_cli_warning` if a harness CLI on your machine is unauthenticated — that is an environment advisory about dreaming harness availability, not a substrate problem, and does not fail this gate.

If you ran the import, confirm the imported memories are queryable (e.g. via a `memory_search` from the harness, or the TUI/web dashboard). The importer grounds its writes, so the imported memories land as governance candidates and become queryable — see Step 3.

## Step 6 — tear down

Kill the daemon and remove the scratch trees:

```bash
# Stop the daemon (find it by the scratch socket path).
pkill -f "memoryd serve.*$SMOKE_RUNTIME" || true

# Remove scratch directories.
rm -rf "$SMOKE_AGENT_DIR" "$(dirname "$SMOKE_REPO")"
```

Confirm no `memoryd serve` process survives that points at your scratch runtime:

```bash
pgrep -fl "memoryd serve.*$SMOKE_RUNTIME" || echo "no orphaned daemon"
```

## Pass / fail criteria

The gate **passes** when all of the following held without you hand-holding the agent past Step 1:

- The agent onboarded from `docs/agent-onboarding.md` alone.
- `memoryd init` emitted a pure-JSON `SetupReport` on stdout with the exit code matching the body.
- `ensure_repo`, `ensure_daemon`, `wire_mcp`, and `verify` were all `succeeded`; both verify probes were `succeeded`.
- `restart_required` was honored — if `true`, the agent told you to restart, and after restart the harness saw the Memorum MCP tools.
- A Memorum MCP tool round-trip succeeded inside the real harness.
- `memoryd doctor` reported a clean substrate (harness-CLI warnings excepted).
- Teardown left no orphaned daemon.

The gate **fails** if the agent needed manual rescue to get `memoryd init` to run, if stdout was not pure JSON, if any of the above steps reported a state inconsistent with reality, or if the harness could not reach the daemon after onboarding.

Record any deviation (especially anything the agent had to be told that the onboarding doc should have covered) as a finding against `docs/agent-onboarding.md` or the init flow.
