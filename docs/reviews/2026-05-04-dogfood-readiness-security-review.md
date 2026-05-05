# Dogfood-Readiness Security Review (Adversarial)

**Reviewer:** Claude (general-purpose subagent)
**Date:** 2026-05-05
**Scope:** Touched surfaces: installer, doctor health/output, dream harness/EchoCli gate, coordination allowlist, live eval env handling.
**Verdict:** ⚠️ risks-only

## Threat Model Notes

This is a local-machine project with git-as-sync transport. The realistic attacker surface is: a malicious clone that sets env vars or injects files before the daemon runs; a concurrent local process that races the installer's PID lifecycle; and untrusted peer updates from synced repos whose harness name or recall block fields are attacker-controlled. There is no remote access surface, no auth boundary beyond local Unix socket permissions, and no multi-tenant data model. Privilege escalation means "dev-only code paths reaching a release daemon binary."

## Findings

1. **[Severity: RISK] `crates/memoryd/src/dream/orchestration.rs:139-142`** — `echo_cli_override_enabled()` returns `true` in any `dev-fixtures` debug build without checking the env var. The logic is `cfg!(debug_assertions) || (cfg!(feature="dev-fixtures") && env-var-check)`. In a debug build (the default for `cargo build --features dev-fixtures`), `debug_assertions` is true, so echo is enabled without `MEMORYD_ENABLE_ECHO_DREAM_HARNESS=1`. An attacker or misconfigured CI that compiles with `--features dev-fixtures` in debug mode gets unrestricted echo harness access; the env-var gate is only enforced in release builds with the feature. _Suggested mitigation:_ Remove the `cfg!(debug_assertions)` short-circuit branch; require the env-var check in all `dev-fixtures` builds, and reserve the bare `cfg!(test)` branch for unit-test cfgs only.

2. **[Severity: RISK] `crates/memoryd/src/dream/harness.rs:76-79` — `AuthProbeResult::CliMissing` leaks the full daemon PATH string in `DoctorFinding.message`.** The message format is `"… daemon PATH={path}"` where `path` is the daemon process's full `PATH` env string. This string is forwarded over the Unix socket and printed to any user running `memoryd doctor`. On a developer machine the PATH typically reveals installed tool directories, home directories, and sometimes secrets injected via shell profiles (e.g., a Homebrew prefix with embedded tool paths). _Suggested mitigation:_ Truncate or omit the PATH value in the public-facing `DoctorFinding.message`; the full PATH can be logged at debug level without being in the socket response.

3. **[Severity: RISK] `scripts/install-memorum.sh:106` — TOCTOU between `kill -0` liveness check and `kill` in `stop_existing_daemon`.** The script reads a PID from `$pid_file`, checks `kill -0 $existing_pid`, then immediately calls `kill $existing_pid`. If the existing daemon exits after the liveness check (naturally, or due to another process) and the OS recycles that PID before `kill` runs, the script sends SIGTERM to an unrelated process. macOS has a 32768-entry PID table that wraps relatively slowly, making this very low-probability but non-zero on a loaded machine. _Suggested mitigation:_ Use `pkill -P <parent-pid>` semantics or double-check the PID's command name before killing; alternatively document the known race and accept it as benign in this local context.

4. **[Severity: NIT] `scripts/install-memorum.sh:155` — Unquoted `$socket` in MCP JSON snippet heredoc.** The heredoc at line 155 is `cat <<SNIPPET` (unquoted), so `$socket` is expanded. If the user passes `--socket` with a value containing a double-quote or newline (e.g. a path with unusual characters), the printed JSON snippet will be syntactically invalid. This cannot be exploited for remote code execution — it only affects the user's own display output — but a user copy-pasting a malformed snippet would get a broken MCP config silently. _Suggested mitigation:_ Sanitize or `printf '%q'`-escape the socket path before inserting into the JSON snippet, or validate that the socket path matches `^[/\w.-]+$` at arg-parse time.

## Notes

**EchoCli compile-time gate — the real picture.** The review-triggering plan item (N10) said the previous state was runtime-only gating. The fix did apply `#[cfg(any(test, feature="dev-fixtures"))]` to `EchoCli` in `harness.rs` and its imports — so `EchoCli` is genuinely absent from release/no-feature binaries. That part is correctly done. The residual risk (finding #1 above) is not about `EchoCli` being present in release builds — it isn't — but about the `dev-fixtures` debug mode bypassing the env-var guard.

**Doctor PATH disclosure — already known.** The prior review (`stream-a-final-review.md`) noted PATH disclosure as an intentional local diagnostic surface. I'm recording it here as a risk rather than a nit because the DoctorFinding reaches the JSON socket response, which means any process with socket access (not just the invoking user's shell) can read the daemon's PATH. On a shared development machine this is a mild confidentiality concern. On a single-user workstation it is genuinely fine.

**Assertion-marker spoofing in live.rs.** The `MEMORUM_EVAL_ASSERTIONS=` marker is read from the nested cargo subprocess's combined stdout+stderr. A malicious test fixture that prints this string could fake a pass. This is not a realistic attack vector — the test process itself is trusted code — but it is worth noting for future multi-harness eval work where harness stdout is less controlled.

**No shell injection found.** All variables in subprocess invocations in `install-memorum.sh` are double-quoted. The `memoryd serve` invocation at line 130 correctly quotes `"$repo"`, `"$runtime"`, and `"$socket"`. No injection surface was found.

**Coordination allowlist.** `FULL_COORDINATION_HARNESSES` is a lowercase literal array; `is_full_coordination_harness()` lowercases and trims the harness field before matching. Untrusted harness names from peer updates cannot escalate to full-coordination by case manipulation. The invariant comment added in T6C is present. No bypass found.
