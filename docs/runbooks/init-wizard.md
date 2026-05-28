# Runbook: `memoryd init`

The init wizard is the explicit-consent on-ramp for a new Memorum user. It detects existing Claude Code and Codex CLI memory directories, reports what it sees, and (when run interactively) offers to import.

## When to use

- First-time setup on a new machine.
- After a fresh clone of `~/memorum`, when you want to see what would get pulled in.
- Anytime you want to inspect harness-memory detection without running an import.

## What it does

1. Resolves `$MEMORUM_REPO` (or `~/memorum`), runtime dir, and socket path with sensible defaults.
2. Notes whether `$MEMORUM_REPO/.memorum/` already exists — if so, switches to **detection-only** mode (no re-init, no destructive changes).
3. Counts Claude Code topic files at the discovered memory root (default `~/.claude/projects/<encoded>/memory/`).
4. Checks whether `~/.codex/memories/MEMORY.md` exists.
5. If any memory is detected and stdin is a TTY, prompts:

   ```
   Would you like to import detected harness memory now? (Y/n)
   ```

   Default is **yes** (the whole point of the wizard is to be the on-ramp).
6. Prints next steps: daemon-start command, `memoryd doctor`, `docs/troubleshooting.md`, `docs/importer.md`.

## ASCII walkthrough

```
$ memoryd init
Memorum init
  repo:    /Users/u/memorum
  runtime: /Users/u/memorum/.memoryd
  socket:  /Users/u/memorum/.memoryd/memoryd.sock

Detected harness memory:
  Claude Code: 47 memory topic file(s)
  Codex CLI:   MEMORY.md present

Would you like to import detected harness memory now? (Y/n) y

Run this command in a separate shell once the daemon is up:
  memoryd import --repo "/Users/u/memorum" --socket "/Users/u/memorum/.memoryd/memoryd.sock"

Next steps:
  - Start daemon: memoryd serve --init --repo "/Users/u/memorum" --runtime "/Users/u/memorum/.memoryd" --socket "/Users/u/memorum/.memoryd/memoryd.sock"
  - Health check: memoryd doctor --repo "/Users/u/memorum" --runtime "/Users/u/memorum/.memoryd"
  - Troubleshooting: docs/troubleshooting.md
  - Importer details: docs/importer.md
```

## Flags

- `--repo <path>` — override the default `$MEMORUM_REPO`/`~/memorum`.
- `--runtime <path>` — override the default `<repo>/.memoryd`.
- `--non-interactive` — suitable for CI: never prompts, exits without running the importer. Use `memoryd import` explicitly afterward when ready.

## Detection-only mode

When `$MEMORUM_REPO/.memorum/` already exists, the wizard reports what's on disk and exits without changing anything. Run `memoryd import` directly when you actually want to re-import.

## Troubleshooting

- **"0 memory topic file(s)" for a harness you actually use** — set `CLAUDE_CONFIG_DIR` or `CODEX_HOME` to point at the right tree. The wizard honors the same discovery precedence as the importer.
- **Wizard hangs at the prompt** — stdin isn't a TTY. Rerun with `--non-interactive` and invoke `memoryd import` later.
- **MCP client doesn't see new memories after import** — see `docs/troubleshooting.md` "MCP client doesn't list any tools."
