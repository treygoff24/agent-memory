# Dogfooding day one

1. Install and start the daemon:

   ```bash
   ./scripts/check-dogfood.sh
   scripts/install-memorum.sh --force-reinstall --repo ~/memorum --runtime ~/memorum/.memoryd
   ```

   The installer leaves the daemon detached from the install shell, writes a
   durable log at `~/memorum/.memoryd/memoryd.log`, and writes the daemon PID at
   `~/memorum/.memoryd/memoryd.pid`. Use `--force-reinstall` while dogfooding a
   dirty branch because the local binary version can match `Cargo.toml` even
   when the worktree has newer code.

   Verify it from any fresh shell:

   ```bash
   memoryd status
   kill -0 "$(cat ~/memorum/.memoryd/memoryd.pid)"
   ```

2. Paste the printed MCP client snippet into your client config. The MCP command shape is:

   ```json
   { "command": "memoryd", "args": ["mcp", "--runtime", "~/memorum/.memoryd"] }
   ```

3. Write the first memory:

   ```bash
   memoryd write-note "I dogfooded Memorum on 2026-05-04."
   ```

4. Search it:

   ```bash
   memoryd search "dogfood"
   ```

5. Optional scheduler:

   ```bash
   scripts/install-launchd.sh --repo ~/memorum --runtime ~/memorum/.memoryd
   ```

6. Optional manual dream:

   ```bash
   memoryd dream now --repo ~/memorum --runtime ~/memorum/.memoryd --scope me
   ```

7. Web dashboard:

   ```bash
   memoryd web enable
   open http://127.0.0.1:7137
   ```

8. Weekly Reality Check:

   ```bash
   memoryd reality-check run
   ```

9. TUI:

   ```bash
   memoryd ui --panel 9
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

## Troubleshooting

- `dream_disabled`: dreaming is disabled in config or by the local sentinel under the runtime directory.
- `dream_unavailable`: no supported harness CLI is installed and authenticated in the daemon environment.
- `unknown harness CLI override`: the selected `--cli` is not a production harness.
- Socket errors: verify daemon liveness with `memoryd status` and `kill -0 "$(cat ~/memorum/.memoryd/memoryd.pid)"`.
- Stop the installer-started daemon with `kill "$(cat ~/memorum/.memoryd/memoryd.pid)"`.
- Restart it with `scripts/install-memorum.sh --repo ~/memorum --runtime ~/memorum/.memoryd`.
- `memoryd status` only proves the daemon socket is reachable.
- `memoryd doctor --repo ~/memorum --runtime ~/memorum/.memoryd` is the health/auth check for this install. Add `--reindex` if doctor reports event-log mirror lag. It exits 0 only when substrate checks are clean and at least one enabled harness CLI is authenticated. Missing one harness is a warning if another enabled harness works; no authenticated harnesses is unhealthy because dreams cannot run.
