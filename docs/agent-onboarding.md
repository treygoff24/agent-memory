# Agent onboarding guide: installing Memorum for a user

This guide is written to you — the AI agent — not the user. Follow it in order. Every command here is grounded in the real `memoryd` CLI surface; do not invent flags.

Related guides:

- A **human operator** setting up by hand should follow [`docs/getting-started.md`](getting-started.md), which uses the same `memoryd init` bootstrap interactively.
- For **build/install on a fresh machine** (`cargo install`, `scripts/install-memorum.sh`), see [`docs/install.md`](install.md).
- When something breaks, [`docs/troubleshooting.md`](troubleshooting.md) maps first-run symptoms to fixes.

`memoryd init` is the unified bootstrap entrypoint for all of these; this guide drives its non-interactive (`--non-interactive --json`) path. Always pass those flags: when stdin is not a terminal, a bare `memoryd init` refuses with guidance rather than provisioning anything, and on a terminal it launches the human wizard instead.

## What you are doing

You are helping a user install Memorum: a local-first daemon that creates one shared memory layer across Claude Code, Codex CLI, and any other MCP-capable harness. You will detect what they already have, get their consent, run setup, verify it, and tell them to restart their session.

The outcome is a running `memoryd serve` daemon with MCP wiring in place and (optionally) prior harness memory imported. The user must restart their harness session after wiring so the MCP config takes effect.

## Overview of the canonical loop

1. Detect — run `memoryd init --detect-only` and read the JSON. Learn what harnesses are installed and whether prior memory exists.
2. Propose — explain to the user what you found and what you plan to do. Get explicit consent before writing anything.
3. Run setup — run `memoryd init --non-interactive --json` with the flags that match their decisions.
4. Verify — read the `SetupReport` JSON; call `memoryd doctor` and `memoryd status` for belt-and-suspenders confirmation.
5. Restart — tell the user to restart their harness session so MCP wiring loads.

Never run a mutating step without the user's explicit yes first.

---

## Step 1: detect

Run detection. This is zero-mutation — no files are written.

```bash
memoryd init --detect-only
```

This emits a JSON detection summary to stdout and exits. Parse the JSON. Key fields:

- Which harnesses were found (Claude Code, Codex CLI, or both).
- Whether memory files exist in each harness's standard location (`~/.claude/projects/` for Claude Code, `~/.codex/memories/` for Codex CLI).
- Whether a Memorum repo already exists at the default location (`~/memorum`).
- Whether a daemon socket is already reachable.

If the detection JSON is empty or the command fails, report the stderr output to the user and stop.

---

## Step 2: propose and get consent

Present your findings in plain language. Then ask the user to make the following decisions. Do not proceed past this point until you have explicit answers.

### Decision 1: which harnesses to wire

Explain: Memorum will write MCP config for the chosen harnesses so they can access shared memory. This modifies the harness config file on disk.

Ask: "Should I wire MCP for the harness you are using now (`current`), for Claude Code (`claude`), for Codex CLI (`codex`), for all detected harnesses (`all`), or none (`none`)?"

Map their answer to `--wire-mcp current|claude|codex|all|none`.

### Decision 2: which harness memory to import

Explain: If they have existing memory files in Claude Code or Codex CLI, Memorum can import them so nothing is lost. This is non-destructive — source files are never modified.

Ask: "Should I import memory from the harness you are using now (`current`), from Claude Code (`claude`), from Codex CLI (`codex`), from all detected harnesses (`all`), or skip import (`none`)?"

Map their answer to `--harness current|claude|codex|all|none`. If they want import, also add `--import`.

### Decision 3: how to handle memories whose project folder is not a git checkout

This only matters if import is enabled. Some prior memories are associated with directories that are not git repositories.

Explain: "Some of your prior memories may be from non-git project folders. I can skip those (`skip`, safe default), assign them to your personal memory bucket (`me`), or auto-generate a project bucket for each (`generate`)."

Map their answer to `--non-git-cwd-default skip|me|generate`. The safe default is `skip`.

### Decision 4: daemon persistence

Explain the options:

- `on-demand` (default): the daemon starts only when an MCP client needs it. Low overhead, requires a compatible launcher. Recommended unless they need persistent background behavior.
- `background`: the daemon runs persistently as a background process.
- `launchd` (macOS only): registers a launchd service so the daemon starts at login and is managed by the OS. **This writes a launchd plist and registers it with `launchctl`.** Get explicit yes before choosing this.
- `none`: no daemon provisioning. They will manage the daemon themselves.

Ask: "How should the daemon be managed? Options: `on-demand`, `background`, `launchd` (macOS, starts at login), or `none` (self-managed)."

Map their answer to `--daemon on-demand|background|launchd|none`.

**Consent summary before running:** Tell the user exactly what you are about to do:
- Initialize a Memorum repo at `~/memorum` (or their chosen path).
- Wire MCP config for their chosen harnesses (names the config files that will be modified).
- Optionally import prior harness memory.
- Set up daemon persistence as chosen.

Then ask: "Should I proceed?" Do not run the next step until they say yes.

---

## Step 3: run setup

Run `memoryd init` with `--non-interactive` and `--json` plus the flags you collected. All output to parse is on stdout; diagnostics go to stderr.

Base command (non-interactive agent mode):

```bash
memoryd init --non-interactive --json \
  --wire-mcp <current|claude|codex|all|none> \
  --daemon <on-demand|background|launchd|none>
```

Add `--import --harness <value>` if the user wants to import prior memory:

```bash
memoryd init --non-interactive --json \
  --import \
  --harness <current|claude|codex|all|none> \
  --non-git-cwd-default <skip|me|generate> \
  --wire-mcp <current|claude|codex|all|none> \
  --daemon <on-demand|background|launchd|none>
```

Optional path overrides (use only if the user has non-default paths):

```bash
  --repo <PATH>      # override the canonical repo root (default: ~/memorum)
  --runtime <PATH>   # override the per-device runtime dir (default: <repo>/.memoryd)
```

Use `--print-only` to preview what would happen without applying side effects (useful to show the user the plan before they commit):

```bash
memoryd init --non-interactive --json --print-only \
  --wire-mcp <value> --daemon <value>
```

### Exit codes

- Exit 0 with JSON on stdout: all steps succeeded. Parse the JSON report.
- Non-zero exit with JSON on stdout: at least one step failed; details are in the report body.
- Non-zero exit with no JSON on stdout: a pre-report fatal error occurred (e.g., detection failed); reason is on stderr.

---

## Step 4: interpret the SetupReport JSON

The JSON object printed to stdout has this shape:

```json
{
  "schema_version": 1,
  "detection": { ... },
  "decisions": { ... },
  "steps": [
    {
      "step": "detect",
      "status": "succeeded",
      "message": null
    },
    {
      "step": "ensure_repo",
      "status": "succeeded",
      "message": null
    },
    {
      "step": "ensure_daemon",
      "status": "succeeded",
      "message": null
    },
    {
      "step": "import",
      "status": "succeeded",
      "message": null
    },
    {
      "step": "wire_mcp",
      "status": "succeeded",
      "message": null
    },
    {
      "step": "verify",
      "status": "succeeded",
      "message": null,
      "verify": {
        "status_probe": "succeeded",
        "doctor_probe": "succeeded"
      }
    }
  ],
  "import_report": null,
  "restart_required": true
}
```

**Fields to check:**

- `restart_required`: if `true`, you MUST tell the user to restart their harness session. MCP wiring does not take effect until the harness is restarted.
- `steps[*].status`: each step reports `succeeded`, `failed`, `skipped`, or `expected`. A `skipped` step means the step was not needed given the chosen flags (e.g., import skipped when `--harness none`). An `expected` step means the outcome was expected given the mode (e.g., a status probe that is absent because no daemon was started).
- `steps[*].message`: human-readable explanation, present on failures. Report this to the user if a step failed.
- `import_report`: present if an import ran; contains per-harness counts of written, skipped, and failed memories.
- `verify.doctor_probe`: if this is `failed`, the substrate or daemon has a real problem regardless of daemon mode. Report the step message and tell the user to run `memoryd doctor` manually.

**Handling failures:**

If any step has `status: "failed"`, read its `message` and report it to the user. The most common recoverable failures are:

- `ensure_daemon` failed: the daemon could not start. Ask the user to check that the binary is in their PATH and that no other process holds the socket.
- `wire_mcp` failed: the config file could not be written. Check file permissions.
- `verify` failed on `doctor_probe`: substrate issue; run `memoryd doctor` for details.

---

## Step 5: verify with doctor and status

Even when the report shows all steps succeeded, run these two probes:

```bash
memoryd doctor
memoryd status
```

If the user has non-default paths, add the path flags:

```bash
memoryd doctor --repo <PATH> --runtime <PATH>
memoryd status --socket <PATH>
```

**Interpreting doctor output:** Doctor reports substrate health. `healthy` means nothing is broken. `events_log_mirror_lag` means the derived SQLite mirror is behind; run the reindex command it prints. Any other failure is a real problem — report the output to the user and stop.

**Interpreting status output:** Status queries the running daemon over its socket. A successful response means the daemon is reachable. If status fails and the daemon mode was `none`, that is expected. If status fails with any other daemon mode, the daemon did not start correctly — check `ensure_daemon` step message in the report.

Neither `doctor` nor `status` has a JSON output mode; their output is human-readable text.

---

## Step 6: restart instruction (mandatory)

**Tell the user this, in plain language, before you consider the task done:**

> Memorum is installed and the daemon is running. MCP wiring has been written to your harness config. You must **restart your harness session** (close and reopen Claude Code, Codex CLI, or whichever harness was wired) for the MCP server to load. After restarting, ask the harness to list available tools — you should see Memorum tools such as `memory_write`, `memory_search`, and `memory_startup`.

Do not mark the onboarding complete until you have delivered this restart instruction and the user acknowledges it.

---

## Reference: full flag surface for memoryd init

These are every flag accepted by `memoryd init`. Do not cite flags outside this list.

```
--repo <PATH>
    Canonical Memorum repo root (default: $MEMORUM_REPO or ~/memorum)

--runtime <PATH>
    Local per-device runtime directory (default: <repo>/.memoryd)

--non-interactive
    Run without prompts; drive setup from flags and emit a machine-readable report.
    Suitable for CI and agent bootstrap.

--json
    Emit machine-readable JSON to stdout.
    Implied by --non-interactive and --detect-only; diagnostics go to stderr.

--detect-only
    Run detection only: no decisions, no steps, zero mutation.
    Emits the detection summary as JSON and exits.

--import
    Import detected harness memory through the daemon during setup.

--harness <current|claude|codex|all|none>
    Harness set to import. Omitted: prompted by the wizard on a TTY;
    defaults to current on the non-interactive path.

--non-git-cwd-default <skip|me|generate>
    Default placement for imported memories whose cwd is not a git checkout.
    Omitted: prompted by the wizard on a TTY; defaults to skip on the
    non-interactive path.

--wire-mcp <current|claude|codex|all|none>
    MCP configs to wire. Omitted: prompted by the wizard on a TTY; defaults
    to current on the non-interactive path.

--daemon <on-demand|background|launchd|none>
    Daemon arrangement to provision during setup. Omitted: prompted by the
    wizard on a TTY; defaults to on-demand on the non-interactive path.

--print-only
    Plan and report every step without applying side effects
    (dry-run import, print-only MCP wiring).
```

## Reference: related subcommands

```
memoryd doctor [--repo <PATH>] [--runtime <PATH>] [--reindex]
    Check local substrate and daemon configuration.
    Note: doctor has no json output flag; output is human-readable text.

memoryd status [--socket <PATH>]
    Query daemon health.
    Note: status has no json output flag; output is human-readable text.

memoryd serve [--repo <PATH>] [--runtime <PATH>] [--socket <PATH>] [--init]
    Run the local daemon directly (for manual or custom arrangements).

memoryd import [--harness <all|claude|codex>] [--dry-run]
               [--from-claude <PATH>] [--from-codex <PATH>]
               [--non-git-cwd-default <skip|me|generate>]
               [--report <PATH>] [--quiet] [--socket <PATH>] [--repo <PATH>]
    Backfill prior harness memory (standalone, outside of init).
```
