# Stream E — Ambient Recall (Associative Memory) v4.0

Supersedes `stream-e-ambient-recall-v3.0.md` (which stays on disk, per convention). Shipped baseline being replaced: the v0.7 passive-recall contract (`stream-e-passive-recall-v0.7.md`), currently torn off the live daemon since 2026-06-25. Parent system spec: `docs/specs/system-v0.2.md` (v0.3 pending via the agent-cli-first-surface plan).

Companion documents: the design-session glossary at `CONTEXT.md` (repo root) is the canonical vocabulary for this spec; the evidence base for every fast-model claim is `~/Code/volley` (`LOG.md`, `docs/experiments/`, `NOTES.md`) and the live context-hydrator dogfood at `~/Code/claude-space/.claude/hooks/hydrate.py`.

## Revision goal (v4.0)

v3.0's vision is retained verbatim — an agent only remembers by making a tool call; humans don't; passive recall exists to close that gap, with silence as the common correct output. What changed is the machinery, driven by two things that did not exist when v3.0 was written: a design session with a stronger model (2026-07-08 grill, recorded in `CONTEXT.md`), and **measured evidence that a superfast cloud model (Gemma 4 31B on Cerebras, ~1,100 tok/s, p95 ~1.07s over a 12K-token package) can sit inside a synchronous hook** — validated across ~12 experiments in the volley repo and a live global dogfood (hydrate.py).

The structural changes:

1. **The cue widens to the work stream.** v3.0 could only perceive user prompts. v4.0 also perceives tool activity (files touched, commands run, errors emitted) via `PostToolUse`, with rising-edge semantics — the single biggest capability change, and the one that makes the channel *associative* rather than per-turn RAG (§3.2).
2. **Association is precomputed, then judged.** A dream-compiled **trigger index** (explicit activation conditions per memory: paths, error signatures, command patterns, terms) serves high-frequency work-stream cues near-free and razor-sharp; prompt cues get trigger index + the existing fusion retrieval, then a **retina judge stage** — one fast-model call choosing among deterministic candidates — replacing v3.0's never-calibrated relevance floor as the primary prompt-cue gate (§5, §6).
3. **The retina/cortex principle** (volley's central finding) becomes a safety invariant: the fast model senses, extracts, classifies, and ranks *presented* items; it never synthesizes canonical content, never self-calibrates, never writes. **The retina proposes, never disposes** (§2 inv 8).
4. **The continuity engine is deleted.** No SessionEnd hook, no `ContinuityState` object, no substance/acceptance gate, no closeout write path. Its job is done by the **focus memory** — one pinned, sitrep-shaped, project-scoped *ordinary memory*, dream-maintained through the normal governed write path, hand-editable, its age always rendered (§8). Orientation becomes the associative channel firing on a **desk cue** — not a separate subsystem (§8).
5. **The loop closes with new dreaming jobs**: capture proposals mined from session transcripts (retina fan-out → cortex disposition), universal use-signal adjudication, grounding/staleness verification, contradiction/duplicate pre-screening, trigger compilation, and eval labeling (§9).
6. **One invariant is deliberately relaxed.** v3.0 inv 7 barred all network on synchronous recall paths. v4.0 permits **exactly one bounded, fail-open retina call on the prompt-cue path** (and nowhere else), gated by key presence, Stream D egress classification, and a daemon-enforced spend budget (§2 inv 7, §10).

Everything else — the rendering contract, byte-budgeting, entry-boundary truncation, attributed-declarative framing, external-evidence framing, uncached-tail placement, read-only hooks, fail-open desk reads, empty-wrapper silence — carries forward from v3.0 §7–§10, which got those right.

---

## 0. Vision, goals, non-goals

### 0.1 Vision

Human-like automatic associative memory for agents: if something relevant exists in memory, it is *present* in context the moment it becomes relevant — pushed, never pulled, token-efficient, and absent when nothing qualifies. The agent spends zero tool calls to receive it. Deliberate recall (`memoryd search` / `memoryd get`) remains the conscious "go dig" path.

### 0.2 Goals (measurable)

- Work-stream cues resolve in the daemon at p95 ≤ 15 ms when silent, ≤ 40 ms on a hit (deterministic only, no subprocess, no network).
- Prompt cues: deterministic candidate assembly p95 ≤ 120 ms; the optional judge call bounded at 1.5 s with hard fail-open; total hook budget respected per harness wiring.
- Silence on a clear majority of routine turns and the overwhelming majority of tool events; ≤ 3 recollections on a prompt cue, typically 1 on a work-stream cue.
- A memory associated with an exact repo artifact (a path, an error signature) surfaces on first contact with that artifact in a session — the "bench file → bench memory" test.
- Zero contentless entries; every surfaced unit is a self-contained prose lesson.
- Injected units read as declarative attributed facts; system-derived or low-confidence units are framed as external evidence, never the agent's own voice.
- Deterministic lanes are byte-stable given the same index state, cue, budget, and clock fixture; the retina lane is flight-recorded and replayable in evals (§11).
- No write on any recall hot path; telemetry is out-of-band.
- Retina spend never exceeds the configured budget; retina absence (no key, no budget, outage) degrades quality, never availability.

### 0.3 Non-goals

- Owning model inference for synthesis. Cortex work rides Stream F dreaming, which shells to the user's harness CLI.
- A second persistence layer. The focus memory, capture proposals, and trigger index live in Stream A (canonical files + index) under Stream C governance and Stream D classification.
- Replacing deliberate recall.
- Multi-tenant hardening, labeled corpora at scale, cross-device trigger-index merge — n=1 re-pricings inherited from v3.0 §16 stand.
- Harness parity. Work-stream cues are a per-harness progressive enhancement (Claude Code first); prompt cues are the portable floor.

---

## 1. Conceptual model

### 1.1 The channel and its cues

One **associative channel**, one gate stack, one rendering contract. What varies is the cue:

| Cue | Lifecycle event | Perception | Frequency | Lanes |
| --- | --- | --- | --- | --- |
| **Prompt cue** | UserPromptSubmit | the user's message + bounded recent context | ~once/turn | trigger index + fusion retrieval + judge stage |
| **Work-stream cue** | PostToolUse | tool name, file paths, command text, error output | tens–hundreds/session | trigger index only |
| **Task cue** | SubagentStart | the subagent's task brief | occasional | same as prompt cue |
| **Desk cue** | SessionStart (`startup`\|`resume`\|`clear`\|`compact`) | branch, uncommitted summary, recent commits | once/segment | pinned set + trigger index + fusion over desk text |

There are no other triggers. Orientation and post-compaction re-orientation are the desk cue; they are not subsystems (§8). SessionEnd is not wired (§Revision goal 4).

### 1.2 Data types

1. **Memories** — canonical Stream A records, unchanged.
2. **Gists** — the dream-distilled prose lesson of a memory: one self-contained declarative proposition with its consequence, bounded to a short paragraph. For short memories the gist may be the body itself; long memories *must* carry a compiled gist to be surfaceable (§7.1). Stored as derived metadata alongside the memory (not a second store).
3. **The trigger index** — per-memory compiled activation conditions (§5).
4. **The focus memory** — one pinned, project-scoped, sitrep-shaped ordinary memory (§8.2).
5. **The desk** — live repo state; context, not memory. Read fail-open at session boundaries only (v3.0 §8.0–§8.4 carry forward unchanged, including: network unconditionally off-path, local git bounded p95 ≤ 60 ms with hard fail-open, no per-turn subprocess).
6. **The surfaced set** — per-session daemon state: what has been injected, which triggers are active (rising edge), echo buffers (§4.3, §6.4).

### 1.3 The lifecycle loop

```
 work ──▶ (telemetry, out-of-band) ──▶ dreaming ──▶ next session
 cues fire, gists surface,             retina fan-out digests transcripts;
 echoes accrue                         cortex disposes: capture proposals,
                                       focus memory refresh, trigger
                                       compilation, grounding, habituation
```

The session's exhaust (surfaced sets, echoes, transcripts) is dreaming's raw material; dreaming's output (gists, triggers, focus memory, candidates) is the next session's recall quality. No hook writes anything.

---

## 2. Safety invariants

All live v0.6/v0.7 invariants carry forward: recall hooks are strictly read-only (no write *caused by* a recall hook on its synchronous path; telemetry out-of-band); only `active`/`pinned`, `passive_recall = true`, non-pending-review memories surface as facts; no encrypted plaintext in recall; governance lifecycle authoritative; tombstoned/superseded records do not teach; deterministic token estimator; typed errors.

v4.0's consolidated additions and amendments:

1. **Attributed-declarative rendering is contract** (v3.0 inv 1/4 unchanged): every unit is declarative, attributed, carries a recoverable `ref`; system-derived or low-confidence units render as external evidence ("A prior memory reports…"), never the agent's own voice.
2. **No contentless entry**; truncation cuts at unit boundaries and always emits well-formed output (v3.0 inv 5/6 unchanged).
3. **Uncached-tail placement** for per-turn and per-tool-event injection; no recall block mutates a shared static system prompt (v3.0 inv 8 / B8 unchanged, extended to PostToolUse).
4. **Desk reads are read-only, fail-open, session-boundary-only** (v3.0 inv 7 local-git half + inv 9 unchanged).
5. **Silence always emits the per-cue empty wrapper**; cold start never degrades to a recency dump (v3.0 inv 10/11 unchanged).
6. **The work-stream path is hermetic**: no subprocess, no network, no embedding-model demand. Trigger-index matching plus session-state bookkeeping only. A cold embedder never blocks any cue path — vector-lane demand degrades to FTS + triggers while the model warms in the background.
7. **The prompt-cue path may make exactly one retina call**, bounded by a 1.5 s timeout, only when (a) a Cerebras-class key is configured in Memorum config (explicit, never sniffed from ambient env), (b) the daemon spend budget has headroom, and (c) every candidate crossing the wire passes the egress gate (inv 9). Timeout/error/exhaustion falls open to the deterministic gate, silently, in-budget. No other recall path makes network calls.
8. **The retina proposes, never disposes.** Retina output is always a candidate, a flag, or a ranking over presented items, carrying provenance and machine-verified anchors (refs from the candidate set; verbatim quotes checked against source). It never authors injected prose, never writes canonical memory, never self-reports confidence as a gate signal. Canonical writes stay behind governance and the cortex dream path.
9. **The egress gate**: content leaves the machine only if its Stream D classification permits it. Sensitive/encrypt-class memory content never rides a retina call; transcript egress for capture mining requires the explicit `retina.capture.enabled` opt-in and is documented as cloud egress. A memory ineligible for egress still participates fully in every deterministic lane.
10. **Habituation is ranking-only, conservative, reversible.** No use-signal ever deletes, tombstones, or reclassifies a memory; disuse is never inferred from the mere absence of a positive signal without adjudication (inherits v3.0 B5's caution).

---

## 3. The cues

### 3.1 Prompt cue (UserPromptSubmit) and task cue (SubagentStart)

**Pipeline:** no-op pre-gate → candidate assembly → judge stage (or standout fallback) → novelty/dedup → render.

- **No-op pre-gate** (v3.0 §6.3 unchanged): rejects only bare acknowledgements, empty/whitespace prompts, and injected-wrapper noise (`<`-prefixed payloads, slash commands — the hydrate.py lesson). Every substantive prompt proceeds.
- **Candidate assembly** (deterministic): trigger-index matches on the cue text ∪ top fusion-retrieval hits (the existing v0.6 BM25 + vector RRF lane over `query_chunks`) ∪ lesson-boosted feedback memories when the cue carries a decision/difficulty signal (v3.0 §6.2's `lesson_boost`, unchanged). Capped at 15 candidates, each carried as (ref, gist, kind, confidence, structural score).
- **Judge stage** (§6): one retina call over the egress-eligible candidates.
- **Task cue** differs only in input (the subagent's task brief) and dedup scope: a subagent maintains its own surfaced set keyed by its context id — it has not "seen" the parent's injections (v3.0 §3.2 subagent scope, unchanged).

### 3.2 Work-stream cue (PostToolUse) — the new organ

**Fires on:** `PostToolUse` for file-bearing and command-bearing tools (initial matcher: `Read|Edit|Write|Grep|Glob|Bash`; config-extensible). The hook payload's tool name, file path(s), command text, and error/output tail (bounded) form the cue.

**Pipeline:** trigger-index match → rising edge → render (≤ 1–2 units). No fusion retrieval, no embedding, no judge, no subprocess (inv 6). The overwhelming majority of tool events must resolve to silence in single-digit milliseconds.

**Rising edge:** the daemon's session state tracks which triggers are active. A trigger fires when its condition *becomes* true (first touch of `bench/baseline.json` this session), not continuously while it stays true. A fired trigger re-arms only after a turn-distance threshold or topic shift (inherits v3.0 N3's re-surfacing rule).

**Harness scope and verification note:** Claude Code supports `additionalContext` injection from PostToolUse; Codex does not currently expose an equivalent — Codex stays prompt-cue-only, losing nothing it has today. **Build-time verification required** (the B8 idiom): confirm per-harness that PostToolUse-injected context lands in the conversation (uncached tail), not a cached prefix. If a harness accepts the hook but cannot inject, the daemon still runs the match and **defers delivery to the next prompt boundary** (the hit rides the next prompt cue's block, marked `deferred="true"`).

**Hook wiring:** `hooks_wire.rs` gains the PostToolUse entry (Claude family only), with a tight timeout (1 s) distinct from the prompt-cue timeout. This is the single new hook wire in v4.0.

### 3.3 Desk cue (SessionStart, all sources)

See §8. Post-compaction (`compact`) and `resume` are the same cue with the surfaced set reset — re-orientation needs no distinct machinery; the desk read is current-state-absolute (v3.0 R10 carries).

---

## 4. The gate stack

Order: candidates → **judge** (prompt/task/desk cues, when retina live) or **standout** (fallback, and the only gate v0-offline installs have) → **novelty** → **habituation weighting** → budget.

### 4.1 Judge stage

Primary gate for prompt-shaped cues. §6.

### 4.2 Standout gate (deterministic fallback)

Gates on the *shape* of the score distribution, never an absolute threshold: the top candidate must stand out from the cue's background match level (margin over the median candidate score, constants config-tuned by dogfooding, shipped permissive per v3.0 R4). A flat mediocre field yields silence regardless of raw scores. Exact trigger-index hits (path/error-signature matches) bypass the standout test — they are sharp by construction — but still pass novelty and habituation.

### 4.3 Novelty

Never surface what context already has: dedup against the session's surfaced set (per-context scope), against the native memory head (`read_native_memory_head`), and against the loaded CLAUDE.md/AGENTS.md head. Re-surfacing allowed past the turn-distance threshold (v3.0 N3 unchanged).

### 4.4 Habituation

A per-memory, cross-session damping factor fed by the use signal (§7): surfaced repeatedly without evidence of use → progressively downweighted; any positive use signal resets; damping decays over time so nothing is suppressed forever; sharp trigger hits habituate slower than fuzzy semantic hits. Stored as derived index state (not frontmatter), updated only by out-of-band telemetry and dream-time adjudication. Ranking-only (inv 10).

---

## 5. The trigger index

### 5.1 Shape

Per memory, zero or more **activation conditions**: `path` (exact), `glob`, `error_sig` (normalized error-text signature), `command` (command-pattern), `term` (distinctive lexical key). Each row: memory id, kind, pattern, provenance (`retina` | `structural`), compiled-at. Stored in the Stream A index (new tables), derived data — rebuildable from canon at any time, never synced as canonical content.

### 5.2 Compilation

A dream pass compiles triggers per memory: the retina extracts candidate conditions from the memory body (grounded per-item extraction — its validated 90%+ zone), then **every condition is machine-verified** before landing: paths/globs must syntactically parse and, for project memories, resolve against the project tree or its git history; error signatures and commands must be substrings/normalizations of text actually present in the memory. Unverifiable output is dropped, not stored (the volley firewall: point, don't conclude). A structural fallback (paths and code-fence commands lexically extracted from the body) compiles triggers when the retina is absent.

### 5.3 Staleness

Recompile on memory write/supersede (event-driven, incremental); full sweep at dream time; index version stamped so a schema bump forces rebuild. A memory with no compiled triggers is still fully reachable via fusion retrieval and deliberate recall — the index is an accelerator, not a door.

---

## 6. The judge stage

### 6.1 Contract

One call, prompt-shaped cues only, when retina is live (inv 7). Input: the cue plus the deterministic candidate set (ref, gist, kind, confidence) — **egress-eligible candidates only**; ineligible candidates skip the judge and compete via the standout gate on the same turn. Output (strict JSON, one re-roll on parse failure, ` ```json ` fences stripped): the subset of refs genuinely relevant now, ordered, each with a one-line why. Machine checks: every returned ref ∈ candidate set (else dropped); count ≤ 3.

### 6.2 The firewall

The judge **selects and orders; it never authors.** The injected body is the memory's own gist, rendered by the deterministic pipeline; the judge's "why" goes to the flight recorder only, never into context. Consequence: **given a selection, rendering is byte-stable** — the non-determinism is confined to *which* refs surface, and evals replay recorded selections (§11).

### 6.3 Failure and fallback

Timeout 1.5 s, single attempt (plus the one JSON re-roll within budget); on timeout/error/no-key/no-budget/zero-egress-eligible-candidates, the standout gate decides. The fallback is not an error state — it is the complete offline product.

### 6.4 The flight recorder

Every retina call (judge, and every §9 job) appends to a daemon-owned JSONL log: full input package, raw output, usage, latency, session id. This is the hydrate.py pattern generalized: misses are only diagnosable if we can see exactly what the retina saw, and the log is the replay corpus for evals and the join key for use-signal adjudication.

---

## 7. Rendering and the use signal

### 7.1 Rendering

v3.0 §7 carries forward wholesale as contract: the `<recall ref=… kind=… confidence=…>` prose unit; no empty elements; byte-budgeting on full rendered cost; entry-boundary truncation; per-cue empty wrapper (`<memory-recall empty="true" trigger="…" policy="stream-e-v4.0"/>`); `neutralize_imperative_prose` as contract; trust-keyed lead-ins (user-authored high-confidence → "Recalled — …"; system-synthesized or low-confidence → "A prior memory reports…"); `ref` as provenance, with go-deeper guidance naming the CLI (`memoryd get <ref>`) per the CLI-first pivot, phrased without card-catalog over-advertising. The long-memory rule is new: a memory whose body exceeds the per-unit render cap surfaces only via its compiled gist; no gist, no surfacing (and dream-time owes it one).

Per-cue budgets:

| Cue | Target | Hard cap | Units |
| --- | ---: | ---: | ---: |
| Desk (orientation) | 300 | 600 | pinned + focus + hits |
| Prompt / task | margin (often 0) | 360 | ≤ 3 |
| Work-stream | margin (usually 0) | 200 | ≤ 2 |

### 7.2 Use signal, tier 1 — same-session echo

The daemon already holds the surfaced set; subsequent cues (tool commands, file paths, next prompts) are checked for the surfaced gist's distinctive terms/paths. An echo is a weak positive, buffered in session state, flushed as an out-of-band telemetry write. Deterministic, model-free, near-free.

### 7.3 Use signal, tier 2 — dream-time adjudication

Hook payloads carry `transcript_path`; the flight recorder joins surfaced sets to transcripts by session id. A dream job has the retina adjudicate **every** session (not a sample): for each surfaced memory — engaged / acted-on / contradicted / ignored — as classification over presented transcript evidence, with verbatim supporting quotes machine-checked against the transcript. The cortex arbitrates only ambiguous cases. Output feeds habituation (§4.4) and the trust metrics (§12). Requires transcript egress opt-in (inv 9); without it, tier 1 alone feeds habituation, more slowly.

---

## 8. Orientation, the focus memory, and hydrate.py

### 8.1 Orientation = desk cue

At SessionStart the channel fires with the desk as cue: pinned memories (identity, invariants, the focus memory) always compete; trigger-index and fusion hits against the desk text (branch names, changed paths, commit subjects) join them; the gate stack and budgets apply as anywhere else. Cold start (v3.0 §6.5 unchanged): desk-only orientation if a desk exists, plus whatever pinned identity/invariants exist, else the empty wrapper — never a recency dump.

### 8.2 The focus memory

One pinned, project-scoped, sitrep-shaped ordinary memory (reserved tag `focus`): status, open loops, worries, pointers — each line carrying file-path pointers where detail lives. Written and refreshed **by dreaming**: retina fan-out digests recent session transcripts → cortex synthesizes the update → normal governed write path (dream-source confidence gating, Stream C, Stream D — no carve-outs). Hand-editable at any time; hand-authored lines are preserved across refreshes (cortex instructed; enforcement is a plan detail). Its `updated` age is always rendered at orientation; the grounding job (§9.4) flags it when the git log contradicts it. A refresh with nothing substantive to say **skips the write** — staleness is visible (age), never masked by a hollow rewrite. This deletes v3.0's B2 gate machinery: the failure mode it guarded (hollow snapshot becomes authoritative) is structurally absent when the artifact is an aging ordinary memory rather than an authoritative closeout snapshot.

### 8.3 hydrate.py supersession

The claude-space context hydrator (Layer 0 STATE.md / Layer 1 injection / Layer 2 first-prompt Gemma briefing) is the interim scaffold; **once Memorum's channel is live in a project, Memorum is the thing** (Trey ruling, 2026-07-08). Layer 0/1 are absorbed by the focus memory + orientation. Layer 2's file-pointer ranking merges into the judge stage: the first-prompt judge call may carry the file listing alongside memory candidates and return file pointers in the same briefing — one call, one injection, never two systems racing on the same hook event. Packaging (in-daemon vs. retired hook) is a plan decision.

---

## 9. Dreaming jobs (Stream F additions)

All follow one pattern — **retina fan-out digests, cortex disposes, governance arbitrates** — run under the existing dream lease and budgets, all fail-open (a night without retina is a night of cortex-only or skipped jobs, never an error).

1. **Trigger compilation** (§5.2). Retina extracts, machine-verification firewalls, structural fallback exists.
2. **Gist distillation** (§7.1). Retina proposes a gist with verbatim anchor quotes (machine-checked ⊆ body); **cortex approves/edits prose** — synthesis is cortex work (volley: mode-collapse, "mediocre analyst").
3. **Capture proposals.** Retina fans out across recent session transcripts (rate-limit-aware batching; 500K tok/min ceiling) nominating candidate memories — corrections, stated preferences, reversed decisions, lessons — each with verbatim machine-checked quotes and origin-session provenance. Cortex disposes: write as governance **Candidates** (attention, not truth; the existing review/quarantine path is the human gate), merge into existing memories, or drop. Closes the diagnosed ambient-capture gap with zero hot-path cost. Requires transcript egress opt-in.
4. **Grounding / staleness verification.** Retina checks each project memory's checkable claims against current reality (named files exist; named commands parse; git log doesn't contradict) — per-claim grounded verification, mechanically checkable outputs. Contradicted memories are flagged for cortex revision and rendered with a staleness hedge meanwhile.
5. **Contradiction & duplicate pre-screening.** Retina judges embedding-pruned memory *pairs* for conflict/duplication; output is a flag into the existing Stream C attention paths, never an action.
6. **Use-signal adjudication** (§7.3).
7. **Focus memory refresh** (§8.2) — retina digests, cortex writes.
8. **Eval labeling** (§11) — retina bulk-labels recorded cue→candidate pairs for Stream H, calibrated against a small human-labeled seed; its self-reported confidence is never used (volley: vote-margin blindness to confident wrong beliefs).

---

## 10. Retina operations

- **Model-agnostic config**: `retina.provider/model/endpoint/key` — Gemma 4 31B on Cerebras today, a config value, not an identity; smaller local siblings (12B, E4B) are a future local-lane trade (speed for privacy) with no architectural change.
- **Activation**: retina lanes exist iff the key is configured in Memorum config (explicit; a project pin beats ambient env — keys are a billing boundary). No key → deterministic product, complete and silent about what it's missing except one doctor info line.
- **Budget**: `retina.monthly_budget_usd` (default 20), enforced by the daemon with a persistent running counter; exhaustion degrades exactly as keylessness plus a doctor warning, never a hard error. Expected spend at current pricing ($2.15/$2.70 per M): judge ~1–3¢/session, transcript mining ~5–15¢/session digested, grounding/screening pennies per dream — $5–15/month typical.
- **Failure posture**: every call has a tight timeout and a deterministic fallback (judge → standout; dream jobs → structural fallback or skip). Outage degrades quality, never availability; nothing blocks a hook or a dream.
- **Observability**: spend counter, per-lane call/fallback/latency counters, and budget headroom surface in `memoryd doctor` and Stream G. All calls flight-recorded (§6.4).
- **Known serving quirks to engineer around** (volley-measured): ~1% bad JSON (single re-roll clears), ` ```json ` fences (strip), Unicode/control-char mangling on accented text (post-sanitize), `stream=false` for any tool-shaped call, max-token guard on every call.

## 11. Determinism, evals, and performance

- **Deterministic lanes byte-stable** (v0.6 §2.7 lineage): given index state, cue, budget, clock fixture — candidate assembly, standout gating, novelty, rendering are byte-identical. Desk conditioning per v3.0 §8.4 (fixture snapshots).
- **Retina lane recorded-and-replayed**: live selections vary; evals replay flight-recorder fixtures, so the full pipeline is regression-testable end-to-end. Given a recorded selection, output is byte-stable (§6.2).
- **Eval harness (Stream H)**: parser follows the v4.0 wrapper/policy strings; gains replay fixtures and the labeled-pair corpus from §9.8. The judge stage itself is A/B-able against standout-only in a worktree before merge (eval-gated merge discipline).
- **Performance budgets** (release-gate fixtures, warm, 1,000 memories): work-stream silent p95 ≤ 15 ms / hit ≤ 40 ms; prompt-cue deterministic ≤ 120 ms; desk-cue ≤ 250 ms (desk read ≤ 60 ms fail-open inside it); judge wall-clock excluded from daemon budgets (network), bounded by its own 1.5 s timeout. Observability counters extend v3.0 §9.4's set (silence rate stays the primary health metric; add per-lane retina counters and `recall.deferred_delivery_total`).

## 12. Cross-stream surfaces (additive)

- **Protocol/daemon**: a work-stream cue request variant (PostToolUse payload); the out-of-band telemetry endpoint (surfaced-set flush, echoes) — v3.0's B1 plumbing, now load-bearing; retina config/budget/counter state; session state (surfaced set, rising-edge arms, echo buffer) keyed by session id with idle expiry.
- **Hooks wiring**: PostToolUse entry (Claude family), distinct timeouts per event; prompt-cue timeout raised only when retina is enabled.
- **Stream A**: trigger-index and habituation tables (derived, rebuildable, never canonical); events `PassiveSurfaced`, `RecallUsed`, `CaptureProposed` (out-of-band writers only); reserved `focus` tag.
- **Stream C**: capture proposals enter as ordinary Candidates; contradiction/duplicate flags feed existing attention paths. No new governance machinery.
- **Stream D**: the egress gate consumes existing classification; no new surface beyond the config flags.
- **Stream F**: the §9 job list under the existing lease/pipeline.
- **Stream G**: retina spend/budget panel; silence-rate, trust (surfaced vs used), staleness-flag counts.
- **Stream H**: replay fixtures, labeled pairs, wrapper/policy regression.
- **CLI (agent-cli-first-surface)**: `memoryd doctor` retina lines; optional judge rerank on `memoryd search` (same stage, pull path); recall CLI surfaces unfreeze from the v0.7 carve-out when this spec's P1 lands.

## 13. Phased build

Each phase independently shippable, eval-gated (A/B in worktree before fast-forwarding `main`), dogfooded live before the next begins. Sequenced **after** the agent-cli-first-surface plan executes.

- **P1 — Channel core (prompt cues).** Rendering contract + gists-for-long-memories rule; surfaced set/session state; standout gate; judge stage with fail-open + flight recorder; retina config/budget/doctor plumbing; orientation-as-desk-cue (incl. desk read, cold start); echo telemetry + out-of-band endpoint; **live re-wire of the three existing hook events on ~/memorum** — dogfood starts here. Replaces the v0.7 startup/delta content wholesale; policy string `stream-e-v4.0`.
- **P2 — Association.** Trigger compilation (first new dream job, retina-assisted with structural fallback + machine verification); trigger index tables + staleness; PostToolUse wiring with rising edge and deferred delivery; work-stream cue path with its hermetic budgets.
- **P3 — The dreaming jobs.** Capture proposals; focus memory (incl. hydrate.py supersession per project); grounding; contradiction/duplicate screening; use-signal adjudication.
- **P4 — The learning layer.** Habituation over accumulated telemetry; eval labeling; judge-vs-standout A/B knob sweeps; `memoryd search` rerank.

## 14. Open questions

1. **PostToolUse injection mechanics per harness** — verified at P2 start (§3.2); deferred-delivery fallback specified either way.
2. **Transcript redaction before egress** — capture mining sends transcript content to the cloud under an opt-in; whether a redaction pass (the volley watcher's `redact.py` idiom) precedes egress, and what it strips, is decided in the P3 plan.
3. **Focus-memory hand-edit preservation** — mechanism (marker lines vs. diff3 vs. cortex instruction alone) decided in P3.
4. **Standout-gate constants** — shipped permissive, tuned by dogfooding; the labeled-pair corpus (§9.8) may later make this empirical.
5. **Judge briefing scope at first prompt** — whether the hydrate.py file-listing merge (§8.3) ships in P1 or follows as P2 polish.

## 15. Explicit deferrals

Inherited from v3.0 §16 where still applicable: daemon-cached `DeskProjection`; forced-sampling self-test; labeled should-surface corpus at scale; embedding-centroid topic drift; Stream I merge surfaces; ANN/quantization for the vector lane. New: local retina lane (small Gemma on-device); Codex work-stream cues (pending harness support); item-level trigger-index cross-device reconciliation (derived data — rebuild beats merge).

If a phase's acceptance cannot pass without a deferral, revise this spec before coding continues.
