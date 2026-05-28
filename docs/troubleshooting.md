# Troubleshooting

Common Memorum first-run failures and how to diagnose them. Symptom → diagnosis → fix.

Run the operator-facing health check anytime you suspect something is off:

```bash
memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
```

`doctor` exits 0 when the substrate is clean and at least one enabled harness CLI is authenticated.

---

## The daemon won't start

**Symptom**: `memoryd serve --init` exits immediately, or `memoryd status` reports the socket is not reachable.

**Diagnose**:

```bash
memoryd status --socket "$MEMORUM_SOCKET"
ls -la "$MEMORUM_RUNTIME/memoryd.sock"
test -f "$MEMORUM_RUNTIME/memoryd.pid" && kill -0 "$(cat "$MEMORUM_RUNTIME/memoryd.pid")"
```

**Fix**:

- If a stale pid file points at a dead process, remove it: `rm "$MEMORUM_RUNTIME/memoryd.pid"`.
- If the socket already exists but no process owns it: `rm "$MEMORUM_RUNTIME/memoryd.sock"` and re-run `memoryd serve --init`.
- If `$MEMORUM_RUNTIME` doesn't exist: `mkdir -p "$MEMORUM_RUNTIME"` and re-run.
- Re-run the installer: `bash scripts/install-memorum.sh --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"`.

---

## `dream_disabled`

**Symptom**: `memoryd dream now` returns `dream_disabled`, or `memoryd status` reports dreaming inactive.

**Diagnose**: dreaming is disabled either in config or by the local sentinel file under the runtime directory.

```bash
ls "$MEMORUM_RUNTIME/dream.disabled" 2>/dev/null && echo "sentinel present"
grep -E "^dream" "$MEMORUM_REPO/config.yaml" 2>/dev/null
```

**Fix**: remove the sentinel (`rm "$MEMORUM_RUNTIME/dream.disabled"`) or update `config.yaml` to enable dreaming, then restart the daemon.

---

## `dream_unavailable`

**Symptom**: dreaming is enabled but cycles return `dream_unavailable`.

**Diagnose**: no supported harness CLI is installed and authenticated in the daemon's environment.

```bash
codex login status    # current Codex CLI
claude auth status    # current Claude Code CLI
```

**Fix**: log into at least one harness CLI in the same shell that launched `memoryd serve`. If `memoryd doctor` reports `harness unauthenticated`, that's the same issue surfaced earlier.

---

## MCP client doesn't list any tools

**Symptom**: Claude Code or Codex CLI loads the MCP server but `/mcp` shows no tools, or `tools/list` returns empty.

**Diagnose**:

```bash
memoryd mcp --socket "$MEMORUM_SOCKET" </dev/null  # should print the MCP handshake then exit
memoryd status --socket "$MEMORUM_SOCKET"          # should show the daemon is live
```

**Fix**:

- The MCP client must launch the stdio bridge with the absolute socket path, not `~/...`. Most clients don't expand `~` inside JSON/TOML config.
- If the daemon isn't running, `memoryd mcp` fails silently — start the daemon first.
- Run the installer's printed MCP snippet verbatim into your client's config.

---

## First `memory_write` returns nothing visible

**Symptom**: An MCP client successfully calls `memory_write`, the response looks OK, but the user can't tell that anything actually landed.

**Diagnose**:

```bash
memoryd search "" --socket "$MEMORUM_SOCKET" --limit 5
memoryd get --id <returned-id> --socket "$MEMORUM_SOCKET"
ls "$MEMORUM_REPO/projects/" "$MEMORUM_REPO/me/" 2>/dev/null
```

The memory is on disk if `memoryd get` returns it. If you ran `memoryd write` directly (not via MCP), the CLI emits a success banner on the **first** write only — subsequent writes do not (by design; see `docs/importer.md` rationale).

**Fix**: nothing's broken — just confirm via `memoryd search` or open the file under `$MEMORUM_REPO/projects/<namespace>/decisions/<id>.md`.

---

## `memoryd doctor` reports findings

**Symptom**: `memoryd doctor` exits non-zero with a list of findings.

**Common findings**:

- `event-log mirror lag` — add `--reindex` to bring the SQLite index back in sync with the events log.
- `harness unauthenticated` — see `dream_unavailable` above.
- `substrate dirty tree` — there are uncommitted changes in `$MEMORUM_REPO` outside the substrate's own namespaces. Inspect with `cd "$MEMORUM_REPO" && git status`; commit or `git clean` as appropriate.
- `unknown harness CLI override` — you passed `--cli <something>` that isn't a production harness. Drop the override or use `mock` only for evals.

---

## `memoryd import` complains

**Symptom**: The importer refuses to start, reports `AnotherImportInProgress`, or skips most sources as `SkipUnchanged`.

**Diagnose**:

```bash
ls "$MEMORUM_REPO/.memorum/import-state.json"      # state file exists?
ls "$MEMORUM_REPO/.memorum/import-state.json.lock" # lock file from a hung run?
cat "$MEMORUM_REPO/.memorum/import.pid" 2>/dev/null
```

**Fix**:

- `AnotherImportInProgress { pid: <N> }` — verify that pid is alive with `kill -0 <N>`. If it's dead, remove the lock file: `rm "$MEMORUM_REPO/.memorum/import-state.json.lock"`.
- All sources reported `SkipUnchanged` — that's correct behavior on re-run of an already-imported corpus. To force a re-import, delete `import-state.json` (or rename it; the importer preserves a corrupt-state copy on parse failure too).
- See `docs/importer.md` for the full conflict-report format and re-import semantics.

---

## Two harnesses see different memory

**Symptom**: Claude Code finds a memory that Codex CLI doesn't (or vice-versa).

**Diagnose**:

```bash
memoryd status --socket "$MEMORUM_SOCKET"        # should show both harnesses connected
memoryd search "<the missing query>" --socket "$MEMORUM_SOCKET"
```

**Fix**:

- Both harnesses must use the same socket path. Check each harness's MCP config.
- If one harness is configured against a stale socket, the daemon never sees its writes — point it at the canonical socket and restart that harness's MCP client.

---

## Where to look when nothing here helps

- Daemon logs: `$MEMORUM_RUNTIME/logs/` (rotating files).
- Events log: `$MEMORUM_REPO/events/<device>.jsonl` — append-only audit trail.
- Reality Check surface (web dashboard): `memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137` and visit `http://localhost:7137`.
- Stream-specific runbooks: `docs/runbooks/`.
- Project context for agents: `CLAUDE.md`.
