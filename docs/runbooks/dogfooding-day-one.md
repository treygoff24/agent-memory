# Dogfooding day one

1. Install and start the daemon:

   ```bash
   scripts/install-memorum.sh --repo ~/memorum --runtime ~/memorum/.memoryd
   ```

2. Paste the printed MCP client snippet into your client config. The MCP command shape is:

   ```json
   { "command": "memoryd", "args": ["mcp", "--socket", "/tmp/memoryd.sock"] }
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

## Troubleshooting

- `dream_disabled`: dreaming is disabled in config or by the local sentinel under the runtime directory.
- `dream_unavailable`: no supported harness CLI is installed and authenticated in the daemon environment.
- `unknown harness CLI override`: the selected `--cli` is not a production harness.
- Socket errors: verify the daemon is running with `memoryd status --socket /tmp/memoryd.sock`.
