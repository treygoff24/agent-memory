---
name: using-memorum
description: Use when asked to back up, import, or backfill prior agent memory (Claude Code / Codex CLI) into Memorum, or to operate the memoryd CLI — searching memory, reading/writing/forgetting memories, checking daemon health, or activating imported memories. Covers the non-interactive agent flow end to end.
---

# Operating Memorum from the CLI

Memorum is a local-first daemon that gives Claude Code, Codex CLI, and any MCP-capable harness one shared memory layer. You drive it with the `memoryd` CLI; this skill is the non-interactive flow for backing up prior memory and operating the store.

Deeper reference — full flag tables, the reconciliation report schema, troubleshooting, and the exit-code contract — is in `docs/agent-import-guide.md`. Read it when something here is underspecified.

## Paths and env vars

Every command needs to know the repo and socket. Use these if the user has them exported; otherwise the defaults below apply.

```bash
MEMORUM_REPO="${MEMORUM_REPO:-$HOME/memorum}"
MEMORUM_SOCKET="${MEMORUM_SOCKET:-$MEMORUM_REPO/.memoryd/memoryd.sock}"
```

Two flag conventions, and they differ by subcommand:

- Daemon commands (`status`, `search`, `get`, `review`, `forget`, `export`) take `--socket`.
- `doctor` takes `--repo` / `--runtime` (it inspects the substrate directly). It now also tolerates `--socket` so a single scripted loop can pass the same flags everywhere.

## 1. Confirm Memorum is running

```bash
memoryd status --socket "$MEMORUM_SOCKET"   # daemon reachable over the socket?
memoryd doctor --repo "$MEMORUM_REPO"        # substrate healthy?
```

`status` returning a `result.success` payload means the daemon is live. `doctor` exits 0 and reports `healthy: true` when the store is clean. If `status` fails, the daemon isn't running — start it (or tell the user to) before doing anything else.

## 2. Import prior memory — one command

This is the headline. It backs up everything the user has taught Claude Code and Codex CLI into Memorum.

```bash
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET"
```

What that one invocation does, by default:

- **Claude**: auto-detects and imports the **union of all Claude profile roots** (`~/.claude*/projects`). A user with three profiles gets all three — no flag needed.
- **Codex**: imports from `~/.codex/memories`.
- **Non-git-cwd memories**: placed in `me` scope (saved, never silently skipped).
- **Me-scope imports**: auto-activated — recall-visible immediately.
- **Malformed YAML frontmatter**: recovered leniently; the body always imports.

Run it once. It's idempotent and non-destructive: source files are never modified, and re-runs skip unchanged sources by content hash. Running it twice is safe and cheap.

To pin exact Claude roots instead of auto-detecting, pass `--from-claude` (repeatable):

```bash
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET" \
  --from-claude ~/.claude/projects --from-claude ~/.claude-work/projects
```

Preview without writing anything:

```bash
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET" --dry-run
```

## 3. Read the result

The run ends with a reconciliation summary:

```
imported-active: 294   queued-for-review: 0   privacy-blocked: 6
frontmatter-recovered: 3   dropped: 1
next: memoryd search "<topic>" --socket <sock>
```

Read it like this:

- **imported-active** — written and recall-visible now. The win.
- **queued-for-review** — candidates awaiting activation (only happens with `--no-activate`, or for governance-quarantined items). Activate later with `memoryd review approve-imports`.
- **privacy-blocked** — Stream D refused these (PII, contacts, donor data). **This is by design, not an error.** They appear in the report's `refusals[]`. Don't retry them or report them as failures.
- **frontmatter-recovered** — had broken YAML; body imported anyway. Fine.
- **dropped** — truly unreadable files, listed in the report. The only real data loss; mention them to the user.

For machine-readable output, add `--report run.json` and parse the JSON.

## 4. Exit codes

`import` exits **0 on success even when some writes were refused or recovered** — those are reported, not failures. A privacy refusal does not fail the run.

Non-zero means a **hard** failure only:

- Can't reach the daemon.
- Lock contention: `AnotherImportInProgress { pid: <N> }` — another import holds the lock.
- Unreadable repo.

## 5. Common follow-ups

```bash
# Search memory
memoryd search "delegate droid alias" --socket "$MEMORUM_SOCKET"

# Read one memory in full by id
memoryd get <id> --socket "$MEMORUM_SOCKET"

# See what's queued for review (candidates + quarantine)
memoryd review queue --socket "$MEMORUM_SOCKET"

# Bulk-activate import candidates (only needed after --no-activate)
memoryd review approve-imports --socket "$MEMORUM_SOCKET"

# Remove one memory
memoryd forget <id> --socket "$MEMORUM_SOCKET"

# Dump the store
memoryd export --socket "$MEMORUM_SOCKET"
```

After a search, if you need the whole memory and not just the snippet, follow up with `memoryd get <id>`.

## Gotchas

- The default `memoryd import` already covers multi-profile Claude setups. Don't reach for `--from-claude` unless the user wants to pin specific roots.
- Privacy refusals are expected output, not a problem to fix. Report the count; move on.
- If a re-run reports everything as skipped/unchanged, that's correct — the corpus is already imported.
