# Plan: CLI-first agent surface — skill + hardened CLI replaces MCP as the Tier-1 active surface

**Date:** 2026-07-08
**Owner:** Claude (Stream B)
**Status:** Draft — pending multi-model review
**Branch:** `feature/agent-cli-first-surface` off `main`

## Plan revision history

- r1 (2026-07-08): initial draft.

## Motivation

Agents drive CLIs better than MCP tools: a CLI composes with pipes and scripts, loads zero schema tokens into sessions that never touch memory, works identically across every harness with a shell (Claude Code, Codex CLI, delegate lanes), and pairs with a skill that carries judgment MCP schemas cannot express ("when is something worth a governed write," "search before writing"). The live dogfood has run since 2026-06-25 with MCP wiring torn off and lost nothing — passive recall arrives via lifecycle hooks (`memoryd recall hook`), and the MCP surface only ever served active ops.

Architecturally the change is small: `memoryd mcp` (`src/mcp_stdio.rs`, 291 lines) is a thin stdio forwarder to the same daemon socket the CLI client uses. The daemon is the protocol surface; MCP and CLI are peer transports. The CLI already covers 8 of the 10 MCP tools. What's missing is not functionality but **contract quality**: the CLI today dumps the raw daemon protocol envelope, exits 0 on daemon-level errors, prints errors to stdout, and has no self-describing schema surface.

Live-probe evidence (2026-07-08, daemon v0.1.0):

- `memoryd get nonexistent-id-123` → error envelope on **stdout**, **exit 0** (silent-fail class; an agent cannot branch on failure).
- Success output is the raw daemon frame `{"id":"cli-get","result":{"success":{...}}}` — transport detail leaked as public contract.
- Clap usage errors correctly exit 2; daemon errors and success both exit 0.
- No `schema`/`capabilities` command; exit codes undocumented; `--meta` takes raw JSON strings.

## Scope boundary

**Frozen inputs this plan must not touch:**

- Passive recall stays at the stream-e v0.7 contract (hooks, markers, startup/delta blocks). This plan changes only the *active* surface. The ambient-recall v3 redesign is a separate later arc.
- The MCP tool surface itself (`src/mcp.rs`, 10 tools) is not modified — it is demoted, not changed. The v1 freeze language in system spec §14 continues to describe the bridge.
- Daemon socket protocol (`src/protocol.rs`) is unchanged; all work is in the CLI client layer and setup wiring.
- Stream A substrate, governance, privacy: untouched.

## Decisions for Trey's approval-by-review

**DECISION-1 — No second binary.** The agent surface stays on `memoryd` subcommands; no `memorum` binary in this pass. Rationale: the skill names the binary, so binary-name discoverability is moot; a second binary adds distribution surface and duplicate-command drift for zero contract gain. Revisit at OSS launch as a branding question. Cost of deferring: none (a rename/alias later is mechanical).

**DECISION-2 — System spec bump to v0.3.** `init` ceasing to wire MCP configs is a behavior change to a spec-defined flow (system v0.2 §10 tiers, §19 setup), and the tier model changes meaning: Tier 1 becomes "hooks + skill/CLI", and the MCP bridge becomes an **optional compatibility surface** (still shipped, still frozen at 10 tools, wired only on explicit `--wire-mcp`). Per repo conventions a behavior change requires a version bump; Trey approving this plan is the explicit direction. The revision goal is narrow: reposition integration tiers around CLI-first, demote MCP wiring to opt-in, no other contract changes.

**DECISION-3 — New public CLI envelope, no compatibility shim.** The CLI adopts the agent envelope (`ok`/`data`/`error`/`meta.schema_version`) and an exit-code dictionary, replacing the raw daemon frame on stdout. This breaks nobody: the CLI output shape was never a ratified contract (the MCP surface was), and the only known consumers are our own docs/skill, which this plan updates. No `--raw` flag, no deprecation dance for a surface with zero external users.

## Target CLI contract (v1)

Full contract lands as `docs/api/memoryd-cli-contract-v1.md` (Task 1); the load-bearing rules:

**Envelope.** Success: exactly one JSON object on stdout — `{"ok":true,"data":{...},"meta":{"schema_version":"1.0","warnings":[]}}`. Error: exactly one JSON object on **stderr** — `{"ok":false,"error":{"code","message","details","retryable","suggested_fix"},"meta":{...}}`. Daemon error codes pass through as `error.code`; `suggested_fix` is a paste-ready command. Diagnostics/tracing stay on stderr, never stdout.

**Exit codes** (published dictionary, pinned by tests):

| Condition | Exit |
| --- | ---: |
| Success, including valid empty result | 0 |
| Usage/argument error (clap) | 2 |
| Invalid input data (bad id, malformed meta JSON, validation refusal) | 65 |
| Missing/unavailable input | 66 |
| Internal bug / invariant violation | 70 |
| Daemon unreachable / transient failure (retryable) | 75 |
| Not authorized (reveal gate, review permissions) | 77 |
| Config problem (bad socket path, missing repo) | 78 |

`doctor` keeps its existing linter-style dictionary (documented exception).

**Scope.** The agent-facing command set this contract covers: `search`, `get`, `write`, `write-note`, `supersede`, `forget`, `source`, `reveal` (new), `observe` (new), `status`, `doctor`, `schema` (new), `recall startup-block`/`delta-block`. Admin/ops commands (`serve`, `init`, `uninstall`, `dream`, `peer`, `review`, `quarantine`, `ui`, `web`, `export`, `import`, ...) keep working but are contract-v2 candidates; they are not re-enveloped in this pass except where they already route through the shared client printer.

## Tasks

Dependency-ordered. Inner-loop gate for every task: `cargo check -p memoryd`, `cargo clippy -p memoryd --all-targets -- -D warnings`, `cargo test -p memoryd -- --test-threads=2` (memoryd is a leaf crate; `-p` is complete). Full `scripts/check.sh` runs once, on `main`, after integration — never mid-task.

### Task 1 — CLI contract document

Write `docs/api/memoryd-cli-contract-v1.md` from the table above plus per-command input/output schemas and side-effect annotations (`read_only`/`mutating`/`destructive`, idempotency behavior). This is the contract-first artifact every later task implements against and every test pins.

**Done when:** doc exists; every agent-facing command has schema + side-effect entry; exit-code dictionary and envelope shapes match this plan.

### Task 2 — Envelope + exit-code layer in the CLI client

In the CLI client layer (`src/client.rs`, `src/cli/`): introduce serializable `AgentEnvelope` success/error types; route all agent-facing command output through one writer (success → stdout, error → stderr); map daemon `error.code` + client-side failures to the exit dictionary; make empty search results exit 0 with `data.hits: []` and a broadening hint in `meta.warnings`. Convert clap parse errors via `try_parse` only if it preserves standard help/version behavior — otherwise keep clap's native exit-2 handling (it is already correct).

**Done when:** the live-probe defects reproduce as failing tests first, then pass: bad-id `get` exits 65 with error JSON on stderr; daemon-down exits 75 with `retryable:true`; two consecutive identical `search` invocations are byte-identical on stdout.

### Task 3 — `schema` subcommand

`memoryd schema [all|commands|envelope|exit-codes] --json` prints the machine contract: version, agent-facing commands with arg/flag schemas, envelope schemas, exit-code dictionary, env vars (`MEMORUM_REPO`, `MEMORUM_SOCKET`), side-effect annotations. Generated from the same Rust types that implement the contract (serde structs + clap introspection), not a hand-maintained blob — drift between `schema` output and behavior must be structurally hard.

**Done when:** `memoryd schema --json | jq` round-trips; a test deserializes the full schema output; contract doc and schema output agree on command count.

### Task 4 — `reveal` and `observe` subcommands

The two MCP tools with no CLI equivalent. `reveal` is the audited unmask surface (Stream D): gate it the way the MCP bridge does (`allow_reveal`) — refuse by default with exit 77 and a `suggested_fix` naming the explicit opt-in flag; the refusal must name the audit consequence. `observe` (Stream F) forwards the existing `MemoryObserve` daemon request. Both emit the Task-2 envelope.

**Done when:** both commands round-trip against a live test daemon; `reveal` refusal path is exercised in a test (the gate must be *seen holding*); help text carries examples.

### Task 5 — Error-pedagogy and ergonomics pass over existing agent-facing verbs

Sweep `search`, `get`, `write`, `write-note`, `supersede`, `forget`, `source`, `status` against the contract: every error names the bad input, why, and the exact corrective command; `--meta` JSON parse failure shows a minimal valid example; missing-id errors point at `search` as the discovery step; bounded output defaults verified (`search --limit` exists — confirm `get` bodies are bounded and `--include-body` is the documented opt-in). Add "did you mean" only where clap's built-in suggestions don't already cover it.

**Done when:** each rewritten error has a regression test asserting the hint text survives; no agent-facing command prints prose to stdout in JSON mode.

### Task 6 — De-wire MCP from setup; demote to opt-in

`memoryd init` stops writing MCP configs by default; add `--wire-mcp` for explicit opt-in (keeps `setup/mcp_wire.rs` alive with a purpose). `uninstall` continues to clean up wiring from both old and new installs. `memoryd mcp` itself ships unchanged as the Tier-3 compatibility bridge. Update `tests/cli_init_agent.rs`, `tests/cli_uninstall.rs`, `tests/mcp_wire.rs` expectations.

**Done when:** fresh `init --non-interactive` produces no MCP config edits; `init --wire-mcp` produces today's wiring; `uninstall` removes legacy wiring in a fixture created by the old default.

### Task 7 — Skill rewrite: `skills/using-memorum`

Extend the existing repo skill from its import-centric focus to the full operating loop: orientation (`schema`, `status`, `doctor`), recall interplay (what hooks already inject, so agents don't re-search redundantly), read path (`search` → `get`), write etiquette (note vs governed write; search-before-write to avoid contradictions; when to `supersede` vs `forget`), the exit-code/envelope contract, and the reveal gate. Keep the import flow as a section, not the headline. Update `docs/agent-import-guide.md` exit-code references to contract v1.

**Done when:** a fresh-context agent given only the skill can complete the canonical loop (orient → search → get → note → governed write → supersede) against a live daemon — validated by actually running one such agent (subagent with the skill file as its only briefing).

### Task 8 — Spec + docs ripple

System spec v0.3: revision goal per DECISION-2 — Tier 1 = hooks + skill/CLI, MCP bridge = optional compatibility surface wired via `--wire-mcp`, §19 setup flow updated; §14 tool-freeze language retained for the bridge. Update `docs/api/stream-b-daemon-mcp-api.md` with the CLI contract pointer. One-paragraph note in `CLAUDE.md` current-status block.

**Done when:** v0.3 committed alongside v0.2 (never mutated); grep for "MCP" in README/onboarding docs finds no stale "point your harness at the MCP server" instructions on the default path.

### Task 9 — Dogfood re-wire (live validation gate)

On the live `~/memorum` install: rebuild + redeploy the daemon, re-enable recall hooks, activate the updated skill, leave MCP unwired. Run the canonical loop from Task 7 in a real session; run `memoryd doctor`; confirm hooks inject startup/delta blocks; watch one dream cycle if scheduled.

**Done when:** doctor `healthy: true`; canonical loop transcript saved under `docs/reviews/`; no regression in daemon memory footprint (spot-check `footprint`, not RSS).

### Task 10 — Integration + full gate

Fast-forward `main`, run `bash scripts/check.sh` once. Known-flaky bench-regression stage: apply the standing 3-run evidence rule before treating a cold_reindex delta as real.

## Test strategy

Extend existing infra — `tests/cli_contract.rs` (envelope/exit pinning), `tests/handler_contract.rs` and `tests/protocol_contract.rs` (unchanged, prove daemon layer untouched), `tests/mcp_forward.rs`/`tests/mcp_stdio.rs` (unchanged, prove the bridge still works), new `tests/cli_agent_envelope.rs` for the Task-2 matrix (per command × success/error/daemon-down/empty-result → envelope shape + exit code + stream). Every envelope deserialized via the real serde types, not string matching. No new test frameworks; `std::process::Command` against the built binary as the existing CLI tests already do.

## Risks

- **Enveloping shared printer paths leaks into admin commands** (`export`, `import` output shapes are load-bearing for other tests). Mitigation: Task 2 touches only the agent-facing dispatch arms; admin commands keep their current printer until contract v2.
- **Skill-validated-by-agent (Task 7) is a soft gate** — one agent passing ≠ ergonomic. Acceptable for v1; the `agent-ergonomics` scored audit is the designated follow-up once the surface settles.
- **Tier-3 story weakens for shell-less harnesses.** Accepted: the bridge still ships; anything MCP-only can still be wired manually. Named in spec v0.3 rather than hidden.
- **Init/uninstall matrix is the fiddliest code touched** (931-line `mcp_wire.rs`, cross-harness config formats). Mitigation: no rewiring logic changes — only the default flip and flag; existing tests already cover both harness formats.

## Execution notes

- Single feature branch, commit per task (repo standing order: commits ungated).
- Tasks 1→2→3 are strictly ordered; 4 and 5 depend on 2; 6 is independent of 2–5; 7 depends on 3–5; 8 depends on 6–7; 9 depends on all; 10 last.
- Executor: Claude (Stream B owner). If delegated, Tasks 2/4/5 are clean bounded lanes; 6–9 need coordinator judgment.
