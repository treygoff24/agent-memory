# Dogfood local security/privacy review

Date: 2026-05-05
Reviewer: Codex security_auditor role
Scope: Task 6 in `docs/plans/2026-05-05-dogfood-closeout.md`, limited to dogfood-local security/privacy for the current dirty diff around installer path/PID lifecycle, daemon socket/MCP snippet, doctor diagnostic disclosure, Echo/dev-fixture gates, live eval env/skip behavior, and runbook wording.

## Verdict

No dogfood blockers found.

The current dogfood-local path is acceptable for a single-user local install. I found several dogfood risks worth fixing before relying on this on a shared workstation or before broad release, but none should block Trey's local dogfood closeout. I did not classify production-grade multi-user hardening, T17, T18, rich Recall fields, paid live eval CI, or the full release gate as dogfood blockers.

## Commands run

I did not run `scripts/check.sh` as an executable gate. I did run read-only/dry-run inspection and narrow syntax checks:

```bash
git status --short --branch
sed -n '1,260p' docs/plans/2026-05-05-dogfood-closeout.md
git diff --stat && printf '\n--- name-status ---\n' && git diff --name-status
sed -n '240,520p' docs/plans/2026-05-05-dogfood-closeout.md
git diff -- scripts/install-memorum.sh docs/runbooks/dogfooding-day-one.md docs/runbooks/eval-real-harness-ci.md docs/api/stream-h-eval-api.md crates/memoryd/src/cli.rs crates/memoryd/src/main.rs crates/memoryd/src/handlers.rs crates/memoryd/src/recall/startup.rs crates/memorum-eval/tests/live.rs crates/memorum-eval/tests/eval/domain/t13_cross_harness_substrate_sharing.rs crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs scripts/check.sh scripts/check-fast.sh scripts/check-dogfood.sh
rg -n "Echo|echo|fixture|dev-fixture|dev fixture|MEMORUM_EVAL|CLAUDE|CODEX|skip|SKIP|doctor|diagnostic|redact|secret|token|key|socket|mcp|pid|PID|runtime|install-memorum|dogfood" crates scripts docs -g '!target'
nl -ba scripts/install-memorum.sh | sed -n '1,260p'
nl -ba scripts/check-fast.sh | sed -n '1,180p'; nl -ba scripts/check-dogfood.sh | sed -n '1,220p'
git diff -- crates/memoryd/src/dream/orchestration.rs crates/memoryd/src/dream/run.rs crates/memoryd/src/dream/harness.rs crates/memoryd/Cargo.toml crates/memoryd/tests/dream_cli.rs crates/memoryd/tests/dream_harness_cli.rs crates/memoryd/tests/dream_pass_pipeline.rs
nl -ba crates/memoryd/src/dream/orchestration.rs | sed -n '1,190p'
nl -ba crates/memoryd/src/dream/harness.rs | sed -n '1,220p'
nl -ba crates/memoryd/src/dream/run.rs | sed -n '1,220p'
nl -ba crates/memorum-eval/tests/live.rs | sed -n '1,180p'
nl -ba crates/memorum-eval/tests/eval/domain/t13_cross_harness_substrate_sharing.rs | sed -n '1,140p'
nl -ba crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs | sed -n '1,240p'
nl -ba crates/memoryd/src/handlers.rs | sed -n '1400,1455p'
nl -ba crates/memoryd/src/main.rs | sed -n '48,82p'
nl -ba crates/memoryd/src/main.rs | sed -n '850,880p'
nl -ba docs/runbooks/dogfooding-day-one.md | sed -n '1,130p'
nl -ba docs/runbooks/eval-real-harness-ci.md | sed -n '1,120p'
rg -n "UnixListener|bind\(|remove_file|set_permissions|chmod|umask|socket|serve_substrate_with|serve_stdio|client::request|connect" crates/memoryd/src crates/memoryd/tests -g '!target'
nl -ba crates/memoryd/src/server.rs | sed -n '1,240p'
nl -ba crates/memoryd/src/client.rs | sed -n '1,220p'
nl -ba crates/memoryd/src/mcp_stdio.rs | sed -n '1,200p'
nl -ba crates/memoryd/src/mcp.rs | sed -n '170,255p'
rg -n "auth_probe|is_authenticated|ClaudeCodeCli|CodexCli|AuthProbe|operator_message|stderr_tail|AUTH_PROBE" crates/memoryd/src/dream -g '*.rs'
nl -ba crates/memoryd/src/dream/harness.rs | sed -n '260,585p'
rg -n "struct HarnessRunner|fn run\(|Command::new|env_clear|envs|stdout|stderr|MCP_CONFIG|copy_env|write_mcp_config_file|MEMORUM_EVAL_CLAUDE_KEY|MEMORUM_EVAL_CODEX_KEY|OPENAI_API_KEY|ANTHROPIC_API_KEY" crates/memorum-eval/src crates/memorum-eval/tests -g '*.rs'
nl -ba crates/memorum-eval/src/harness_runner.rs | sed -n '1,280p'
nl -ba crates/memorum-eval/src/orchestrator.rs | sed -n '630,705p'
bash -n scripts/install-memorum.sh scripts/check-fast.sh scripts/check-dogfood.sh && scripts/install-memorum.sh --dry-run --repo /tmp/memorum-review --runtime /tmp/memorum-review/.memoryd --socket /tmp/memoryd-review.sock
bash -n scripts/install-memorum.sh && bash -n scripts/check-fast.sh && bash -n scripts/check-dogfood.sh
tmp=$(mktemp -d); mkdir -p "$tmp/runtime"; printf '999999\n' > "$tmp/runtime/memoryd.pid"; scripts/install-memorum.sh --dry-run --repo "$tmp/repo" --runtime "$tmp/runtime" --socket "$tmp/memoryd.sock" >/tmp/memorum-security-dryrun.out; test -f "$tmp/runtime/memoryd.pid"; cat /tmp/memorum-security-dryrun.out; rm -rf "$tmp" /tmp/memorum-security-dryrun.out
ls -l docs/reviews | sed -n '1,200p'
nl -ba docs/reviews/2026-05-05-dogfood-closeout-gate-report.md | sed -n '1,260p'
```

## Findings

### F1 - dogfood risk - installer PID/log files inherit ambient umask

Files/lines:

- `scripts/install-memorum.sh:138` creates `repo` and `runtime` with plain `mkdir -p`.
- `scripts/install-memorum.sh:142` truncates/creates `memoryd.log` without setting mode.
- `scripts/install-memorum.sh:165` writes `memoryd.pid` without setting mode.
- `docs/runbooks/dogfooding-day-one.md:10-20` tells the operator where these files live and how to inspect PID/liveness.

Exploitability: On Trey's normal single-user laptop this is low. On a shared machine, a permissive umask such as `022` can make the runtime directory metadata, daemon log, and PID file group/world-readable. The PID file itself is not secret, but the log can contain local paths, daemon diagnostics, and future accidental request/error content.

Impact: Local privacy leak to other OS users on a shared workstation; possible operational reconnaissance via PID/runtime paths. This does not give remote access and is not a dogfood blocker.

Minimal remediation: Set an installer-local `umask 077` before creating runtime artifacts, or explicitly `chmod 700 "$runtime"` and `chmod 600 "$log_file" "$pid_file"` after creation. Keep this scoped to installer-owned lifecycle files.

### F2 - dogfood risk - PID lifecycle trusts the pid file before killing

Files/lines:

- `scripts/install-memorum.sh:108-135` reads `memoryd.pid`, checks `kill -0`, sends `kill`, waits briefly, then removes the pid file.
- `scripts/install-memorum.sh:188-189` and `scripts/install-memorum.sh:197-198` print copy-paste stop/restart commands that trust the PID file.
- `docs/runbooks/dogfooding-day-one.md:95-97` repeats the direct `kill "$(cat .../memoryd.pid)"` lifecycle command.

Exploitability: Low but real locally. A stale PID can be recycled, or a malformed/tampered PID file under a writable runtime can point the installer/operator at an unrelated same-user process. The current code does not validate that the PID is numeric, that the process is `memoryd`, or that its argv matches the expected `serve --repo/--runtime/--socket` tuple before signaling.

Impact: Accidental or local malicious same-user denial of service by terminating the wrong process. This is lifecycle safety, not a remote auth boundary break, so it is a dogfood risk rather than a blocker.

Minimal remediation: Before `kill`, validate the PID with a numeric regex and check `ps -p "$pid" -o command=` (or platform equivalent) for `memoryd serve` plus the expected repo/runtime/socket. If it does not match, do not kill; warn and remove only a demonstrably stale pid file. Use the same safer stop helper in the printed runbook wording.

### F3 - dogfood risk - doctor findings disclose full daemon PATH when a harness CLI is missing

Files/lines:

- `crates/memoryd/src/handlers.rs:1424-1434` adds harness auth probe failures to public `DoctorFinding` responses.
- `crates/memoryd/src/dream/harness.rs:73-83` formats missing/auth-failed diagnostics for operator display.
- `crates/memoryd/src/dream/harness.rs:457-461` uses the daemon's full `PATH` when no explicit path env is passed.
- `docs/runbooks/dogfooding-day-one.md:98-99` positions `memoryd doctor` as the health/auth check users will run and likely paste into handoffs.

Exploitability: Any local process with access to the owner-only daemon socket can request doctor output, and the human operator may paste it into docs/issues. The socket itself is owner-only after bind (`crates/memoryd/src/server.rs:151-176`), so this is not cross-user remote disclosure by default.

Impact: Full PATH can reveal usernames, private tool locations, and local environment shape. The current hardened harness redacts stderr/stdout capture diagnostics (`crates/memoryd/src/dream/harness.rs:541-543`, `708-713`), so the main residual disclosure is PATH, not provider tokens.

Minimal remediation: Remove the raw PATH from `DoctorFinding.message`, or replace it with a bounded hint such as `daemon PATH is unset` / `daemon PATH has N entries; run which <cli> in the daemon environment`. If full PATH is still useful, gate it behind an explicit verbose/debug mode that is not returned over MCP/socket by default.

### F4 - dogfood risk, but not a closeout blocker - live eval harness env isolation is broader than the test helper names imply

Files/lines:

- `crates/memorum-eval/src/harness_runner.rs:201-209` uses `Command::new(...).envs(request.env)` without `env_clear()`, so harness subprocesses inherit the whole parent environment in addition to the explicit map.
- `crates/memorum-eval/tests/eval/domain/t13_cross_harness_substrate_sharing.rs:97-114` builds one env map containing both `MEMORUM_EVAL_CLAUDE_KEY` and `MEMORUM_EVAL_CODEX_KEY`, both provider aliases, both config dirs, `HOME`, and `PATH`, then uses it for both Codex and Claude phases.
- `crates/memorum-eval/tests/eval/domain/t15_privacy_filter_refusal_retry.rs:99-115` is better scoped per harness, but still flows through the same inherited-environment runner.
- `docs/runbooks/eval-real-harness-ci.md:24-38` documents local smoke commands with real eval keys.

Exploitability: Only when a developer intentionally runs live evals with real credentials. This is explicitly outside the dogfood closeout bar for paid live eval execution, and `scripts/check-dogfood.sh:33-35` runs the live wrapper with `MEMORUM_EVAL_*` keys unset to prove skip honesty without provider calls.

Impact: A harness CLI process can see unrelated ambient secrets from the invoking shell/CI environment, and in T13 each harness phase receives the other provider's eval/provider credential material. This is not a blocker for local Memorum dogfooding because paid live eval is not required, but it should be fixed before making live evals routine.

Minimal remediation: In `HarnessRunner`, call `command.env_clear()` before applying the explicit env map. Split T13 into per-harness env maps so Codex receives only Codex/OpenAI/CODEX_HOME material and Claude receives only Claude/Anthropic/CLAUDE_CONFIG_DIR material, plus the minimal shared MCP/project/socket values.

### F5 - release-only - MCP snippet and default socket path are acceptable locally but not production-hardened

Files/lines:

- `scripts/install-memorum.sh:15` defaults to `/tmp/memoryd.sock`.
- `scripts/install-memorum.sh:173-183` prints the MCP snippet with `"command": "memoryd"` and an interpolated socket string.
- `docs/runbooks/dogfooding-day-one.md:23-27` repeats the MCP shape.
- `crates/memoryd/src/server.rs:149-176` removes a stale socket, binds, then chmods the socket owner-only.
- `crates/memoryd/src/mcp.rs:229-242` rejects admin/UI/peer payloads over MCP before socket forwarding.

Exploitability: Low for local dogfood. The daemon socket is chmodded owner-only after bind, and the MCP tool surface blocks admin/UI/peer methods. Remaining issues are mostly robustness: `/tmp/memoryd.sock` can collide with another same-user process, the bind-then-chmod sequence has a small local race window, and unusual socket paths containing quotes/newlines can make the printed JSON invalid. The `memoryd` command is PATH-resolved by the client rather than absolute.

Impact: Local confusion/DoS or malformed config, not a dogfood-local secret leak or auth bypass under the current single-user model.

Minimal remediation: For release, prefer defaulting the socket under the owner-only runtime directory, JSON-escape the printed snippet, and optionally print the absolute `command -v memoryd` path after install. Do not block dogfood on this.

## Confirmed non-findings / closed risks

- Echo/dev-fixture gate is no longer a dogfood blocker. `EchoCli` and deterministic echo helpers are compiled only under `#[cfg(any(test, feature = "dev-fixtures"))]` (`crates/memoryd/src/dream/harness.rs:183-220`, `crates/memoryd/src/dream/run.rs:47-107`), and `--cli echo` is accepted only for tests or when `dev-fixtures` is compiled and `MEMORYD_ENABLE_ECHO_DREAM_HARNESS=1` (`crates/memoryd/src/dream/orchestration.rs:98-143`). Normal no-feature builds use `UnselectedHarness` and production registry CLIs.
- The daemon socket is owner-only after bind on Unix (`crates/memoryd/src/server.rs:151-176`). This does not solve every release-hardening concern, but it is enough for dogfood-local socket confidentiality.
- `memoryd doctor` now exits non-zero when its successful doctor response is unhealthy (`crates/memoryd/src/main.rs:57-68`, `867-872`). That improves health truthfulness and does not introduce a dogfood-local auth bypass.
- The dogfood gate does not accidentally spend live provider calls: `scripts/check-dogfood.sh:33-35` unsets the Memorum eval keys before running the live wrapper, and `crates/memorum-eval/tests/live.rs:11-33` returns with explicit `MEMORUM_EVAL_SKIP:*` markers when required eval keys/CLIs are absent.

## Residual risk and confidence

Residual risk is local-machine oriented: same-user process interference, accidental disclosure through diagnostics copied into reports, and live-eval credential overexposure if someone opts into paid harness smokes. I have medium-high confidence for the requested dogfood-local scope because I inspected the dirty diff, the current line-level implementation, the relevant existing socket/MCP/harness support code, and ran syntax/dry-run checks without executing the full release gate.
