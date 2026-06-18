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

   You can also have the installer invoke the same launchd setup with
   `scripts/install-memorum.sh --with-scheduler ...`; this works even when the
   installer is called by absolute path from outside the repo root.

6. Optional manual dream:

   ```bash
   memoryd dream now --repo "$MEMORUM_REPO" --runtime "$MEMORUM_RUNTIME" --scope me
   ```

7. Web dashboard:

   The dashboard is auth-gated. `memoryd web enable` mints a bearer token and
   prints it inside the launch URL as `?auth=<token>`. Opening that URL in a
   browser authenticates the session (it sets an `HttpOnly` cookie and redirects
   to the clean URL). Headless `curl` must send the token in the
   `x-memorum-dashboard-auth` header instead, and protected `/api` reads also
   require the CSRF token from the dashboard HTML shell.

   ```bash
   launch_url="$(memoryd web enable --socket "$MEMORUM_SOCKET" --port 7137 | sed -n 's/^Web dashboard enabled at //p')"
   auth_token="${launch_url#*?auth=}"

   # The CSRF token lives in the shell's <meta name="csrf-token"> tag; the shell
   # itself needs only the dashboard auth token.
   csrf_token="$(curl -fsS -H "x-memorum-dashboard-auth: $auth_token" http://127.0.0.1:7137/ \
     | tr '\n' ' ' | sed -n 's/.*name="csrf-token"[^>]*content="\([^"]*\)".*/\1/p' | head -n1)"

   api() { curl -fsS -H "x-memorum-dashboard-auth: $auth_token" -H "x-memorum-csrf: $csrf_token" "$@"; }
   api http://127.0.0.1:7137/api/status >/dev/null
   api 'http://127.0.0.1:7137/api/roi?window=90' >/dev/null
   api http://127.0.0.1:7137/api/policy-editor >/dev/null

   # The notifications stream is a bootstrap route — only the dashboard auth token is required.
   curl -fsS -H "x-memorum-dashboard-auth: $auth_token" --max-time 3 -N http://127.0.0.1:7137/api/notifications/stream || true

   open "$launch_url"   # browser authenticates via the ?auth= token, then drops it from the URL
   ```

   The bearer token is printed only by `web enable`, never by `memoryd web status`, so capture it at enable time.

   `/api/roi` is not full business ROI. It is the alpha operational metric set
   for promotion/refusal/dream/Reality Check signals, and daemon mode should
   return live or zero metrics rather than fixture/deferred data.

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
provider keys, minimal `memoryd` feature compile, daemon-backed dashboard ROI,
notifications SSE, local-artifact source capture, policy GET/validate/write,
and the eval alpha release-set dry run.

The dogfood gate intentionally fails if an alpha surface is still a fixture,
placeholder, or unclassified deferral. Acceptable alpha skips must say why they
are unsupported alpha scope, such as browser-rendered source capture, device
pairing, or model/semantic privacy classification.

`check-dogfood.sh` chooses a free local web smoke port in the 7137-7199 range.
Set `MEMORUM_DOGFOOD_WEB_PORT=7137` only when you specifically want to force the
normal dashboard port.

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
