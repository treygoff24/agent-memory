# Plan: durable Claude dream-auth profile + PATH fix

**Date:** 2026-06-18
**Owner:** Claude (Stream B)
**Status:** in progress
**Design input:** synthesized from two independent adversarial design reviews — `delegate codex safe` (codex-41) and `delegate cursor safe` (cursor-17) — plus on-machine ground-truthing the parent could do that the isolated delegates could not.

## Plan revision history

- v0 (2026-06-18): initial plan. Synthesizes the codex + cursor design opinions and the on-machine ground truth.
- v1 (2026-06-18): added the third root cause found during live deployment — the macOS keychain `USER` requirement (see "Deployment finding" below). Shipped as a follow-up commit on top of the profile/PATH commit.

## Deployment finding: macOS keychain needs `USER` (third root cause)

Verifying against the live launchd daemon surfaced a third, independent root cause the design phase did not anticipate. Claude's claude.ai auth token is stored in the **macOS login keychain**, not (only) in `CLAUDE_CONFIG_DIR/.credentials.json`, and the keychain lookup keys off the `USER` environment variable. Two facts compounded:

1. memoryd's hardened subprocess `env_clear()`s and forwards only an allowlist, which did not include `USER` — so even a correct `CLAUDE_CONFIG_DIR` produced `loggedIn:false`.
2. launchd user agents do **not** receive `USER` in their environment at all (confirmed via `launchctl print`: the daemon env had only PATH, CLAUDE_CONFIG_DIR, XPC_SERVICE_NAME).

So the fix needs both halves: add `USER` to `DOCUMENTED_ENV_ALLOWLIST` + `CLAUDE_ENV_ALLOWLIST` (so memoryd forwards it), and inject `USER` into both plists' `EnvironmentVariables` (so the daemon process has it to forward). Codex's auth is file-based (`CODEX_HOME`), so it does not need `USER`; the allowlist keeps `USER` out of the Codex subprocess. `USER` is public identity, not a credential, so forwarding it does not weaken the no-secret-leakage invariant. Bisected empirically: `USER` alone flips `loggedIn` to true; `LOGNAME` alone does not, so only `USER` is added.

Verified live: in the daemon's exact environment (`USER` + `CLAUDE_CONFIG_DIR=$HOME/.claude-personal` + the rendered PATH), `memoryd doctor` reports `healthy: true` with zero harness findings. Without the pin, the resolver correctly fails loud with "multiple authenticated Claude profiles found … set CLAUDE_CONFIG_DIR".

## Problem

The launchd-managed `memoryd` daemon cannot dream via Claude. Two independent root causes, only one of which was visible from the doctor message:

1. **PATH:** the daemon's launchd plist `PATH` is `/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:~/.cargo/bin`. The real `claude` binary is at `~/.local/bin/claude`, which is **not on that PATH**. So under launchd the Claude adapter returns `CliMissing` — it never even runs an auth probe. (`codex` is at `/opt/homebrew/bin/codex`, which *is* on the PATH, so dreaming silently works via Codex only.)
2. **Profile:** even when `claude` is found (e.g. an interactive `memoryd doctor`, which inherits the user's shell PATH), the daemon runs `claude auth status` with no `CLAUDE_CONFIG_DIR`, so it reads the **default** `~/.claude` config, which is logged out. The user's auth lives in profile dirs (`~/.claude-personal`, `~/.claude-work`).

Both the auth probe **and** the actual dream `complete()` call use the same hardened-subprocess environment (`harness.rs` `complete_for_adapter` / `auth_probe`, both via `MinimalEnvironment::for_adapter`). So fixing only the probe yields a green doctor with still-broken dreams — unacceptable (constraint #1 below).

## On-machine ground truth (2026-06-18)

- `CLAUDE_CONFIG_DIR` is the correct lever: `~/.claude` → `loggedIn:false`; `~/.claude-personal` → `loggedIn:true` (lawrencegoffiii@gmail.com); `~/.claude-work` → `loggedIn:true` (trey@newayfunds.com).
- **Two profiles authenticate, with different accounts.** So auto-detect cannot silently pick one — it is a billing/org/data-boundary choice.
- Auth artifact in a profile dir is `.credentials.json`. Noise dirs (`~/.claude-shared`, `~/.claude-test-empty`) lack it; `~/.claude-space` has it. So a naive `~/.claude-*` glob is wrong; candidates must be filtered and the choice must be explicit when ambiguous.
- `claude` is a shell function wrapper interactively; the real binary is `~/.local/bin/claude`. The daemon must resolve the real binary.

## Hard constraints (fail review if violated)

1. The resolved profile must feed **both** the auth probe and `complete()`.
2. No machine-specific path in synced config. `DreamsConfig` is part of synced `config.yaml` (`memory-substrate/src/config/mod.rs:40-41`, `102-156`) — confirmed by both reviewers — so a config field there is out. Use env / launchd plist / runtime-local state only.
3. Don't widen the hardened-subprocess env allowlist. Inject only `CLAUDE_CONFIG_DIR`, allowlist-filtered. `CLAUDE_CONFIG_DIR` is already in `CLAUDE_ENV_ALLOWLIST` (`harness.rs:30`).
4. Keep typed `AuthProbeResult` semantics (`Ok` / `CliMissing` / `AuthFailed` / `Timeout` / `Error`).

## Design

### Resolution policy (synthesized; Codex's conservative ambiguity stance wins)

Resolve a single `CLAUDE_CONFIG_DIR` for the Claude adapter, in precedence order:

1. **Explicit** `CLAUDE_CONFIG_DIR` in the daemon env (launchd plist or shell) → use verbatim, **fail-closed** (if it fails to authenticate, do *not* fall through to scanning — the operator chose it).
2. **Default** `~/.claude` → if it authenticates, use it (covers ordinary single-profile users with zero config).
3. **Enumerate** sibling profile dirs: existing, sorted, capped, only `~/.claude-*` under `$HOME` that contain `.credentials.json`. Probe each:
   - exactly **one** authenticates → use it (cache on the adapter instance).
   - **multiple** authenticate and no explicit override → return `AuthProbeResult::Error` with an actionable message ("multiple authenticated Claude profiles; set `CLAUDE_CONFIG_DIR` in the launchd plist"). Do **not** guess.
   - none → Claude unavailable; Codex covers dreams.

**Critical implementation note (Codex finding):** the existing `auth_probe_any_with_runner` is *terminal on normal `AuthFailed`* (it only continues on "unsupported command" markers). The profile-candidate loop must therefore live **outside** `auth_probe_any` and continue to the next dir on a normal auth failure, while reusing the per-dir command fallback (`auth status` → legacy `config get auth.user`) *within* each candidate.

### Caching

Cache the resolved dir on the `ClaudeCodeCli` instance (async `OnceCell`). Verified safe: `select_harness` → `select_first_available` probes and returns the same `Arc<dyn HarnessCli>` instance that pass1/2/3 call `.complete()` on, so probe and completion share the resolved dir within a run. Doctor builds a fresh registry per invocation, so it re-resolves each `memoryd doctor` — acceptable (infrequent). No forever-caching of negative/ambiguous results; a fresh instance re-resolves on the next run.

### PATH discovery (parent's finding; neither delegate could see it)

The adapters already accept a `path_env` override (`ClaudeCodeCli::with_path_env`) that flows into both `find_executable` and the subprocess env. Construct the adapters in `registry.rs` with an **augmented PATH** = process PATH + well-known user bin dirs (`~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`) deduped. This makes the daemon find `claude`/`codex` regardless of how it was launched, on any reasonable setup. Belt-and-suspenders: the installer also writes a PATH into the plist that includes the resolved `claude` dir.

### Doctor diagnostics

Replace the generic repair string with Claude-specific hints: default-logged-out-but-profiles-exist, multiple-authenticated-profiles (ambiguous), explicit-env-path-failed, and claude-not-found-on-daemon-PATH. Surface the resolved profile path on success.

## Files

- `crates/memoryd/src/dream/harness.rs` — profile resolver + per-dir probe loop; `MinimalEnvironment` override injection (allowlist-filtered); `ClaudeCodeCli` resolved-dir cache; PATH augmentation helper.
- `crates/memoryd/src/dream/registry.rs` — construct adapters with augmented PATH.
- `crates/memoryd/src/handlers/doctor.rs` — Claude-specific actionable hints + resolved-profile surfacing.
- `scripts/install-launchd.sh` + **both** plist templates (`com.memorum.daemon.plist.template`, `com.memorum.dream-scheduled.plist.template`) — inject `CLAUDE_CONFIG_DIR` (new `--claude-config-dir` flag, defaulting to a detected profile); ensure plist PATH includes the `claude` dir. (Codex caught that the scheduled-dream plist is a separate job that also needs this.)
- Tests: `crates/memoryd/tests/dream_harness_cli.rs` (stub `claude` reading `$CLAUDE_CONFIG_DIR`; assert probe + complete share dir; ambiguity → Error; explicit fail-closed) and `harness.rs` unit tests (resolver candidate enumeration/filtering/ordering). `scripts/install-launchd.test.sh` for the new plist rendering.

## Deferred (not needed for durability; mention as follow-ups)

- `local-device.yaml` `harness.claude_config_dir` override rung (both reviewers suggested; env/plist already gives explicit device-local control). Defer.
- Codex `CODEX_HOME` parity. Defer.
- Cache-invalidation-on-complete()-auth-failure. Defer (fresh instance re-resolves next run).

## Open decision (needs Trey)

Which account funds nightly dreaming — **personal** (lawrencegoffiii@gmail.com) or **work** (trey@newayfunds.com)? Because both authenticate, the resolver fails-closed on ambiguity, so Trey's machine *requires* an explicit `CLAUDE_CONFIG_DIR` in the plist. Default to **personal** (his shell wrapper's default when `CC_DEFAULT_PROFILE` is unset); confirm and make switching a one-line plist edit + reinstall.

## Gate

Narrow during dev: `cargo test -p memoryd --tests`. Trunk gate after merge: `bash scripts/check.sh`. Then re-render + reinstall the plist, restart the daemon, and confirm `memoryd doctor` reports `claude CLI: ✓ authenticated` with the resolved profile.
