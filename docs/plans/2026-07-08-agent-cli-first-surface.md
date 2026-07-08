# Plan: CLI-first agent surface — skill + hardened CLI replaces MCP as the Tier-1 active surface

**Date:** 2026-07-08
**Owner:** Claude (Stream B)
**Status:** Reviewed — r2 incorporates the three-model review (opus plan-reviewer, Codex, Cursor)
**Branch:** `feature/agent-cli-first-surface` off `main`

## Plan revision history

- r1 (2026-07-08): initial draft.
- r2 (2026-07-08): post-review revision. Adopted: governance-refusal exit mapping (DECISION-4, the review's deepest catch — refusals ride inside daemon Success payloads); recall block commands carved out of the envelope contract; corrected DECISION-3's false "zero consumers" claim (seed-dev-substrate.sh, check-dogfood.sh, installer tests parse raw frames — migrations now owned in-plan); additive daemon error-code vocabulary (`not_found`; `substrate_error` no longer blanket-retryable) with an enumerate-and-test crosswalk rule; Task 6 rewritten around reality (interactive init already defaults to skipping MCP; the flip is the non-interactive default); Task 2 repointed at the real code seams (cli/output.rs, cli/exit.rs, per-command modules — not client.rs); reveal gate specified as CLI-local `--allow-reveal`; observe arg shape specified; dual exit-code dictionaries documented; Task 3 schema test deepened to field level; Task 7 gate split into automated + manual halves; Tasks 2/4/5 marked non-parallelizable; docs ripple expanded to a grep-driven checklist.

## Motivation

Agents drive CLIs better than MCP tools: a CLI composes with pipes and scripts, loads zero schema tokens into sessions that never touch memory, works identically across every harness with a shell (Claude Code, Codex CLI, delegate lanes), and pairs with a skill that carries judgment MCP schemas cannot express ("when is something worth a governed write," "search before writing"). The live dogfood has run since 2026-06-25 with MCP wiring torn off and lost nothing — passive recall arrives via lifecycle hooks (`memoryd recall hook`), and the MCP surface only ever served active ops.

Architecturally the change is small: `memoryd mcp` (`src/mcp_stdio.rs`) is a thin stdio forwarder to the same daemon socket the CLI client uses. The daemon is the protocol surface; MCP and CLI are peer transports. The CLI already covers 8 of the 10 MCP tools. What's missing is not functionality but **contract quality**. Evidence (live probes 2026-07-08 + review findings):

- `memoryd get <bad-id>` → error envelope on **stdout**, **exit 0** (`src/cli/output.rs:6-8` prints; `main.rs` returns `Ok(())` regardless). An agent cannot branch on failure.
- Worse: a **governance-refused write** (policy refusal, tombstone match, contradiction refusal) returns inside a daemon `Success` payload (`handlers/governance/pipeline.rs:669,749,763,785`), so even a "fixed" success→0 mapping would report "your write was refused" as ok:true, exit 0.
- A well-formed-but-missing id maps to `substrate_error` with hardcoded `retryable: true` (`handlers/error.rs:52`) — "not found" masquerades as "transient, retry me."
- Success output is the raw daemon frame `{"id":"cli-get","result":{"success":{...}}}` — transport detail leaked as public contract.
- No `schema`/`capabilities` command; exit codes undocumented outside recall/doctor.

## Scope boundary

**Frozen inputs this plan must not touch:**

- Passive recall stays at the stream-e v0.7 contract. `recall startup-block`, `recall delta-block`, and `recall hook` emit raw block output on stdout for hook consumers (`src/cli/recall.rs:57-73`) with their own pinned exit dictionary (`src/cli/exit.rs:19-28`, `tests/recall_cli.rs`) — they are **documented exceptions to the envelope contract**, not covered commands. The ambient-recall v3 redesign is a separate later arc.
- The MCP tool surface (`src/mcp.rs`, 10 tools) is demoted, not changed. The v1 freeze language in system spec §14 continues to describe the bridge.
- The daemon socket protocol **frame shape** (`src/protocol.rs`) is unchanged. The error-code **vocabulary** is free-form strings within that shape; Task 2 adds stable literals additively (`not_found`) and corrects one wrong `retryable` value — a handler-layer change (`handlers/error.rs`), not a protocol change. The MCP bridge forwards codes verbatim, so this is additive there too.
- Stream A substrate, governance decisions, privacy: untouched. (One error-classification fix in memoryd's handler error mapping; no substrate crate edits.)

## Decisions for Trey's approval-by-review

**DECISION-1 — No second binary.** The agent surface stays on `memoryd` subcommands; no `memorum` binary in this pass. The skill names the binary, so binary-name discoverability is moot; a second binary adds distribution surface and duplicate-command drift for zero contract gain. Revisit at OSS launch as branding. Cost of deferring: none.

**DECISION-2 — System spec bump to v0.3.** Init ceasing to wire MCP by default is a behavior change to a spec-defined flow (v0.2 §10, §19), and the tier model changes meaning: Tier 1 becomes "hooks + skill/CLI"; the MCP bridge becomes an **optional compatibility surface** (still shipped, still frozen at 10 tools, wired only on explicit request). Per repo conventions a behavior change requires a version bump; Trey approving this plan is the explicit direction. v0.3 also fixes v0.2's internal ten-vs-"agent-facing nine" tool-count inconsistency (§14 vs line ~540).

**DECISION-3 — New public CLI envelope; consumers migrated in-plan, no compat shim.** The CLI adopts the agent envelope (`ok`/`data`/`error`/`meta.schema_version`) and an exit-code dictionary, replacing the raw daemon frame on stdout. Known consumers of the raw frame — all in-repo, all migrated by this plan: `scripts/seed-dev-substrate.sh` (jq on `.result.success.governance_write.*`, `.result.error.message`; lines 236-311, 434), `scripts/check-dogfood.sh:148-153` (raw `status` JSON), `skills/using-memorum/SKILL.md`, `docs/agent-import-guide.md`, onboarding docs. No `--raw` flag: every consumer is ours and moves in the same branch.

**DECISION-4 — Governance write statuses map to explicit outcomes.** A governed `write`/`supersede` returning `status: Refused` (policy, tombstone, contradiction) becomes `ok:false`, **exit 65**, `error.code` = the refusal kind, `retryable:false`, `suggested_fix` naming the next move (e.g. `search` for the contradicting memory, or `supersede` instead of `write`). `Candidate` and `Quarantined` are successful submissions into the review flow — `ok:true`, exit 0 — but `data.status` is mandatory in the envelope and `meta.warnings` carries "accepted into review queue; not yet active; check `memoryd review list`." An agent must never read a refused or queued write as a completed one.

## Target CLI contract (v1)

Full contract lands as `docs/api/memoryd-cli-contract-v1.md` (Task 1); the load-bearing rules:

**Envelope.** Success: exactly one JSON object on stdout — `{"ok":true,"data":{...},"meta":{"schema_version":"1.0","warnings":[]}}`. Error: exactly one JSON object on **stderr** — `{"ok":false,"error":{"code","message","details","retryable","suggested_fix"},"meta":{...}}`. Diagnostics/tracing stay on stderr, never stdout. (The first-write banner already goes to stderr — `src/cli/output.rs:48` — compatible.)

**Exit codes — enveloped agent commands** (published dictionary, pinned by tests):

| Condition | Exit |
| --- | ---: |
| Success, including valid empty result and Candidate/Quarantined writes | 0 |
| Usage/argument error (clap) | 2 |
| Invalid input / validation or governance refusal (bad id format, malformed meta JSON, refused write) | 65 |
| Well-formed id that doesn't exist (`not_found`) | 66 |
| Internal bug / invariant violation | 70 |
| Daemon unreachable / transient failure (retryable) | 75 |
| Refused client-side gate (reveal without `--allow-reveal`) | 77 |
| Config problem detected pre-connect (bad socket path, missing repo/runtime dir) | 78 |

**Dual-dictionary rule.** The table above applies **only** to enveloped agent commands. Documented exceptions, all published in `schema` output: `doctor` keeps its linter-style dictionary (0-4); `recall *` keeps its pinned v0.7 dictionary (`src/cli/exit.rs`); admin/setup commands (`init`, `uninstall`, `export`, `peer`, `web`, `reality-check`, ...) keep their current codes (typically 1/2) until a future contract v2.

**Crosswalk discipline.** The daemon emits free-form error-code strings (~16 today, enumerable in `handlers/error.rs`). The contract publishes an explicit `daemon code → exit code` crosswalk, and the Task 2 test enumerates every code the daemon can emit — an unmapped code fails the test, so vocabulary drift is caught at the gate. Known mapping constraints: auth-class 77 is client-synthesized only (the daemon has no auth taxonomy; review-permission refusals stay 65); 78-vs-75 is decided client-side (pre-connect validation → 78; ECONNREFUSED-class → 75); `not_found` → 66 requires the Task 2 additive daemon code.

**Covered commands (contract v1).** `search`, `get`, `write`, `write-note`, `supersede`, `forget`, `source`, `reveal` (new), `observe` (new), `status`, `schema` (new). Not covered: `doctor` and `recall *` (exceptions above); admin/ops commands (contract-v2 candidates).

## Tasks

Dependency-ordered. Inner-loop gate for every task: `cargo check -p memoryd`, `cargo clippy -p memoryd --all-targets -- -D warnings`, `cargo test -p memoryd -- --test-threads=2` (leaf crate; `-p` is complete). Full `scripts/check.sh` runs once, on `main`, after integration — never mid-task.

### Task 1 — CLI contract document

Write `docs/api/memoryd-cli-contract-v1.md`: the envelope shapes, the dual-dictionary exit tables, the full daemon-code→exit crosswalk, the DECISION-4 governance-status mapping, and per-command input/output schemas with side-effect annotations (`read_only`/`mutating`/`destructive`, idempotency). This is the contract-first artifact every later task implements against and every test pins.

**Done when:** every covered command has schema + side-effect entry; the crosswalk covers every code in `handlers/error.rs`; the governance-status table matches DECISION-4; exceptions (doctor, recall, admin) are listed with their dictionaries.

### Task 2 — Envelope + exit-code layer at the real seams

The seams are `src/cli/output.rs` (`print_response`), `src/cli/exit.rs`, and the per-command modules `src/cli/memory.rs`, `src/cli/source.rs`, `src/cli/daemon.rs` — **not** `src/client.rs` (a 32-line socket helper with no printing/exit logic). Work:

1. New agent-envelope writer (serializable `AgentEnvelope` success/error types) alongside the existing `print_response`. Repoint only the covered commands' dispatch arms; `review.rs`, `quarantine.rs`, and other admin paths keep `print_response` untouched.
2. Exit mapping applied per covered command after the response lands (today `main.rs` returns `Ok(())` unconditionally): daemon error code → crosswalk exit; `GovernanceWrite`/`GovernanceSupersede` **Success payloads inspected for `status`** per DECISION-4; empty search results → exit 0 with `data.hits: []` and a broadening hint in `meta.warnings`.
3. Additive daemon error vocabulary in `handlers/error.rs`: distinguish `not_found` (retryable:false) from real `substrate_error`; stop blanket `retryable:true`. Frame shape untouched.
4. Migrate the two in-repo consumers in the same slice: `scripts/seed-dev-substrate.sh` (to `.data.*`/`.error.*`), `scripts/check-dogfood.sh` (status shape).

**Done when:** new `tests/cli_agent_envelope.rs` runs a per-command matrix — for each covered command: success, daemon-error, daemon-down, empty-result, **and refused-write / candidate-write cells for governed writes** — asserting envelope shape (deserialized via the real serde types), exit code, and output stream. Crosswalk enumeration test fails on any unmapped daemon code. Two consecutive identical `search` invocations byte-identical on stdout. Seed/dogfood scripts run green against the new shape.

### Task 3 — `schema` subcommand

`memoryd schema [all|commands|envelope|exit-codes] --json` prints the machine contract: version, covered commands with arg/flag schemas, envelope schemas, both exit dictionaries + exceptions, env vars (`MEMORUM_REPO`, `MEMORUM_SOCKET`), side-effect annotations. Generated from the same Rust types that implement the contract (serde structs + clap introspection), not a hand-maintained blob.

**Done when:** `memoryd schema --json | jq` round-trips; the test asserts **field-level** agreement — per command: required args, flags, side-effect class, exit codes — against the clap definitions (not just a command count); contract doc and schema output agree.

### Task 4 — `reveal` and `observe` subcommands

The two MCP tools with no CLI equivalent. **`reveal`** (audited unmask, Stream D): the existing gate is MCP-transport state (`mcp_stdio.rs:161-167` checks `allow_reveal`); the daemon `Reveal` handler has no gate and always audits (`handlers/memory_ops.rs:274-309`). So the CLI reimplements the gate client-side: `memoryd reveal <id> --allow-reveal`; without the flag, refuse **before any daemon request** with exit 77 and a `suggested_fix` naming the flag and the audit consequence. **`observe`** (Stream F): expose the MCP schema's required shape — `<TEXT>` positional plus required `--kind` (ValueEnum from the MCP schema's kinds); `session_id`/`harness` default from environment (documented) with explicit `--session-id`/`--harness` overrides, not the protocol's synthetic constants; document the 16KiB text bound and `ent_*` entity validation in the contract and surface them as 65-class errors.

**Done when:** both commands round-trip against a live test daemon; `reveal` refusal is tested with **no daemon socket present** (proving refusal precedes connection); `observe` bad-kind and oversize-text produce 65 with pedagogical errors; help text carries examples; `tests/cli_contract.rs` help-surface test extended with all three new subcommands (incl. `schema`).

### Task 5 — Error-pedagogy and ergonomics pass over covered commands

Sweep `search`, `get`, `write`, `write-note`, `supersede`, `forget`, `source`, `status` against the contract: every error names the bad input, why, and the exact corrective command; `--meta` JSON parse failure shows a minimal valid example; missing-id errors point at `search` as the discovery step; refused-write errors point at the DECISION-4 next moves. Output bounding documented as it actually is: `search --include-body` is the opt-in (`src/cli/mod.rs:432-434`); `get` bodies are bounded **server-side** (`handlers/memory_ops.rs:14,246`) — document the truncation and its `truncated` marker; there is no `get --include-body`. Add "did you mean" only where clap's built-ins don't already cover it. `scripts/check-doc-cli-surface.sh` (existing doc-vs-help gate) must pass.

**Done when:** each rewritten error has a regression test pinning the hint text; no covered command prints prose to stdout.

### Task 6 — Flip the non-interactive MCP-wiring default

Reality check (from review): the interactive wizard **already defaults to "None (skip MCP wiring)"** (`src/cli/init/interactive.rs:247-248`), and `--wire-mcp` already exists as a harness-target enum (`src/cli/mod.rs:116-127,164-167`). The change is the non-interactive path: `decisions_from_args` `unwrap_or(Current)` → `None` (`src/cli/init/agent.rs:87`), `SetupDecisions::default()` (`src/setup/decide.rs:25`), clap help text, and init's own help prose (`src/cli/mod.rs:78-80` still says "wire MCP configs"). Hooks wiring (`agent.rs:88`) **deliberately stays default-`current`** — Tier 1 is hooks + CLI; only MCP demotes. `uninstall` keeps cleaning legacy wiring (no change needed per review — `tests/cli_uninstall.rs` covers entry removal). `setup/mcp_wire.rs` logic unchanged.

**Done when:** `init --non-interactive` without `--wire-mcp` leaves harness config files **byte-identical** (new integration test); `init --wire-mcp current` produces today's wiring; test updates land in `tests/cli_init_agent.rs` (the ambiguous-`current` fatal test at :158-196 re-scoped), `tests/setup_end_to_end.rs` (currently always passes explicit `--wire-mcp none` — add an omitted-flag case), and the interactive epilogue unit tests; `scripts/install-memorum.sh` + `scripts/install-memorum.test.sh` no longer treat the MCP snippet as the default next step.

### Task 7 — Skill rewrite: `skills/using-memorum`

Extend the skill from import-centric to the full operating loop: orientation (`schema`, `status`, `doctor`), recall interplay (what hooks already inject, so agents don't re-search redundantly), read path (`search` → `get`), write etiquette (note vs governed write; search-before-write; when to `supersede` vs `forget`; **what Candidate/Quarantined/Refused mean and how to react**), the envelope/exit contract, and the reveal gate. Keep import as a section. Update `docs/agent-import-guide.md` exit-code references.

**Done when (two halves):** *Automated:* every command invocation quoted in the skill runs verbatim green against a test daemon, and `scripts/check-doc-cli-surface.sh` passes. *Manual artifact:* a fresh-context subagent briefed only with the skill completes the canonical loop (orient → search → get → note → governed write → supersede) against a live daemon; transcript saved under `docs/reviews/` as a review artifact, not a CI gate.

### Task 8 — Spec + docs ripple (grep-driven)

System spec v0.3 per DECISION-2 (including the ten-vs-nine tool-count fix). Then the enumerated stale-reference sweep — every item below verified or updated, with a final `grep -ri "mcp"` over docs/ and README to catch stragglers:

- `README.md` (lead still centers MCP; :12-15, :60-67, :94)
- `docs/getting-started.md:28,35,91-123`
- `docs/agent-onboarding.md` (success criteria :15-17; wiring :58-60, :107-124, :202-246; default docs :292-294, :330-338)
- `docs/mcp-wiring.md` (reframe as the opt-in compatibility path)
- `docs/troubleshooting.md:66-97`
- `docs/runbooks/agent-onboarding-smoke.md:61,78` (`wire_mcp: succeeded` is currently the manual gate — rewrite before Task 9)
- `docs/api/stream-b-daemon-mcp-api.md` (pointer to the CLI contract)
- `CLAUDE.md` current-status paragraph

**Done when:** v0.3 committed alongside v0.2 (never mutated); the sweep list is checked off; no doc on the default path tells a user or agent to wire MCP.

### Task 9 — Dogfood re-wire (live validation gate)

On the live `~/memorum` install: rebuild + redeploy the daemon, re-enable recall hooks, activate the updated skill, leave MCP unwired. Run the Task 7 canonical loop in a real session; `memoryd doctor`; confirm hooks inject startup/delta blocks; watch one dream cycle if scheduled.

**Done when:** doctor `healthy: true`; canonical-loop transcript saved under `docs/reviews/`; no regression in daemon memory footprint (spot-check `footprint`, not RSS).

### Task 10 — Integration + full gate

Fast-forward `main`, run `bash scripts/check.sh` once. Known-flaky bench-regression stage: apply the standing 3-run evidence rule before treating a cold_reindex delta as real.

## Test strategy

Extend existing infra — `tests/cli_contract.rs` (help surface + new subcommands), `tests/handler_contract.rs` / `tests/protocol_contract.rs` (prove daemon layer shape untouched; extended only for the additive `not_found` code), `tests/mcp_forward.rs` / `tests/mcp_stdio.rs` (unchanged — prove the bridge still works), new `tests/cli_agent_envelope.rs` (Task 2 matrix incl. governance-status cells). Every envelope deserialized via the real serde types, not string matching. No new frameworks; `std::process::Command` against the built binary, as existing CLI tests do.

## Risks

- **Doctor's pinned raw shape** (`tests/cli_contract.rs:80-83`) — doctor stays un-enveloped by decision; if a future pass envelopes it, that's contract v2. Stated to prevent drive-by "consistency" fixes.
- **Skill-validated-by-agent is a soft gate** — mitigated by splitting Task 7's done-when into an automated verbatim-command check plus a manual transcript artifact. The scored `agent-ergonomics` audit is the designated follow-up once the surface settles.
- **Tier-3 story weakens for shell-less harnesses.** Accepted: the bridge still ships; MCP-only harnesses can be wired manually or via `init --wire-mcp`. Named in spec v0.3.
- **Mid-branch doc/code skew**: Tasks 2-6 change behavior before Tasks 7-8 update docs/skill. Acceptable on a feature branch; nothing merges to `main` until the branch is whole.
- **Governance-status mapping (DECISION-4) touches interpretation, not governance logic** — the pipeline still decides; the CLI only renders the decision. If a new governance status appears later, the envelope layer must fail loud (unmapped-status test cell), not default to ok.

## Execution notes

- Single feature branch, commit per task (repo standing order: commits ungated).
- Ordering: 1 → 2 → {3, 4, 5} → 6 → 7 → 8 → 9 → 10. Tasks 2, 4, and 5 all edit `src/cli/mod.rs` and the per-command modules — **run them sequentially on the branch; do not delegate them as parallel lanes.** Task 6 (init/setup files) is disjoint from 2-5 and may interleave.
- Executor: Claude (Stream B owner).
