# Dogfooding day one

1. Install and start the daemon:

   ```bash
   export MEMORUM_REPO="$HOME/memorum"
   export MEMORUM_RUNTIME="$MEMORUM_REPO/.memoryd"
   export MEMORUM_SOCKET="$MEMORUM_RUNTIME/memoryd.sock"

   ./scripts/check-dogfood.sh
   scripts/install-memorum.sh --force-reinstall --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
   ```

   The installer leaves the daemon detached from the install shell, writes a
   durable log at `$MEMORUM_RUNTIME/memoryd.log`, and writes the daemon PID at
   `$MEMORUM_RUNTIME/memoryd.pid`. Use `--force-reinstall` while dogfooding a
   dirty branch because the local binary version can match `Cargo.toml` even
   when the worktree has newer code.

   Verify it from any fresh shell (with the exports above, or re-export them):

   ```bash
   memoryd status --socket "$MEMORUM_SOCKET"
   kill -0 "$(cat "$MEMORUM_RUNTIME/memoryd.pid")"
   ```

2. Paste the MCP client snippet printed by `scripts/install-memorum.sh` into your client config. The installer emits an absolute `--socket` path suitable for JSON/TOML (do not hand-write a `--runtime` MCP snippet with `~`).

3. Write the first memory:

   ```bash
   memoryd write-note --socket "$MEMORUM_SOCKET" "I dogfooded Memorum on 2026-05-24."
   ```

4. Search it:

   ```bash
   memoryd search --socket "$MEMORUM_SOCKET" "dogfood"
   ```

5. Optional scheduler:

   ```bash
   scripts/install-launchd.sh --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"
   ```

6. Optional manual dream:

   ```bash
   memoryd dream now --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --scope me
   ```

7. Web dashboard:

   ```bash
   memoryd web enable --socket "$MEMORUM_SOCKET"
   open http://127.0.0.1:7137
   ```

8. Weekly Reality Check:

   ```bash
   memoryd reality-check run --socket "$MEMORUM_SOCKET"
   ```

9. TUI:

   ```bash
   memoryd ui --socket "$MEMORUM_SOCKET" --panel 9
   ```

   Panel 9 is Recall. It shows recent daemon recall-hit events when the socket is reachable.

## Gates

Use `./scripts/check-fast.sh` for implementation loops. It runs formatting,
shell syntax, targeted clippy/checks for the dogfood crates, JS formatting and
linting when the repo tooling is installed, specgate when available, and the
baseline-discipline check. It intentionally does not run workspace-wide tests,
release tests, durability, convergence, or benches.

Use `./scripts/check-dogfood.sh` before installing. It calls the fast gate, then
adds the focused dogfood tests for the Recall TUI panel, panic restore, doctor
health, startup recall peer-update references, live-harness skip honesty without
provider keys, and the minimal `memoryd` feature compile.

Reserve `bash scripts/check.sh` for release confidence. It is intentionally
broader and slower than the dogfood bar.

## Optional local harness-auth smoke

If `codex` or `claude` is installed and authenticated in your shell, `memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"` should exit 0 when the substrate is otherwise clean.

Useful direct checks:

```bash
codex login status   # current Codex CLI
claude auth status   # current Claude Code CLI
```

Older CLIs may use legacy auth status commands (`codex auth status`, `claude config get auth.user`); Memorum falls back only when the preferred current command is unsupported.

## Troubleshooting

- `dream_disabled`: dreaming is disabled in config or by the local sentinel under the runtime directory.
- `dream_unavailable`: no supported harness CLI is installed and authenticated in the daemon environment.
- `unknown harness CLI override`: the selected `--cli` is not a production harness.
- Socket errors: verify daemon liveness with `memoryd status --socket "$MEMORUM_SOCKET"` and `kill -0 "$(cat "$MEMORUM_RUNTIME/memoryd.pid")"`.
- Stop the installer-started daemon with `kill "$(cat "$MEMORUM_RUNTIME/memoryd.pid")"`.
- Restart it with `scripts/install-memorum.sh --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"`.
- `memoryd status --socket "$MEMORUM_SOCKET"` only proves the daemon socket is reachable.
- `memoryd doctor --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME"` is the health/auth check for this install. Add `--reindex` if doctor reports event-log mirror lag. It exits 0 only when substrate checks are clean and at least one enabled harness CLI is authenticated. Missing one harness is a warning if another enabled harness works; no authenticated harnesses is unhealthy because dreams cannot run.
