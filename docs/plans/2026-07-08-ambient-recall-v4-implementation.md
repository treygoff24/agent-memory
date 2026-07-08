# Ambient Recall v4.0 — Implementation Plan

**Spec:** `docs/specs/stream-e-ambient-recall-v4.0.md` (supersedes v3.0). Vocabulary: `CONTEXT.md`. Evidence base for retina claims: `~/Code/volley` (LOG.md, docs/experiments/), live hydrator dogfood `~/Code/claude-space/.claude/hooks/hydrate.py`.

**Plan revision history**

- r1 (2026-07-08): initial draft, pre-review.

## Sequencing and ground rules

- **Starts after `feature/agent-cli-first-surface` merges.** That plan froze recall CLI surfaces at v0.7 as its boundary; this plan unfreezes them (spec §12). Do not interleave — both arcs edit `crates/memoryd/src/cli/` and the daemon.
- **Branch model:** one integration branch `feature/ambient-recall-v4` off `main`; each wave executes in a delegate-managed worktree off that branch and merges back after the wave gate + review. `main` advances only at phase boundaries, fast-forward, after the phase gate.
- **Commits ungated** (commit per wave, per fix, before risky changes); **pushes/PRs gated on Trey, every time**.
- **CPU discipline is a hard rule:** inner loop is `cargo check|clippy|test -p <crate>` only (mostly `-p memoryd`, `-p memory-substrate`). `bash scripts/check.sh` runs **only on integrated `main` at phase boundaries**, never in worktrees, never mid-wave. Foundational-crate edits (`memory-substrate`) get `cargo tree -i` ripple checks, full coverage lands at the phase gate.
- **Eval-gated merges:** any wave that changes ranking/gating/selection behavior runs the Stream H eval A/B **in the worktree** (old vs new on the same fixtures) before merging — the deterministic eval makes knob comparisons cheap (project memory: eval-gated merge order).
- **Live dogfood is part of the plan, not an afterthought:** P1 ends with the redeploy + hook re-wire on `~/memorum`; every later phase ships to the live daemon before the next begins.

## Execution model (orchestrator + delegation)

**Orchestrator:** a Claude Code session on Fable or Opus. The orchestrator plans waves, writes worker briefs, reviews all diffs, runs gates, commits, and never trusts a worker's "done" without verifying state on disk (global lesson: long worker runs land the meaty edits then cut off before trailing call-site updates/tests — finish leftovers at coordinator level).

**Workers via the `delegate` CLI** (load the `delegate-agent` skill at session start):

- **Codex (`delegate codex work`) is the primary implementation lane.** Brief it **goal-shaped**: state the goal, the owned files, the invariants that must hold, and the narrow gate command — let Codex plan the path. Cluster related tasks that share files or design trade-offs into ONE Codex run with rich context (full spec sections pasted in, not summarized); fan out only across genuinely disjoint file ownership.
- **Cursor (`delegate cursor work`) and Grok (`delegate grok work`)** split the Composer subscription roughly evenly: bulk/mechanical work (test scaffolds, counter plumbing, doc ripples, config threading) and the second implementation lane when two disjoint waves run concurrently. Cursor when speed matters; review their diffs for defensive noise.
- **Isolation:** every work lane runs `--isolation worktree` (with `--include-dirty` when building on uncommitted wave state). Orchestrator reviews the worktree diff **against the merge-base** (project memory: `main..HEAD` lies after sibling merges), then merges and commits. Never let two work lanes share a tree in one wave (interleaved-diff lesson, 2026-07-06).
- **Reviews are multi-model, per wave:** at each wave end, fan the wave diff to `delegate codex safe` + one Composer lane (`cursor safe` / `grok safe`, alternating) with findings-only prompts (BLOCKERS/RISKS/NITS, file:line evidence); orchestrator triages, fixes blockers before merge. **Phase boundaries add the heavier gate:** a fresh-context `plan-reviewer`-style adversarial pass over the phase's cumulative diff + spec conformance, plus the full `scripts/check.sh` on integrated `main`. If the orchestrator is Opus, phase-boundary review is a Fable gate under the standing protocol (ask first, `[[FABLE-OK]]`); if the orchestrator is Fable, it does that review natively.
- **Stuck-twice escalation** stands: two failed attempts on the same root cause → stop grinding, write the execution log, escalate (`/codex` rescue or Trey).

**Worker brief self-check (hard contract, top of every brief):** you own ONLY the files listed; run ONLY the stated narrow gate (`cargo check|clippy|test -p <crate> -- --test-threads=2`); never run workspace-wide cargo commands or `scripts/check.sh`; commit nothing (orchestrator commits); report deviations instead of improvising outside owned files.

## Code seams (verified 2026-07-08)

- Recall pipeline: `crates/memoryd/src/recall/` — `candidates.rs`, `fusion.rs`, `hybrid.rs`, `rank.rs`, `render.rs`, `budget.rs`, `dedup_state.rs`, `delta.rs`, `startup.rs`, `counters.rs`, `types.rs`, `config.rs`.
- Hook dispatch: `crates/memoryd/src/cli/recall_hook.rs`; wiring `crates/memoryd/src/setup/hooks_wire.rs` (today: SessionStart/UserPromptSubmit/SubagentStart, timeout 2).
- Daemon: `protocol.rs`, `server.rs`, `state.rs`, `handlers/`.
- Dreaming: `crates/memoryd/src/dream/` — `orchestration.rs`, `registry.rs`, `pass1-3.rs`, `prompts.rs`, `harness/`, `lease.rs`.
- Index: `crates/memory-substrate/src/index/` (sqlite; FTS5 + sqlite-vec `vec0`; `query.rs`).
- Eval: `crates/memorum-eval`.

Workers verify seams before editing; if a seam moved, report back rather than guessing.

---

## Phase P1 — Channel core (prompt cues)

Goal: the redesigned channel live on the three existing hook events, judge stage in front of deterministic candidates, dogfood running on `~/memorum`. Spec §3.1, §4, §6, §7, §8.1, §10, §11.

### Wave 1.1 — Retina substrate (no recall changes yet)

**One Codex run.** New module `crates/memoryd/src/retina/`: HTTP client (Cerebras OpenAI-compatible; `stream=false`; max-token guard; JSON re-roll + fence-strip + control-char sanitize per spec §10 quirks), key resolution (Memorum config only, project pin beats global; never ambient env), persistent spend counter + `retina.monthly_budget_usd` enforcement, flight recorder (JSONL under the daemon state dir: package, raw output, usage, latency, session id), typed errors, and `memoryd doctor` lines (key present / budget headroom / last-call health). Config surface in daemon config + docs. **Gate:** `-p memoryd`. **Done when:** unit tests cover budget exhaustion→keyless-equivalent degradation, timeout→typed error, recorder append; doctor renders all three lines; zero recall-path integration yet.

### Wave 1.2 — Session state, gate stack, telemetry endpoint

**One Codex run** (clusters: these share `state.rs` + `recall/`). Surfaced set / rising-edge arms / echo buffer as per-session daemon state with idle expiry (`state.rs` + new `recall/session.rs`); standout gate (margin-over-median, config constants, permissive defaults) in `rank.rs`/new `recall/gate.rs`; novelty extension of `dedup_state.rs` (turn-distance re-arm carried from v0.7 behavior); out-of-band telemetry endpoint (new `RequestPayload` variant + handler) recording `PassiveSurfaced`/echo flushes as Stream A events — **read paths write nothing** (spec inv, test-asserted). **Gate:** `-p memoryd`. **Eval:** A/B standout-vs-current on eval fixtures in-worktree; attach numbers to the wave review.

### Wave 1.3 — Rendering v4 + orientation as desk cue

**One Codex run; Composer lane may take the test scaffolds.** `render.rs`/`budget.rs`: v4 wrapper + `policy="stream-e-v4.0"`, gist-only rule for long memories (no gist → not surfaceable), trust-keyed lead-ins, CLI-first go-deeper guidance; `startup.rs` rework: desk cue (bounded local-git read, fail-open, §8.4-style fixture snapshots; network unconditionally off-path) through the ordinary candidate/gate/render pipeline + pinned set; cold-start fixtures (desk-or-empty, never recency). `compact`/`resume` = same path + surfaced-set reset. **Gate:** `-p memoryd`. **Eval:** startup-fixture A/B vs v0.7 renders reviewed by hand (content is deliberately different; eval asserts structure/budget/emptiness invariants, not equality).

### Wave 1.4 — Judge stage

**One Codex run.** Wire retina into the prompt-cue path (`delta.rs` + `candidates.rs`): egress-eligible candidate briefing (Stream D classification check per memory — consume existing classification, no new privacy surface), one call, 1.5 s timeout, machine-checked ref subset, ≤3, whys to flight recorder only; fallback to standout on every failure class; deferred: file-listing merge (spec open question 5 — decide at review). Deterministic rendering given a selection (byte-stability test with recorded selection fixtures). **Gate:** `-p memoryd`. **Eval:** judge-vs-standout A/B on replay fixtures; record live-call p95 against the 1.5 s budget.

### Wave 1.5 — Phase gate + live re-wire (orchestrator-owned, no delegation)

Merge `feature/ambient-recall-v4` → `main` ff after: multi-model wave reviews clean, cumulative phase review (Fable-gate rules above), `bash scripts/check.sh` green on integrated `main` (bench-regression stage known-flaky — 3-run evidence rule). Then redeploy `~/memorum` daemon, re-wire the three hook events live, start the dogfood log. **P2 does not start until the channel has ≥3 days of live silence-rate/latency telemetry.**

## Phase P2 — Association (trigger index + work-stream cues)

### Wave 2.0 — Harness verification spike (orchestrator, hours not days)

Confirm PostToolUse `additionalContext` injection lands in conversation (uncached tail) in current Claude Code; document Codex's absence. Output: a short note in `docs/dev/` + the go/no-go for deferred-delivery-only mode. (Spec §3.2 verification note.)

### Wave 2.1 — Trigger index (substrate)

**One Codex run.** `memory-substrate` index tables for activation conditions (memory id, kind ∈ path|glob|error_sig|command|term, pattern, provenance, compiled_at; derived/rebuildable, index-version stamped) + query API (match a cue's paths/command/error text → memory ids) + event-driven recompile hooks on write/supersede. Structural compiler (lexical path/command extraction) lands here so the index works retina-free. **Gate:** `-p memory-substrate` + `cargo tree -i memory-substrate` ripple check on memoryd compile.

### Wave 2.2 — Trigger compilation dream job (retina-assisted)

**One Codex run.** New dream job in `dream/registry.rs` idiom: retina extracts candidate conditions per memory; machine verification firewall (paths resolve against project tree/git history, signatures substring-check against body); unverifiable output dropped; structural fallback when retina absent; rate-limit-aware batching. **Gate:** `-p memoryd`.

### Wave 2.3 — Work-stream cue path

**One Codex run.** New protocol variant + `recall/` work-stream path: trigger match + rising edge + novelty + render (≤2 units, 200-token cap), hermetic (no subprocess/network/embedding — test-asserted); PostToolUse wiring in `hooks_wire.rs` (Claude family, matcher `Read|Edit|Write|Grep|Glob|Bash`, 1 s timeout) + `recall_hook.rs` event mapping; deferred delivery when injection unsupported (`deferred="true"` on next prompt cue). **Gate:** `-p memoryd`. **Perf fixtures:** silent p95 ≤ 15 ms / hit ≤ 40 ms at 1k memories.

### Wave 2.4 — Phase gate

As 1.5: reviews, check.sh on main, live redeploy + PostToolUse re-wire, ≥3 days telemetry (watch: per-tool-event latency, silence rate, rising-edge nag reports).

## Phase P3 — The dreaming jobs

Waves are independent dream-registry jobs — **this is the fan-out phase**: run 3.1/3.2 (Codex) and 3.3/3.4 (Composer lanes) as concurrent worktrees with disjoint file ownership inside `dream/`, orchestrator merging sequentially.

- **Wave 3.1 — Capture proposals.** Transcript discovery from flight-recorder session ids + `transcript_path`; retina fan-out nominating candidate memories with verbatim machine-checked quotes + origin-session provenance; cortex disposition prompt; writes land as governance **Candidates** through the ordinary path; `retina.capture.enabled` opt-in gate + egress documentation; redaction decision (spec open question 2) resolved here.
- **Wave 3.2 — Use-signal adjudication + habituation state.** Join surfaced sets to transcripts; retina adjudicates every session (engaged/acted/contradicted/ignored, quote-anchored); habituation table writes (ranking-only, conservative ramp, decay, trigger-hits-damp-slower); cortex arbitration for ambiguous.
- **Wave 3.3 — Focus memory job.** Retina digests recent transcripts → cortex synthesizes sitrep-shaped update → governed write with reserved `focus` tag; hollow-refresh skip; hand-edit preservation mechanism (spec open question 3) decided and tested; orientation renders age.
- **Wave 3.4 — Grounding + contradiction/duplicate screening.** Per-claim reality checks (files exist, commands parse, git-log contradiction) → staleness flags + hedged rendering; embedding-pruned pair screening → Stream C attention flags.
- **Wave 3.5 — Phase gate** + live redeploy; hydrate.py supersession begins per-project (Memorum is the thing once live — Trey ruling).

## Phase P4 — The learning layer

- **Wave 4.1 — Habituation in ranking** (consume 3.2's state in `rank.rs`; eval A/B mandatory).
- **Wave 4.2 — Eval labeling + knob sweeps.** Retina bulk-labels recorded cue→candidate pairs against a small human seed; standout constants + judge thresholds swept against the labeled set; `memorum-eval` fixtures extended with retina replay.
- **Wave 4.3 — `memoryd search` judge rerank** (pull-path reuse; CLI + skill doc ripple).
- **Wave 4.4 — Final gate:** check.sh, cumulative review, docs (api doc `docs/api/stream-e-ambient-recall-api.md`, CLAUDE.md authoritative-docs table repoint, runbook for retina ops), redeploy, close-out note.

## Risks

1. **Judge latency in the hook window.** Volley p95 1.07 s is home-network; a bad network day eats the 1.5 s budget. Mitigated by fail-open + flight-recorder latency telemetry from day one; if live p95 > 1.2 s sustained, drop the judge to first-prompt-only (hydrator precedent) before touching timeouts.
2. **PostToolUse volume surprises.** Matcher may need narrowing (Bash-only error cues) if latency or nagging shows up live; rising-edge constants are config, not code.
3. **Retina JSON/serving quirks** are engineered around in 1.1, but new quirks will appear — the recorder is the diagnostic; never debug from memory of what "should" have been sent.
4. **Transcript egress sensitivity.** Capture mining is the biggest privacy step; it ships opt-in, default off, behind its own flag, and the redaction decision is a P3 blocker, not a fast-follow.
5. **Two arcs colliding.** If the CLI plan slips, do not start this one anyway — the cli/ collision is real (sequencing rule above).
6. **Mid-branch skew:** long-lived `feature/ambient-recall-v4` drifts from `main`; rebase-merge from `main` at each phase boundary, never mid-wave.

## What NOT to do (inherited hard rules)

No workspace-wide cargo/gate commands outside phase boundaries; no `bench/baseline.*.json` writes; no spec version bumps beyond what this plan states; no pushes/PRs/tags without Trey's per-instance go-ahead; no `git add -A`; never amend/force-push/`--no-verify`; don't touch Codex-owned in-flight worktrees; `oxfmt` ignore entries for any new non-source dirs (flight-recorder fixtures, prompt files).
