# Passive recall hooks — "the agent just remembers" — 2026-06-19

Wire Memorum's already-shipped recall blocks into Claude Code **and** Codex via their native lifecycle hooks, so memory is injected automatically: a stable base block at session start and a prompt-relevant delta on each turn, with subagents covered too. The user experience is that the agent silently remembers the right things without anyone asking — at no extra API cost and without the recall itself mutating memory state.

Branch: `passive-recall-hooks`. Owner: Claude (autonomous), rust-engineer discipline, subagent-implemented (native Claude subagents), plan-reviewed + multi-model-reviewed before build, then re-dogfooded sandbox-first then live. **Full build**, not MVP.

## Plan revision history

- **v1** — two-block hook design, WS1/WS2/WS3, §15 governance flag.
- **v2** — folded caching-research (two-block is cache-optimal; byte-determinism required), automem-research (the import dir *is* the native auto-memory store; new WS4 import-loop hardening; don't set `autoMemoryDirectory`), plan-reviewer (3 blockers: WS1 file-ownership, fail-open reusing `exit(2)`, report-schema change), Grok (injection sanitization, determinism tuple, subagent scope, stderr hygiene).
- **v3 (this doc)** — folded Codex (gpt-5.5) review. Net-new: **(a)** recall is *not* side-effect-free — it records hits/surface-markers — so a **read-only hook recall mode** is required (Decision 10); **(b)** canonical harness id is `claude-code`, not `claude`; **(c)** the 10k-char cap is *incompatible* with the 3600-token budget (~14.4KB) → hook-specific char budget + reduced token budget; **(d)** `StartupRequest` has no subagent fields → subagent recall = parent scope, no DTO widening; **(e)** determinism is bigger than "drop session_id" — the render frame embeds session/cwd attrs + wall-clock; **(f)** add `clear` matcher, detect existing Codex `hooks.json`, prefer argv/exec form (path spaces), match a stable marker on unwire (binary path changes on upgrade), prefer `current_exe()`, and make `verify` report Codex trust state.

## Governance gates — BOTH RESOLVED 2026-06-19 (Trey approved)

1. **§15 deferral lift + read-only-recall honesty — DONE.** `stream-e-passive-recall-v0.6.md` was amended in place with a dated in-version block (no version bump, per Trey): it lifts the "automatic hook installation across all harnesses" deferral, rewrites §12.1/§12.2 for the shipped hook contract, strikes the §15 line, and codifies passive hook recall as read-only plus the base-block cache-stability contract.
2. **Report-schema bump — APPROVED.** Bump `SetupReport.schema_version` 1 → 2 alongside the new `SetupStep::WireHooks` variant — implemented in WS2 (not a standalone pre-commit, since a v2 report without the variant would be incoherent). WS2's end-to-end assertions update to expect `schema_version: 2`.

Both gates are cleared; merge to `main` is unblocked once the build + dogfood pass.

## Why this is now buildable (and was deferred before)

Stream E §12 defined the hook contract; §15 deferred the installer; `system-v0.1.md §10` sketched Claude hooks as `.sh` files and punted Codex to "TBD." Reality resolved the punt:

- **Claude Code** (verified vs binary 2.1.183 + code.claude.com/docs/en/hooks): `SessionStart`, `UserPromptSubmit`, `SubagentStart` inject via stdout **or** `hookSpecificOutput.additionalContext`. SessionStart → cache-stable prefix; UserPromptSubmit → fresh tail (`<user-prompt-submit-hook>` system-reminder). Hooks live in `settings.json` (honoring `CLAUDE_CONFIG_DIR`). 10k-char output cap; text must read as **facts, not imperatives**.
- **Codex** (verified vs developers.openai.com/codex/hooks): Claude-style hooks **GA May 2026**, deprecating `notify`. Same three events, same `additionalContext` field, injected as **developer-role context**. Config in `~/.codex/hooks.json` or inline `[hooks]` in `config.toml`. **Trust gate:** non-managed hooks are skipped until trusted via `/hooks` (records a `trusted_hash` over exact bytes).

The recall **callee** ships and is tested: `memoryd recall startup-block`/`delta-block` (`cli/recall.rs:48-74`); daemon assembles/ranks/id-sorts/budgets (`recall/startup.rs:318`, `render.rs:228`). The deferred installer is the only missing piece.

## Root-cause / grounding confirmations (verified against the codebase)

- **Setup order** (`steps.rs:33-55`): `ensure_repo → run_import → ensure_daemon → wire_mcp → verify`; regression at `steps.rs:1509`. `SetupStep` (`report.rs:90-97`) lacks `WireHooks`.
- **Wiring pattern** (`mcp_wire.rs`): pure merge helpers + injectable runtime + atomic `write_config_file_safely` (`mcp_wire.rs:659`, shared with unwire).
- **Path gotcha** (`unwire.rs:36-38` + `mcp_wire.rs:396`): Claude hooks → `$CLAUDE_CONFIG_DIR/settings.json` (else `~/.claude/settings.json`), **not** `.claude.json`. **No existing settings.json resolver honoring `CLAUDE_CONFIG_DIR`** (`import/discovery.rs:111` hardcodes) — WS2 writes a new one.
- **Fail-open hazard:** recall CLI failures route through `cli/exit.rs:9` `exit_recall_unavailable` = **`exit(2)`** and `exit_protocol_error`. The hook handler must reuse neither.
- **Recall has WRITE side effects (Codex finding):** rendering records recall hits (`render.rs:118-129`, called from `startup.rs:210`, `delta.rs:90-92`); startup writes surface markers (`startup.rs:539-566`); assembly reads wall-clock/mutable state (`startup.rs:155,269-271`). Passive hooks fire constantly → these would mutate state on every session/turn/subagent.
- **Canonical harness ids** are `claude-code` and `codex` (`mcp_wire.rs:31-38`; spec `:226-229`) — **not** `claude`.
- **`StartupRequest` shape** (`recall/types.rs:21-29`): `cwd, session_id, harness, harness_version, include_recent, since_event_id, budget_tokens` — **no `agent_id`/`agent_type`**. Default budgets 3600/400 (`types.rs:8-9`); Stream E token estimate ≈ `ceil(bytes/4)` (`spec:187-190`), so 3600 tokens ≈ **14.4KB > Claude's 10k cap**.
- **Render frame embeds session attrs** (`render.rs:170-173`) — non-deterministic across sessions as-is.
- **Native auto-memory reality (verified local):** the import dir *is* the native store. Claude auto-loads the first 200 lines/25KB of the project `MEMORY.md` every session start. Codex Memories is **enabled** here (`memories = true`), writing `~/.codex/memories/MEMORY.md` in `# Task Group:` format (parsed by `import/sources/codex.rs:74`). Import loop: `claude.rs:52` walks the dir; `compute_content_hash` (`claude.rs:171`, `codex.rs:125`) dedupes verbatim but not paraphrases — passive recall amplifies it.
- **Binary resolution:** prefer `std::env::current_exe()` (the running `memoryd`) over a PATH `which` (avoids pinning an older binary); `~`-rejecting helper at `steps.rs:709-728` doesn't handle spaces.

## Locked design decisions

1. **Unified hook handler `memoryd recall hook`, with its OWN failure path and `current_exe`-resolved binary.** Reads hook JSON on **stdin**, dispatches on `hook_event_name`, builds the request, calls the daemon under a hard deadline, emits `{"hookSpecificOutput":{"hookEventName":<event>,"additionalContext":<block>}}`. **Shares NO failure path with `StartupBlock`/`DeltaBlock`** — no `exit_recall_unavailable`, no `exit_protocol_error`; isolated `ExitCode::SUCCESS` wrapper for malformed stdin/daemon-down/timeout/invalid-cwd/oversize/empty-prompt (Blocker). Mapping: `{SessionStart, SubagentStart} → StartupRequest` (passive/read-only); `UserPromptSubmit → DeltaRequest{message=prompt}` (passive/read-only). Install-time: `<current_exe>/memoryd recall hook --socket <sock> --harness claude-code|codex`.

2. **Fail-open is absolute, including stderr.** Any failure → nothing on stdout, nothing on stderr, **`exit 0`**. Internal daemon-call deadline = named constant **`≤800ms`** (spec §12.1 ceiling `:886`), asserted; installed `timeout: 2` is a backstop.

3. **Empty delta = zero bytes.** `<memory-delta empty="true" />` (`render.rs:228`) → handler emits literally nothing. Unit-tested.

4. **Two-block cache discipline (confirmed cost-neutral-to-saving by caching-research).** Base block at SessionStart → cached prefix, read at 0.1×/90%-off every turn (~6× cheaper than re-injection). **Hard requirement: byte-deterministic, frozen per session.** This needs a **hook-mode render frame** that omits the session/cwd attrs (`render.rs:170-173`) and any wall-clock/mutable state (`startup.rs:155,269-271`) — identity tuple `(memory set, cwd, cwd's MEMORY.md-head snapshot, budget)`, never `session_id`/`harness_version`/clock. Delta at UserPromptSubmit → appended tail, only its own raw tokens (no premium). **Append-only; never mutate the prefix mid-session.**

5. **`wire_hooks` step, symmetric, idempotent for the array/trust shapes.** New `setup/hooks_wire.rs` (mirror `mcp_wire.rs`). New `SetupStep::WireHooks`, run **after `wire_mcp`, before `verify`**. Per harness:
   - **Claude** → merge a `hooks` table into `settings.json` (`CLAUDE_CONFIG_DIR`-aware, **not** `.claude.json`): `SessionStart` (matcher **`startup|resume|clear|compact`**), `UserPromptSubmit`, `SubagentStart`, each running the absolute command, **argv/exec form** to survive path spaces, `timeout: 2`. **Hooks are arrays — find-or-update by a stable marker (the `recall hook` subcommand), never blind-append and never keyed on the absolute prefix** (which changes on upgrade). Byte-identical re-merge → `AlreadyCurrent`.
   - **Codex** → **detect existing `~/.codex/hooks.json` first** (avoid the dual-representation warning); else merge inline `[hooks]` into `config.toml`. **Byte-identical re-merge** (any change rewrites the file and silently invalidates `trusted_hash`). The step message + `verify` surface the exact trust step: "configured but inactive until trusted — open Codex, run `/hooks`, trust the Memorum hook."
   - Honor `CLAUDE_CONFIG_DIR`/`CODEX_HOME`; absolute binary via the WS-shared `current_exe` constant.

6. **Decision plumbing mirrors `--wire-mcp`; WS1 owns EVERY `SetupDecisions` construction site.** `WireHooksSelection { Current, All, Claude, Codex, None }` (`decide.rs`) + `SetupDecisions.wire_hooks`; `--wire-hooks` flag + interactive prompt; default `current` non-interactively; decline → no-op. Construction sites (explicit literals, no `..Default`) that WS1 must update or it won't compile: `cli/init/agent.rs:82-91`, `cli/init/interactive.rs` (`InteractiveIo :239`, `SeededDecisions :58`, `from_args :103`, `wire_label :380`, epilogue `:479`, test `ScriptedIo :522`), `cli/init/mod.rs:82-92` (Blocker).

7. **Uninstall parity, upgrade-safe.** `setup/unwire.rs` gains `remove_memorum_hooks_json`/`remove_memorum_hooks_toml`, removing only entries matching the **stable `recall hook` marker** (not the absolute path, which drifts across upgrades). Contract test asserts **sibling non-Memorum hooks survive**. `cli/uninstall.rs` calls them alongside MCP unwire.

8. **Block-format + auto-memory coexistence (concrete).**
   - **Sanitize, don't just frame** — an actual escaping/neutralizing path so imperative *prose* inside a memory can't trip Claude's injection detector. **Hook-specific char cap < 10,000** (target ~8KB) with **a reduced startup token budget (~1800–2000 tok) for the hook channel** so the rendered block stays under the cap (3600 tok ≈ 14.4KB would spill to a file and defeat recall) — deterministic truncation if needed.
   - **Dedup recall against native auto-load, frozen at SessionStart** (reconciles with Decision 4): the daemon suppresses base-block entries already in the active project's `MEMORY.md` head (keyed off `cwd`, computed once per SessionStart request and frozen, so the block stays byte-deterministic). **Claude-only/best-effort** (Codex native recall is model-injected). Gated on a determinism test.
   - **Do NOT set `autoMemoryDirectory`.** Onboarding **detects and reports** native state only.

9. **Subagent recall = parent scope, smaller budget, no DTO widening.** `SubagentStart` → the **same cwd-scoped base block as the parent session** (no privacy widening; recall has no `agent_type` scope filter and `StartupRequest` has no subagent fields — so don't pretend to scope by subagent). `agent_type` is handler-side attribution/logging only, never sent as a new request field. Use a **smaller startup budget** for subagents (full base recall per subagent is expensive/noisy). Field presence gated on a **real `SubagentStart` stdin fixture** from the sandbox smoke; degrade to the plain block if absent.

10. **Read-only hook recall mode (new — Codex blocker).** Passive hooks fire on every session/turn/subagent, so hook recall **must not mutate substrate or ranking state**: no surface-marker writes (`startup.rs:539-566`), no recall-hit feedback that changes future ranking (`render.rs:118-129`). Add a `passive: true` (read-only) flag threaded through the `Startup`/`Delta` request → handlers → recall assembly, gating every write off. Observability-only invocation counters (`StatusResponse.recall`) may still increment if they don't feed ranking. WS3 must locate every write on the recall path and gate it.

## Invariant guards (must hold)

- **Governance:** no spec/plan bump without Trey's ask; both gates are his decision.
- **Read-only recall:** no hook invocation writes surface markers or mutates ranking/substrate. Verified by a test that asserts a passive `Startup`/`Delta` leaves the store byte-unchanged.
- **Fail-open never blocks or leaks:** no `exit 2`/nonzero, no stderr, no stall past the ≤800ms deadline. Daemon-down behaves like "no memory."
- **Cache safety:** base block byte-deterministic, frozen per session, no session_id/clock in the cached frame; injection append-only. Dogfood verifies `cache_read_input_tokens`/`cached_tokens > 0`.
- **Size:** every injected block < 10,000 chars.
- **Privacy unchanged:** inject only already-classified, recall-visible memory; `secret` never surfaced; subagent recall reuses session scope.
- **Narrow, idempotent, upgrade-safe config edits:** wire/unwire touch only our marker-matched entries; siblings preserved; Claude array find-or-update by marker; Codex byte-identical re-merge (preserves `trusted_hash`); atomic write + backup. Never write `autoMemoryDirectory`.
- **Canonical harness id `claude-code`** in the installed command.
- `scripts/check.sh` at the coordinator only; no `bench/baseline.*` writes; no `cargo generate-lockfile`.

## Shared contracts

- **Stdin → request:** `{SessionStart, SubagentStart} → StartupRequest{passive:true}`; `UserPromptSubmit → DeltaRequest{passive:true, message=prompt}`. Stdin fields: `cwd, session_id, prompt, source, agent_type, transcript_path`. `harness=claude-code|codex`, `socket` from install flags.
- **Stdout:** success → `hookSpecificOutput.additionalContext`; empty/failure → zero bytes; **always `exit 0`, never stderr**.
- **No shared exit path** with StartupBlock/DeltaBlock.
- **Determinism tuple:** `(memory set, cwd, cwd's MEMORY.md head, budget)`.
- **`passive` flag** on `StartupRequest`/`DeltaRequest` gates all recall-path writes.
- **`WireHooksSelection { Current, All, Claude, Codex, None }`** + `SetupDecisions.wire_hooks`.
- **`SetupStep::WireHooks`** (`"wire_hooks"`; schema per gate 2).
- **Stable unwire marker:** the `recall hook` subcommand string, not the absolute prefix.

## Work breakdown (file ownership)

| ID | Change | Primary files | Agent |
|----|--------|---------------|-------|
| WS1 | Hook handler (own failure path, `current_exe`) + full CLI/decision surface | `cli/recall.rs`, `cli/mod.rs` (`RecallCommand::Hook`/`HookArgs` + `--wire-hooks`), `setup/decide.rs`, **all `SetupDecisions` sites: `cli/init/agent.rs`, `interactive.rs`, `mod.rs`** | rust-engineer A |
| WS2 | `wire_hooks` step + installer + settings.json resolver + uninstall parity + trust-aware verify | `setup/hooks_wire.rs` (new), `setup/steps.rs`, `setup/report.rs` (+schema bump), `setup/unwire.rs`, `cli/uninstall.rs` | rust-engineer B |
| WS3 | Recall hardening: `passive` read-only mode + hook-mode deterministic frame + sanitization + native-MEMORY.md dedup + hook char/token budget | `recall/types.rs` (passive flag), `recall/startup.rs`, `recall/delta.rs`, `recall/render.rs`, daemon recall handler | rust-engineer C |
| WS4 | Import-loop hardening | `import/sources/claude.rs`, `import/sources/codex.rs`, `import/discovery.rs`; Codex `# Task Group:` format-version guard | rust-engineer D |

WS1↔WS2 share the pinned `decide.rs` contract; WS3 owns the `passive` flag DTO change that WS1's handler sets and WS2 is unaffected by. WS4 independent. Coordinator integrates `decide.rs` + the `passive` flag first, finishes trailing edits/tests, runs the gate once.

## Test obligations

- **WS1:** per-event stdin → request + JSON; **daemon-down → exit 0, no stdout/stderr**; empty-delta → zero bytes; oversize guarded; no shared exit-path symbol (grep-assert); `--wire-hooks` parsing; harness id is `claude-code`; all construction sites compile.
- **WS2:** Claude settings.json **marker find-or-update** (no dup on re-run) + sibling preservation + byte-identical re-merge + **settings.json-not-.claude.json under `CLAUDE_CONFIG_DIR`** + argv form; Codex `hooks.json`-collision detection + byte-identical inline re-merge (trusted_hash preserved); unwire **marker-matched** + sibling survival + empty-scaffolding drop; dry-run; `run_all` step-order regression; schema bump asserted in e2e.
- **WS3:** **passive Startup/Delta leaves store byte-unchanged** (read-only); hook-mode base block byte-identical across repeated requests on the determinism tuple and changes when it changes; no session_id/clock in frame; sanitization neutralizes imperative prose; native-MEMORY.md dedup preserves determinism; block < char cap; empty-delta sentinel.
- **WS4:** Memorum-originated content skipped on re-import; auto-memory candidates weighted below human-authored; Codex non-empty-but-zero-task-groups surfaces "format may have changed."

## Execution tail

1. **Reviews** ✓ — plan-reviewer (3 blockers), Grok (risks/nits), Codex/gpt-5.5 (6 findings) all folded into v3.
2. **Governance gates** — Trey's call on (1) §15 lift + read-only honesty and (2) schema bump.
3. **Build** WS1–WS4 via parallel rust-engineer subagents; coordinator finishes trailing edits + tests.
4. `bash scripts/check.sh` green at coordinator.
5. **Sandbox dogfood (non-destructive):** build; `init --wire-hooks` against a throwaway `--repo` + scratch `CLAUDE_CONFIG_DIR`; assert hooks land in `settings.json` (argv, absolute, marker-matched), `memoryd recall hook` returns right `additionalContext` (zero bytes on empty/daemon-down), passive mode leaves the store unchanged, Codex `[hooks]` byte-stable, capture a real `SubagentStart` fixture. Interactive smoke: fresh Claude session shows SessionStart recall + a prompt-relevant delta + `cache_read_input_tokens > 0`. Iterate until clean.
6. **Live confirmation:** back up configs, `init --wire-hooks` live, trust the Codex hook via `/hooks`, confirm the magical UX + cache hits + read-only behavior, update dogfood log + memories. Fast-forward merge to `main` (after both governance gates).
