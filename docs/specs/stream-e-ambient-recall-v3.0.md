# Stream E — Ambient Recall (Passive Memory Redesign) v3.0

**Status:** Draft for review. Not yet an accepted implementation contract. This document proposes a ground-up redesign of the Stream E passive-recall surface and supersedes the *approach* of the live contract `stream-e-passive-recall-v0.6.md` while reusing much of its machinery. On acceptance, the Authoritative-documents table in `CLAUDE.md` and the `STREAM_E_POLICY` version string should be repointed here; this draft does not mutate either.

**Date:** 2026-06-22.

**Authors:** Claude, from a design session with Trey.

**Sources:** the live Stream E contract (`stream-e-passive-recall-v0.6.md`, including its 2026-06-19 read-only-hooks amendment), the shipped Stream A–I surfaces, the conceptual walkthrough at `docs/explainers/2026-06-22-ideal-agent-memory-hooks.html`, the cap-recent-memory fix (`500a60d`), two ground-truth code recon passes (2026-06-22), the cross-vendor fusion review at `docs/reviews/2026-06-22-ambient-recall-v1-fusion.md`, and its companion Codex verification at `docs/reviews/2026-06-22-ambient-recall-v1.1-codex-verification.md`.

**Lineage note.** The ambient-recall redesign drafts are v1.0 → v1.1 → **v3.0** (this file). The *deployed* passive-recall contract is the separate v0.1 → … → **v0.6** line; v3.0 is the proposed successor to deployed v0.6. The redesign drafts v1.0/v1.1 never shipped (draft-for-review), so no `stream-e-v1.x`/`stream-e-v2.x` policy string was ever deployed. On acceptance the policy/manifest/recall-block version string bumps from the live `stream-e-v0.6` to **`stream-e-v3.0`**.

---

## Revision goal (v3.0)

v3.0 re-sequences v1.1 for the system's actual operating condition: **a single user (Trey) who is simultaneously the developer, the only dogfooder, and the evaluator.** v1.1 folded in a cross-vendor fusion review whose corrections were sound — but that review reasoned, correctly for its own framing, as a *staff shipping a multi-tenant product*. A large share of v1.1's added weight is multi-user / adversarial / can't-observe-the-system hardening that is **mispriced for n=1.** v3.0 keeps every correction that is about *correctness, elegance, or the single user's own cost*, and re-prices the ones that are about *scale, adversaries, or unobservability*.

The decisions cleave cleanly.

**Survives n=1 unchanged (kept verbatim from v1.1):**

- **Out-of-band telemetry (B1).** Clean architecture + prompt-cache stability + testability — right regardless of user count. *And it re-affirms the live contract:* v0.6's 2026-06-19 amendment already mandates read-only passive hooks ("no surface-marker writes, no recall-hit ranking feedback"). v1.0's recall-time `PassiveSurfaced`/`RecallUsed` writes had silently regressed that shipped invariant; v3.0 restores it (§2 inv 4, §5.3).
- **Bottom-of-context placement (B8).** The single user's own per-turn token bill, and the Codex pass confirmed both harnesses already do it. This too re-affirms v0.6, whose amendment already requires the per-turn delta "at the uncached tail." It is a documentary invariant, not new machinery (§9.5).
- **The PreCompact correction.** v1.0's T2 was *unbuildable* — PreCompact is block-only with no `additionalContext` surface in either harness. T2 rides `SessionStart` source `compact`/`resume` instead, at zero new hook wire. User-count-independent (§3.3, §14.1).
- **The entire rendering/budget/truncation set.** Not hardening — just correct. The original noise dump wasted the user's attention (§7, Phase 1).
- **Secret-never-on-disk, network-off-the-hot-path, external-evidence framing (R9).** All cheap, all about protecting the one real user from being misled or leaked, all kept.

**Re-priced for n=1 (relaxed or deferred):**

- **B3 (item-level cross-device merge)** drops from hard ship-blocker to a *do-it-if-it-bites* enhancement. A merge graveyard needs *concurrent* closeouts writing the same continuity object in the same instant; one human driving one terminal at a time makes this near-zero, and the failure (a rare last-writer-wins clobber of an open-loop) is noticed and re-added in seconds. Ship whole-object supersede; the merge driver still guarantees convergence (§4.2, §16).
- **B7 (governance carve-out)** is resolved by *removing* the carve-out, not by adding governance machinery. With no adversary, a wrong synthesized continuity claim is a quality bug, not a breach. Routing continuity writes through the normal governed write path (no special case) is the *simpler* option, and R9 framing already supplies the system-vs-user surface distinction the council asked for (§4.4, §5.2).
- **B6 self-test sampling and R4 labeled-set calibration** are dropped: the user *is* the labeled set. Keep the cheap miss-signal (a genuinely useful real-time feedback loop), set the relevance floor by feel, and skip the forced-sampling harness and the hand-labeled corpus (§6.3, §6.4).
- **B4's full background `DeskProjection`** is deferred to an optimization. Desk is read at session boundaries only (T0/T2), on the user's own normal-size repos, under a tight fail-open deadline — a synchronous local `git` read there is acceptable. The network half (`gh pr view`) still moves off the hot path. The in-memory projection + file-watch removes even the local cost, but it optimizes a stall the single user probably won't feel; build it if a session start ever hangs (§8).

**The headline change — the continuity engine ships, and ships early.** v1.1 deferred the continuity engine (the maintained `ContinuityState` object, the closeout hook, dream-time maintenance) to a final Phase 4 gated behind five hard prerequisites *and* a telemetry-proven adoption signal. That deferral tangled two motives: *"prove it's worth building"* and *"it's unsafe — it'll confidently mislead the user."* At n=1 these separate cleanly:

- The first motive **evaporates.** The user is a faster, higher-signal adoption oracle than any eval metric. The "telemetry proves adoption before we build the cathedral" gate is deleted and replaced by the user's own judgment after dogfooding.
- The second motive **persists and sharpens.** Confident-wrongness costs the single real user attention and misleads real work (Próspera, policy, code) — so the one prerequisite that protects against it survives: **B2, the substance/acceptance gate** (never render a hollow or degraded continuity state as authoritative; frame synthesized claims as external evidence).

Of v1.1's five §4.0 prerequisites: **B1** ships in Phase 1 regardless; **B3** and **B7** are re-priced away; **R5** lands as a lite desk-contradiction check with the desk read; and **B2** is the lone remaining hard gate. So the continuity engine — the *remembered-not-retrieved* inversion that was the entire point of the redesign — moves from a deferred cathedral to **Phase 2**, right after the noise fix. This is the **Option-B (continuity-first)** position from the fusion review (Grok/DeepSeek), and it is acceptable *precisely because* n=1 removes the scale and adversarial risks that made the council prefer Option A.

**Build order (re-sequenced):** Phase 1 rendering/safety → **Phase 2 minimal continuity engine** → Phase 3 relevance/gating → Phase 4 desk & re-entry (+ lite continuity-claim invalidation). Each correction stays tagged to its fusion-finding id (B1–B8, R2–R10, N1–N7) for traceability. Where prose still says "v1.0"/"v1.1," read it as "this redesign" unless a v3.0 note overrides it.

---

## 0. Preamble — how this came up, and what we're building

### 0.1 How this came up

Opening Codex in a repo injected a `<memory-recall>` block that was forty `<recent-memory>` entries deep — heading-fragments of three or four imported documents (`reference-ingest — Tables that matter`, `— Tables that don't`, `— Drop reasons`), each with an opaque 35-character `ref`, a microsecond-precision timestamp, an empty `<snippet></snippet>`, and a flat `confidence="0.70"` — ranked by recency, then cut off mid-entry by the char-cap backstop. It was noise. Worse than noise: it cost attention to read and trained the agent to ignore the whole channel.

The immediate fix (`500a60d`) capped the passive section to eight entries and dropped empty snippet tags. That stopped the bleeding but treated the symptom. Stepping back to the conceptual purpose surfaced the real problem, and a blank-canvas redesign. The conceptual walkthrough is at `docs/explainers/2026-06-22-ideal-agent-memory-hooks.html`; this spec is its implementation contract.

### 0.2 The problem in one sentence

The current system answers **"what was recent?"** at session start — the moment of *minimum* information, before the agent knows what it's doing. The job of passive recall is to answer **"what's relevant *now?*"** continuously, with a bar high enough that the agent learns to trust it.

The deeper framing: an agent only remembers by making a tool call. Humans don't — the relevant memory is simply *present* the moment it becomes relevant, cue-triggered and ambient. Passive recall exists to close that gap. The failure mode of the naive version is that it is a database `SELECT ... ORDER BY recency` pinned to the top of context, which can only be *recent*, never *relevant*.

### 0.3 The governing analogy

Opening a repo is walking into your office in the morning. You arrive carrying a **baseline** — where the project's at, its purpose, what you were last on. Then, *while you work*, things surface passively: a word, an error, a decision pulls up the relevant past, unbidden. Two different mechanisms, two different triggers.

Two consequences fall out of taking the analogy seriously:

1. **The morning state is *remembered*, not retrieved.** You don't re-derive the project by querying your whole memory and sorting by date — you pick up a state that *persisted from yesterday*, lightly decayed. So the startup block should not be a ranked query over atoms. **The end of one session should write the beginning of the next.** Closeout is the moment of maximum information; that is when "where am I" gets snapshotted. Startup reads it back, refreshed against what changed.

2. **The repo is the one part of the office that survives the night with full fidelity.** Your desk literally still has yesterday's work on it. For an agent, memory gets wiped between sessions but the *repo does not* — so live repo state (branch, uncommitted work, recent commits, open PR, CI) is the most trustworthy input in the room, and orientation should anchor on it and let memory annotate it.

And the place the analogy *breaks*, which is the most important part: a human walks in with a faded yesterday and can reconstruct from a continuous underlying memory; an agent walks in with **nothing**. If the block is noise, the agent isn't under-oriented, it's amnesiac. So the externalized state has to carry *more* load than a human's morning brain, not less. **This is the core reason v3.0 builds the continuity engine early rather than deferring it: for an amnesiac, the maintained morning state is not a nice-to-have, it is the mechanism.**

### 0.4 Vision and objectives

Passive recall should feel like continuity — the thing a good colleague has walking back in Monday morning ("right, we were on X, Trey cares about Y, last time Z bit us"), then the specifics arriving exactly as they become relevant.

Concrete objectives:

- **O1 — Relevance over recency.** Most recall fires on what matters *now*, not on the clock. This bundles two distinct relevances with different signals and failure modes, and v3.0 keeps them separate *(N7)*: **(a) cue-relevance** — T1/T2 surface on a semantic/lexical match to the live cue (the user's message and the work in front of the agent); **(b) project-salience** — T0 orients by stakes/consequence ordering over the continuity model and a small set of pinned project memories, with no user cue to match. Ranking gains a cue-relevance term it currently lacks entirely (§6.2); orientation ordering is salience-by-stakes (§3.1).
- **O2 — Conclusions in the body; pointer in the attribute for traceability.** *(N1)* The injected unit is a self-contained recollection — the *lesson*, in prose — that the agent can act on without dereferencing anything. It also carries a `ref` attribute purely for provenance/traceability (an invariant requirement; the Stream H parser needs it), not as a handle the agent is expected to fetch. The `ref` is an affordance for "go deeper," never a substitute for a usable body; the surface must not advertise `memory_get <ref>` so insistently that it trains card-catalog (fetch-to-understand) behavior.
- **O3 — Gate, don't truncate.** A high relevance bar decides what surfaces; silence is a valid and frequent output. Token efficiency is a *consequence* of the bar, never the optimization target.
- **O4 — Continuity is maintained, not queried.** A continuity-state object written at closeout, refined at dream-time, and read back at orientation is the spine (§4–§5). **v3.0 ships it in Phase 2**, gated only by the B2 substance/acceptance gate (§4.0) — not deferred. This is the difference between "remembered" and "merely recent."
- **O5 — Trust is the currency.** Precision (few items, all relevant) earns the agent's trust so it leans on the channel; volume destroys it. A channel the agent distrusts is worse than no channel. v3.0 weights this objective heavily: a *confidently wrong* recollection costs more trust than a noisy-but-ignorable one — which at n=1 (the single user betting real work on the channel) is the reason the continuity engine ships behind the B2 acceptance gate even though every *other* continuity prerequisite is re-priced away.
- **O6 — Safety preserved.** Read-only (strictly — no write is *caused by* a recall hook; telemetry is out-of-band, §2 inv 4/§5.3), no encrypted plaintext, governance-authoritative, deterministic and cache-stable — every live v0.6 safety invariant carries forward, plus new injection-safety invariants for an inherently larger prompt-injection surface.

### 0.5 Goals (measurable)

- Median orientation (T0) block ≤ 300 estimated tokens; hard cap ≤ 600. Never truncates mid-entry.
- Per-turn (T1) recall surfaces nothing on a clear majority of routine turns (silence is the common case); when it surfaces, ≤ 3 recollections by default.
- Zero contentless entries: a recollection with no usable body is not surfaced.
- Injected recollections read as declarative attributed facts, never as imperatives; system-derived or low-confidence recollections are framed as *external* evidence ("A prior memory reports…"), not the agent's own voice *(R9)*.
- Output is byte-stable given the same desk state, request context, clock fixture, and continuity-state version (carries forward v0.6 §2.7, extended §9.1).
- No write is performed on a recall hot path: T0/T1/T2 are read-only; `PassiveSurfaced`/`RecallUsed`/note-consume writes happen out-of-band *(B1; re-affirms v0.6's read-only-hooks amendment)*.
- The continuity state never renders as authoritative when hollow or degraded *(B2)*.

### 0.6 Non-goals

- Owning model inference. Synthesis reuses the Stream F dream pipeline, which shells to the user's own harness CLI.
- A second persistence layer. The continuity-state object is a canonical Stream A memory, governed by Stream C and classified by Stream D — not a private store.
- Replacing deliberate recall. `memory_search` / `memory_get` remain the conscious "go dig" path; this spec makes them *discoverable*, not redundant.
- Cross-harness session-transcript capture. Closeout consumes a bounded, agent- or harness-supplied summary, not a full transcript.
- **Multi-tenant / concurrent-writer hardening.** Item-level cross-device merge, governance carve-out machinery, labeled eval corpora, and forced-sampling self-test are explicitly *not* goals for the n=1 build; they are listed as re-priced enhancements (§16) to revisit only if real usage warrants.

### 0.7 What this keeps, and what it changes

**Keeps (reuses existing machinery):** the `recall hook` dispatch handler and three wired lifecycle events; the per-turn delta path (`build_delta_response`); the v0.6 vector RRF-fusion retrieval lane (the `query_chunks` vector + bm25 FTS fusion the cue-relevance term reuses); project/session binding and namespace resolution (v0.6 §4); candidate collection over the indexed `MemoryQuery` extension (v0.6 §6); the deterministic structural ranking core (v0.6 §8.2) as the *base* score; Stream F dreaming's three-pass pipeline and lease; the dynamics/strength ranking term; Stream D privacy helpers; the recall-explanation/omission accounting and observability counters (v0.6 §3.3, §13.1); the v0.6 read-only-hook and uncached-tail cache-stability contracts (2026-06-19 amendment).

**Changes (net-new or reshaped).** The "Phase" column marks the build phase each change lands in:

| Area | v0.6 (today) | v3.0 target | Phase |
| --- | --- | --- | --- |
| Injected unit | `<memory>` title + empty snippet | prose recollection, ref preserved as attribute (§7) | **P1** |
| Budgeting | summary tokens only | full rendered-byte cost (§7.4) | **P1** |
| Truncation | cuts at any newline → malformed XML | cuts at entry boundary, always well-formed (§7.5) | **P1** |
| Injection position | uncached tail (v0.6 amendment) | unchanged; documentary invariant for the prose unit (§9.5) | **P1** *(B8)* |
| Telemetry/feedback | read-only hooks, no feedback writes (v0.6) | out-of-band `PassiveSurfaced`/`RecallUsed`, positive-only (§5.3) | **P1/P3** |
| Tool discoverability | generic guidance string | guidance names `memory_search`/`memory_get` (§3.4) | **P1** |
| Continuity model | none | maintained `ContinuityState`, closeout + dream-time (§4–§5) | **P2** *(B2 gate)* |
| Closeout | no surface exists | SessionEnd hook writes continuity state (§5.1) | **P2** |
| Per-turn trigger | delta on every prompt | friction/relevance-gated; silence is valid (§3.2, §6) | **P3** |
| Ranking | no relevance-to-now term | adds a cue-relevance term (§6.2) | **P3** |
| Startup content | recency-ranked atom dump | desk read + continuity model + skeleton, salience-ordered prose (§3.1, §8) | **P4** *(desk)* |
| Re-entry / compaction | not handled | new T2 trigger rendering current absolute desk + immediate focus (§3.3) | **P4** |

---

## 1. Conceptual model

### 1.1 Three data types

Passive recall operates over three distinct data types. Conflating them is the original design error.

1. **The continuity-state model** *(ships Phase 2, §4)* — a small, maintained, synthesized object per project: a slow-changing *skeleton* (what the project is, its architecture, hard constraints), a *volatile* layer (current focus, what's hot, what's blocked), *open loops* (unresolved threads ranked by stakes), and *staged notes* (prospective reminders left deliberately for future-me). Written at closeout and dream-time; read at orientation and re-entry. §4.

2. **Recollections** — gist units. A recollection is one self-contained, declarative proposition with a consequence — the lesson — derived from a canonical memory, rendered as prose, carrying provenance (`ref`, `kind`, `confidence`). Recollections are what T1 surfaces. §7.

3. **The desk** — live repo state: branch, uncommitted-file summary, recent commits, and (best-effort, off-path) open PR / CI. Not memory; *context*. Read at session boundaries under a tight fail-open deadline (§8); the network half is off-path; the daemon-cached projection is a deferred optimization (§8.0, §16). Joined with memory at orientation/re-entry so recall reflects what the session is probably about. §8.

### 1.2 The four triggers

| | Trigger | Lifecycle event | Analogy | Status |
| --- | --- | --- | --- | --- |
| **T0** | Orientation | SessionStart (`startup`) | walking into the office | reshape existing |
| **T1** | Associative recall | UserPromptSubmit (+ SubagentStart) | remembering while you work | reshape existing |
| **T2** | Re-orientation | SessionStart (`compact`/`resume`) | coming back after a gap | reuses existing matcher; new response variant (**not** PreCompact, §3.3) |
| **T3** | Deliberate recall | `memory_search` / `memory_get` | consciously trying to remember | exists; fix discoverability |

Plus one non-injection trigger that closes the loop:

| | Trigger | Lifecycle event | Role | Status |
| --- | --- | --- | --- | --- |
| **C0** | Closeout | SessionEnd / Stop | write the continuity state | **net-new hook (ships Phase 2)** |

### 1.3 The lifecycle loop

```
   work ──▶ closeout ──▶ dream-time ──▶ orientation ──▶ work
 (T1/T2)   (C0: snapshot   (refine model,   (T0: read it
            + stage note)    decay/boost)      back, + desk)
```

The session's end writes its successor's beginning; the quiet hours in between maintain the model and compost the noise. Forgetting is a feature: a recollection surfaced repeatedly and never *used* is noise the system hasn't yet learned to stop surfacing (§5.3).

**v3.0 ships the full loop**, with one guardrail: the closeout-writes-startup arm passes through the **B2 substance/acceptance gate** (§4.0) so a hollow or degraded snapshot can never become authoritative orientation. The expensive multi-writer prerequisites that v1.1 attached to this loop (item-level cross-device merge, a governance carve-out, an adoption-proof gate) are re-priced for n=1 — see the Revision goal and §4.0.

---

## 2. Safety invariants

All live v0.6 §2 invariants carry forward unchanged: recall is read-only; passive hooks perform no surface-marker or recall-hit feedback writes (2026-06-19 amendment); no encrypted plaintext in recall; governance lifecycle is authoritative; tombstoned/superseded records do not teach; candidates/quarantines are attention, not truth; the token estimator is deterministic (`ceil(utf8_byte_len / 4)`); output is byte-stable for cacheability; errors are typed.

This spec adds:

1. **Recollections are declarative and attributed, never imperative.** An injected memory ("Always do X") is reframed as reported fact ("Recalled — the standing practice has been X") before emission. The existing `neutralize_imperative_prose` path is the mechanism; v3.0 makes attributed-declarative the contract, not a best-effort. Rationale: injected memory is a prompt-injection surface, and the more it reads as the agent's own voice, the more a poisoned or wrong memory costs.
2. **System-derived and low-confidence recollections are framed as external evidence, not internal fact.** *(R9.)* Declarative rephrasing alone is insufficient: a wrong memory still steers as a *reported fact* ("the standing practice has been to run script X"). Any recollection that is system-synthesized (not directly user-authored) or below a confidence threshold must be rendered as third-party evidence the agent should weigh — "A prior memory reports…", "An earlier note claims…" — never as a settled fact in the agent's own voice. At n=1, this is also how the single user distinguishes a dream-synthesized gist from their own words. The framing is part of the rendered-byte determinism tuple.
3. **Provenance is always recoverable.** Every surfaced recollection carries a `ref` to its canonical memory. The agent (and an injection detector) can always trace a recollection to its source; recall never emits free-floating instructions.
4. **Recall is strictly read-only — no write is *caused by* a recall hook.** *(B1; re-affirms v0.6's read-only-hooks amendment.)* T0/T1/T2 perform zero disk/event mutations on the synchronous read path. Telemetry (`PassiveSurfaced`, `RecallUsed`) and any note-consume state-change are executed **out-of-band** — either harness-driven, or via a separate, decoupled, asynchronous post-render daemon endpoint — with no side effect on, and no ordering dependency from, the read path. The option of "narrowing the invariant to permit deterministic recall-time telemetry writes" is explicitly rejected; it would also regress the shipped v0.6 contract (§5.3).
5. **The desk read is read-only and fail-open.** Serving git/desk state never mutates the repo. It runs only on the session-boundary path (T0/T2), under a tight desk-specific deadline well inside the hook's fail-open budget, and any timeout/failure/lock-contention degrades silently to memory-only orientation (§8.3). The per-turn path (T1) spawns no subprocess.
6. **The continuity-state object is governed and classified like any memory.** *(B7 resolved by no carve-out.)* It is written through the Stream A write path, passes Stream C governance with **no carve-out**, and is classified by Stream D. It must never embed encrypted plaintext, secret-class content, or unreviewed candidate claims (§4.4).
7. **Closeout is read-mostly, gated, and bounded.** *(B2.)* The SessionEnd hook writes at most the continuity-state memory and staged notes through the governed write path; it passes the substance/acceptance gate (§4.0) so a hollow summary cannot become authoritative; it never bulk-imports a transcript and never blocks harness shutdown beyond its deadline (§5.1).

---

## 3. The four triggers

Each trigger is dispatched by the existing unified `recall hook` handler (`cli/recall_hook.rs`), which maps a `hook_event_name` to a daemon request. v0.6 wires three events (SessionStart, UserPromptSubmit, SubagentStart) via `hooks_wire.rs`. **v3.0 adds exactly one new hook wire — SessionEnd (C0, Phase 2).** T2 re-orientation rides the *existing* SessionStart matcher (`startup|resume|clear|compact`) on the `compact`/`resume` sources (§3.3) — a new response variant, not a new event. **PreCompact is deliberately not used** (it is block-only, with no context-injection surface in Claude Code or Codex; §3.3, §14.1).

### 3.1 T0 — Orientation (SessionStart)

**Fires:** SessionStart (existing matcher `startup|resume|clear|compact` for Claude; matcher-free for Codex).

**Reads:**
- the project's **continuity-state model** (§4): skeleton + volatile + the highest-stakes open loops + any unconsumed staged note — read via the ordinary indexed `MemoryQuery` (one pinned, project-scoped, `continuity-state`-tagged memory);
- the **skeleton fallback**: pinned/active `me` identity + project `invariant`/`state`/`decision` memories (the existing `<identity>` / `<project-state>` candidate sources), salience-ordered by stakes, used when the continuity object is absent or held back by the B2 gate;
- the **desk** (§8): branch, uncommitted-file summary, recent commits, and best-effort PR/CI — read at the session boundary under a fail-open deadline (§8), never on the per-turn path.

**B2 gate at read time.** T0 must not render a continuity-state object that is hollow (empty `volatile`, no open-loop evidence) or flagged `degraded: true` as authoritative orientation. A held-back object degrades to the skeleton fallback + desk (§4.0.2).

**Cold-start (R2).** When all inputs are empty — first session in a project, no continuity object, no pinned skeleton, no/empty desk — T0 emits **desk-only orientation (if any) plus pinned identity/invariants, and otherwise the empty wrapper.** It never falls back to a recency dump; a recency-ranked atom list is exactly the v0.x failure mode this redesign exists to kill. See §6.5. (Desk-anchored cold-start lands with the desk read in Phase 4; before then, cold-start is pinned-identity-or-empty.)

**Injects:** a glance — what's nagging, where we left off, the desk crossed with memory. Leads with **unresolved × consequential** (project-salience, O1b), not latest. Flags recent pivots that override a stale assumption ("priority changed last session"). Renders as prose recollections (§7), not an atom list.

**Budget:** target ≤ 300 estimated tokens, hard cap ≤ 600 (replaces the v0.6 startup budget). Never truncates mid-entry; if over budget, drops lowest-salience items and records them as omissions.

### 3.2 T1 — Associative recall (UserPromptSubmit, SubagentStart)

**Fires:** UserPromptSubmit (and SubagentStart, with the subagent's task as cue). Maps to the existing `Delta` request path (`build_delta_response`, `passive: true`).

**Cue:** the submitted message + a bounded window of recent conversation/tool state (§6.1). This is the spreading-activation input.

**Gating — the heart of the change (§6):**
- A cheap **friction pre-gate** suppresses only **obvious no-ops** *(B6)* — bare acknowledgements ("yes", "ok", "do that", "thanks") and empty/trivial prompts. It is *not* the primary relevance filter: every substantive prompt proceeds to the relevance gate **regardless of whether it contains friction words**.
- A **relevance gate** then admits only recollections whose activation clears a high bar. If none clear it, inject nothing — and that is the system working correctly.
- **Lessons** (`feedback`/correction memories) get a salience boost (`lesson_boost`) when the cue carries a decision/difficulty signal — protective recall, surfaced exactly when the agent is about to do the hard thing again. The boost has an **independent path**: it is *not* gated on the friction pre-gate firing *(N6)*.
- Dedup against what is already in context this session (§6.4): never re-surface a recollection already shown (subject to the turn-distance threshold, N3), and never restate what the native memory head already carries.

**Subagent dedup scope** *(under-spec closed).* A SubagentStart cue dedups against the **subagent's own** surfaced set, not the parent's — a fresh subagent context has not "seen" the parent's recollections, so inheriting the parent's suppression set would starve it. The parent and each subagent maintain independent same-session dedup scopes keyed by their context id.

**Injects:** ≤ 3 recollections by default, as prose conclusions (§7), at the uncached tail (§9.5).

**Budget:** ≤ 360 estimated tokens (reuses the v0.6 delta budget), but spent on the *margin* — usually far under, often zero.

### 3.3 T2 — Re-orientation (post-compaction / resume SessionStart) — reuses the existing matcher

**Fires:** **`SessionStart` with source `compact`** (the post-compaction session start), and `SessionStart` source `resume` after a long idle gap. **Not `PreCompact`** — see the harness note below. The `compact`/`resume` tokens are already in the existing SessionStart matcher, so T2 needs **no new hook wire**; it is a new *response variant* (`RequestPayload::Reorient`) dispatched when SessionStart fires with the compact/resume source.

**Reads:** the immediate sub-task — the continuity model's `volatile` layer and the most recent relevant recollection — not the whole project. Finer than orientation, more local than per-turn.

**Injects:** "here's what we were *just* doing" — a single compact re-orientation recollection plus the **current absolute desk state** (branch, dirty-file summary). Delivered via the SessionStart(compact) `additionalContext` surface, i.e. at the start of the post-compaction conversation segment (a session-boundary trigger, not a per-turn one — §9.5).

**No desk delta** *(R10).* v1.0 specified "the live desk delta since the session started," which requires persisting a T0 baseline somewhere — on disk (second-persistence-layer violation), as a memory (read-only-write violation), or in-process (stateful daemon that drifts on restart/crash). v3.0 renders the **current absolute desk state** instead. Both invariants hold; "what changed" is something the agent can see from the desk itself plus its own retained context. *(Future enhancement: once the deferred daemon desk projection ships (§8.0, §16), it can carry a per-session high-water mark in the same in-memory state it already maintains — no new persistence layer — and compute a true delta, falling back to absolute on cold/missing baseline. Tracked in §16, not built in the core sequence.)*

**Budget:** ≤ 200 estimated tokens.

**Harness note (verified — §14.1).** `PreCompact` is **not a context-injection surface** in either Claude Code or Codex: it is a pre-compaction event that can only *block* (exit code 2 / `decision: "block"` / `continue: false`) and exposes no `additionalContext` output shape (Claude Code Hooks docs; OpenAI Codex Hooks docs; Codex verification doc). Re-orientation context must therefore be injected at the *post*-compaction boundary, which both harnesses expose as `SessionStart` with source `compact`. So T2 rides SessionStart(compact) on both harnesses; PreCompact is not used.

**Rationale:** context compaction is exactly the "got interrupted, need to re-orient to the immediate task" moment, and today nothing fires there. This is the cheapest high-value net-new trigger — and on the corrected wiring it costs no new hook event.

### 3.4 T3 — Deliberate recall (the tools)

`memory_search` (`{ query, limit, include_body }`) and `memory_get` (`{ id, include_provenance }`) already exist (`mcp.rs`). v3.0 changes one thing: **discoverability.** The `guidance` string returned by T0/T1 (today the generic `"Memorum passive recall assembled from read-only index projections."`) names the tools and notes that a recollection's `ref` *can* be dereferenced for full provenance — e.g. *"Recollections are self-contained; to go deeper, `memory_get <ref>` returns a recollection's full source, and `memory_search` queries memory directly."* The framing keeps the affordance available without implying the agent must fetch to understand a recollection *(N1)* — the body already carries the conclusion (O2).

---

## 4. The continuity-state object (the spine — ships Phase 2)

Today nothing maintains a "where we left off" or project-state summary — the `<project-state>` block is just the project identity binding (`project_body` emits only the project id + namespace). The maintained model below closes that gap, and is the mechanism by which orientation becomes *remembered* rather than *recent*.

### 4.0 The one hard prerequisite: the substance/acceptance gate (B2)

v1.1 gated this object behind five prerequisites plus an adoption-proof. At n=1, four of the five are re-priced (see Revision goal): **B1** ships in Phase 1 regardless; **B3** (item-level merge) and **B7** (governance carve-out) are relaxed; **R5** (continuity-claim invalidation) lands as a lite desk-contradiction check with the desk read (Phase 4). The lone hard prerequisite that survives is:

**B2 — closeout orientation must be gated on substance, not a timestamp.** T0 renders the continuity state as authoritative prose; a fail-open closeout that writes a fresh-timestamped *hollow* object would poison startup with confident-wrongness — and an `updated_at` freshness gate is defeated by exactly that fresh-but-hollow snapshot. Required instead, before C0 is wired:

1. A **completeness/quality contract**: a continuity-state write is *substantive* only if it has a non-empty `volatile` layer or at least one evidenced open loop; otherwise it is written `degraded: true`.
2. A **degraded object never renders as authoritative** at T0 — it degrades to the skeleton fallback + desk (§3.1).
3. **"Remembered, not verified" surface framing** for the continuity claims (R9 lead-ins), so the single user weighs them rather than trusting them blind.

A two-state test (rich vs. hollow closeout) gates this: a hollow auto-snapshot must register as `degraded` and must not become orientation. Until that test passes, C0 is not wired and T0 reads only the skeleton fallback + desk.

*(Re-priced, not required — tracked in §16.)* **B3 item-level cross-device merge:** ship whole-object supersede; the rare concurrent-closeout clobber is acceptable at n=1 and the merge driver still guarantees convergence. **B7 governance carve-out:** removed entirely — continuity writes use the normal governed path (no special case). Add item-level merge or tighter provenance only if real multi-device usage shows the clobber actually bites.

### 4.1 Shape

```rust
struct ContinuityState {
    project: String,              // canonical project id
    version: u64,                 // monotonic; increments each rewrite
    updated_at: DateTime<Utc>,
    degraded: bool,               // true if hollow or a classified fragment was dropped (B2/R7)
    skeleton: Vec<StateClaim>,    // slow-changing: what this project is, architecture, hard constraints
    volatile: Vec<StateClaim>,    // fast-changing: current focus, what's hot, what's blocked
    open_loops: Vec<OpenLoop>,    // unresolved threads, ranked by stakes
    staged_notes: Vec<StagedNote>,// prospective reminders left deliberately for future-me
}

struct StateClaim {
    text: String,                 // declarative prose, bounded to 240 UTF-8 bytes
    refs: Vec<String>,            // canonical memory ids this claim summarizes
    derived: ClaimSource,         // UserAuthored | SystemSynthesized — drives R9 framing
}

struct OpenLoop {
    id: String,                   // stable item id (cheap to add now; enables future item-level merge, B3)
    text: String,                 // "radar tuning pass — flagged, not started"
    stakes: Stakes,               // High | Medium | Low — drives orientation ordering
    opened_session: String,       // session id that opened it
    refs: Vec<String>,
}

struct StagedNote {
    id: String,
    text: String,                 // "next session, start by checking X"
    author: NoteAuthor,           // Agent | Human
    created_at: DateTime<Utc>,
    consumed: bool,               // cleared once surfaced and acknowledged (out-of-band write, B1)
}
```

*(Note: `OpenLoop.id`/`StagedNote.id` and `StateClaim.derived` are cheap to carry from day one and cost nothing at n=1; they are what a later item-level merge (B3) and provenance tightening would build on without a schema change.)*

### 4.2 Storage

The continuity-state object is a **canonical Stream A memory**, not a private store (preserves the v0.6 "no hidden second persistence layer" invariant). One per project, scoped `project:<canonical_id>`, reserved tag `continuity-state`, pinned status. It is rewritten by **supersede** (Stream C `WriteMode`), so its history is the event log. `version` increments each rewrite.

**Merge at n=1: whole-object last-writer-wins** *(B3 re-priced).* Two devices superseding this object concurrently at their respective session-ends is a near-zero event for one human, and the failure (a clobbered open-loop) is noticed and re-added in seconds. v3.0 ships whole-object supersede over the existing merge driver, which still guarantees two-clone canonical-content convergence. The stable item ids (§4.1) are carried so that *if* concurrent-closeout clobber ever proves real in dogfooding, item-level merge is an additive build, not a schema migration (§16).

Orientation reads it via the ordinary indexed `MemoryQuery` (status `pinned`, namespace `project:<id>`, tag `continuity-state`) — cheap, deterministic, no new query surface.

### 4.3 Who writes it

- **Closeout (C0, §5.1)** writes the cheap, deterministic layer: appends/updates `open_loops` and `staged_notes` from the session's agent-supplied summary, bumps `volatile` minimally, sets `degraded` per the B2 contract. No LLM call on the hot path.
- **Dream-time (§5.2)** writes the synthesized layer: refines `skeleton` and `volatile`, dedups and re-ranks `open_loops`, prunes resolved/stale loops, using the existing pass-1/pass-2 machinery. This is where the heavy synthesis lives, off the hot path. Synthesized claims are marked `SystemSynthesized` (§4.1) so they render with R9 external-evidence framing.

### 4.4 Privacy and governance

The continuity-state memory passes Stream C governance (**no carve-out**, B7) and Stream D classification like any write. It must never embed encrypted plaintext, secret-class content, or unreviewed candidate claims. Each `StateClaim.text` and `OpenLoop.text` runs through `safe_plaintext_fragment` before persistence; a fragment that classifies non-`Allow` is dropped (not encrypted into the summary).

**Partial-drop is a degraded write, not a silent edit** *(R7).* Dropping a classified fragment can invert meaning ("don't mention X until legal" → "don't mention until legal"). When any fragment of a continuity-state write is dropped, the resulting object is marked **`degraded: true`** and **must not silently supersede the last-known-good** version as authoritative orientation — a degraded object is held back or surfaced as explicitly partial, never rendered as settled state (the same B2 gate path). A secret-class closeout summary is refused exactly as any write (`SecretRefused`), never silently stored.

---

## 5. The lifecycle loop

### 5.1 Closeout (C0) — net-new SessionEnd hook (ships Phase 2)

**Fires:** a new SessionEnd (Claude `Stop` / `SessionEnd`; Codex equivalent) hook, wired into `hooks_wire.rs` matcher tables and `HOOK_EVENTS` — the single new hook wire in v3.0. The daemon gains a `RequestPayload::Closeout` variant (the first session-end surface in the protocol). The continuity-state write it performs is a SessionEnd write, not a T0/T1/T2 read, so it does not violate the strict read-only recall invariant (§2 inv 4); it must still pass the §4.0 (B2) acceptance contract so a hollow summary cannot become authoritative.

**Input:** a bounded, agent- or harness-supplied **session summary** — at most a few hundred tokens describing where things landed, the unfinished thread, and any deliberate staged note. The agent authors this (e.g. via the existing `memory_note` surface or a closeout-specific structured field); if absent, closeout falls back to a minimal auto-snapshot (active project + the session's touched entities), which by construction registers as `degraded: true` (it has no substantive `volatile`/open-loop evidence) and therefore does **not** become authoritative orientation (B2). Closeout never ingests a full transcript.

**Effect:** updates the continuity-state object (§4.3): records the open loop(s), appends staged notes, minimally refreshes `volatile`, sets `degraded`. Through the governed write path; fail-open; bounded by a deadline (≤ the v0.6 800 ms hook deadline) so it never blocks harness shutdown.

**Determinism note:** closeout *writes*, so it is exempt from the read-path byte-stability invariant — but its writes go through the normal event log and merge driver and are themselves reproducible given the same summary input.

### 5.2 Dream-time maintenance (ships Phase 2)

Reuses the Stream F nightly pipeline (launchd, lease-elected, shells to the harness CLI). It adds a continuity-maintenance pass alongside the existing three:

- **Refine** `skeleton`/`volatile` from the period's active+candidate memories and the accumulated closeout snapshots.
- **Re-rank and prune** `open_loops`: drop loops whose referenced work is resolved (the referenced memories superseded/closed), merge duplicates, re-score `stakes`.
- **Mark** every synthesized claim `SystemSynthesized` (§4.1) for R9 framing, and decay/boost feeds from the use-feedback signal (§5.3), not from inclusion counts.

This pass writes a new continuity-state version via supersede. It is the only place the *synthesized* layer changes; closeout only touches the cheap layer. Reuses pass-1 masked-reflection and pass-2 candidate-write machinery; the continuity-state write is governed exactly like a pass-2 candidate (**no carve-out**, B7) except it targets the reserved pinned `continuity-state` memory.

### 5.3 The use-feedback signal

Today, strength's frequency term is driven by `RecallHit` events, which fire for every memory *included in a rendered active-path block* — i.e. **inclusion, not use** — and the passive path emits none (v0.6 read-only-hooks). v3.0 separates the signals and corrects two flaws the fusion review found.

**Events (logged out-of-band, B1).**

- **`PassiveSurfaced`** — a recollection was injected passively. Lets us measure surfaced-but-unused.
- **`RecallHit`** — included in a block (existing; rename-neutral).
- **`RecallUsed`** — the agent demonstrably acted on a surfaced recollection; v3.0 operationalizes this conservatively as an explicit `memory_get` on a surfaced `ref` within the same session.

These events are **writes, and recall is strictly read-only (§2 inv 4)** — so none are emitted on the T0/T1/T2 synchronous path. They are produced **out-of-band**: harness-driven, or via the decoupled asynchronous post-render daemon endpoint, after the block has been rendered and returned. The read path computes its response and emits nothing.

**`RecallUsed` is a positive-only signal; disuse is never inferred from its absence** *(B5).* Ambient recall *succeeds* precisely when the agent reads the self-contained prose and acts on it **without** calling `memory_get` — so a missing `memory_get` is the expected outcome of a *working* recollection, not evidence it was unused. v1.0's plan to decay `PassiveSurfaced`-without-`RecallUsed` as a disuse signal would therefore cool exactly the recollections that worked. v3.0 forbids that inference: `memory_get`-on-ref counts only **toward** strength (positive), never against it. *(This survives n=1 intact — a bad disuse inference would cool the single user's working recollections.)*

**Strength re-weighting and disuse-decay stay deferred** until a **validated** disuse signal exists. A reliable "surfaced-but-unused" measure needs a softer signal than `memory_get`-absence (e.g. next-turn n-gram overlap with the recollection, or same-session edits to the files/entities it concerns — §16), validated against real dogfooding before it drives decay. Until then, v3.0 ships the events for **measurement only** (the trust/usefulness ratio in §9.4) plus the cheap **miss-signal** (§6.3), and the strength frequency term keeps its current `RecallHit` basis. *(R6: because the passive path still emits no inclusion `RecallHit`, gating calibration in Phase 3 must be done against the structural-only base explicitly, not a strength term that silently reads zero for passive candidates.)* No persisted strength column is introduced.

---

## 6. Relevance, gating, and the cue

### 6.1 The cue

- **T0:** no user message yet → the cue is the continuity-state object + desk projection (§8) + the pinned skeleton. Orientation is not a semantic retrieval; it is reading state and ordering by salience. The cue-relevance term (§6.2) is zero here (O1a vs O1b).
- **T1:** the submitted message + a bounded rolling window of recent turns/tool state.
- **T2:** the continuity `volatile` layer + most recent relevant recollection.

### 6.2 Activation scoring

The existing structural score (v0.6 §8.2: status + scope + entity-match + recency + confidence + source, plus the bounded strength term, fused with the v0.6 vector RRF lane) becomes the **base**. v3.0 adds the missing organ: a **relevance-to-cue term** for the T1/T2 paths.

```
activation = base_structural_score
           + relevance_to_cue        // NEW: semantic/lexical match of memory ↔ live cue
           + lesson_boost            // NEW: feedback/correction memories at decision/difficulty points
```

`relevance_to_cue` reuses the existing v0.6 chunk/vector RRF retrieval already on the delta path (`query_chunks` over the message) plus entity-seed overlap; it is the term that makes recall *relevant* rather than merely recent. On T0 there is no cue, so this term is zero and orientation rests entirely on the continuity model + skeleton + desk — by design.

**Testable ranges** *(N4).* Both new terms have specified, bounded, testable ranges so ranking behavior is verifiable: `relevance_to_cue ∈ [0.0, R_max]` and `lesson_boost ∈ {0, L}` (applied iff the cue carries a decision/difficulty signal, independent of the friction pre-gate — N6). The concrete `R_max`, `L`, and the relevance floor (§6.4) are config constants tuned by dogfooding (not a labeled corpus, R4 re-priced); the spec fixes their ranges and the acceptance tests assert that a higher cue-match reorders candidates and that `lesson_boost` is additive and bounded.

Determinism is preserved per §9: given the same index state and cue, scoring is reproducible; the v0.6 vector path already has a `vector_recall_degraded` soft-fail flag that keeps recall structural-only on retrieval failure.

### 6.3 The friction pre-gate (T1)

**The pre-gate suppresses only obvious no-ops; it is not the relevance filter** *(B6).* v1.0's pre-gate gated *all* surfacing on lexical friction signals (error output, decision words, entity novelty). The fusion review (judge-elevated to a blocker) showed that produces a large, **silent** false-negative rate on exactly the turns where memory matters most but no friction word appears — procedural reuse ("do the same fix in billing"), social context ("reply to Adam"), terse status continuations ("ship it"). v3.0 inverts the design:

- **The pre-gate rejects only obvious no-ops:** bare acknowledgements ("yes", "ok", "do that", "thanks", "👍"), empty or whitespace-only prompts, and pure tool-result acks with no new content. These never need recall.
- **Every other (substantive) prompt proceeds to the relevance gate**, whether or not it contains friction words. Relevance — not lexical friction — decides what surfaces. Silence then comes from *nothing clearing the relevance floor*, which is measurable, rather than from a pre-gate that fired invisibly.
- **Friction signals become a salience input, not a switch.** Error output in recent tool state, decision/difficulty cues, and entity novelty raise priority and trigger `lesson_boost` (§6.2) — they *promote* protective recall, they no longer *authorize* recall.

**Observability so the gate is tunable — the cheap half only** *(B6 re-priced for n=1).* The single user will *feel* a bad gate in real time, so v3.0 keeps the cheap, genuinely-useful feedback loop and drops the production-scale apparatus:

- **Keep: the miss-signal.** A `memory_search` or `memory_get` issued by the agent shortly after a silent T1 is logged (out-of-band) as a candidate **gate/relevance miss** — the agent went looking for something the channel should arguably have surfaced. Cheap, and a fast precision-debugging signal for the developer-who-is-the-user.
- **Drop: the forced-sampling deterministic self-test.** v1.1's ~1%-forced-surfacing self-test exists to measure precision *without a human watching*. The single user is watching. Not built (kept in §16 in case multi-user ever arrives).

Tier 2 (deferred, §16): an embedding-centroid drift signal for topic shift. Net effect: the pre-gate is a cheap cost-saver on genuinely empty turns, and the *relevance floor* (§6.4) carries the precision burden.

### 6.4 Gating discipline

- **Gate, don't truncate.** Selection admits a recollection only while its activation clears the relevance floor *and* its full rendered cost fits the budget. Stop when either fails. Truncation (§7.5) is a last-resort backstop.
- **The relevance floor ships permissive and is tuned by dogfooding** *(R4 re-priced).* The floor is the whole game and is empirical; defaulting it tight before calibration would silently starve the channel. v3.0 ships T1 relevance gating with a deliberately **permissive floor** and tightens it by feel during dogfooding. *(No flag-gated rollout and no hand-labeled "should-this-surface?" corpus — the single user is the label. If multi-user ever arrives, the labeled set returns from §16.)*
- **Silence is valid output.** Zero recollections is a correct, common T1 result. The CLI/daemon still emits the empty wrapper so downstream parsing never branches on emptiness — consistent with the v0.6 delta-empty contract. **The exact empty-wrapper form is specified per trigger** *(N5)*: T0/T1/T2 each emit a single self-closing `<memory-recall empty="true" trigger="t0|t1|t2" policy="stream-e-v3.0" />` with no child content, asserted by the Stream H parser regression.
- **Dedup against working context, with a turn-distance threshold** *(N3).* Extend the existing native-memory-head dedup (`read_native_memory_head`) to also suppress recollections already surfaced earlier in the same session (per-context scope, §3.2) and content already present in the loaded CLAUDE.md/AGENTS.md head. Suppression is **not permanent for the whole session**: a recollection may re-surface after a turn-distance threshold (a meaningful gap or topic shift). Don't re-tell the agent what's already on screen *now*; do allow it back when it becomes relevant again later.

### 6.5 Cold-start (first contact) *(R2)*

The single most important orientation case is the one v1.0 left undefined: **no continuity-state, no pinned skeleton, no focus** — the first session in a project, or after the relevant memories were deleted. T0 must **not** degrade to a recency dump. Defined behavior:

1. If a desk projection exists (Phase 4+), render **desk-only orientation** (branch, dirty summary, recent commits) as the anchor.
2. Add any pinned identity/invariants that do exist (these are global/`me`-scoped and usually present even on a brand-new project).
3. If neither exists, emit the **empty wrapper** (§6.4) — orientation is allowed to be empty on true first contact.

Cold-start is a Phase-2/Phase-4 acceptance fixture (a project with zero project-scoped memories), asserting desk-or-empty output and the absence of any recency-ranked atom list.

---

## 7. Injection format and rendering

### 7.1 The recollection unit

The `<memory>` element of v0.6 (title + always-present, often-empty `<snippet>`) is replaced. The unit becomes a prose recollection that preserves machine-readable provenance:

```xml
<recall ref="mem_20260619_40edd13334a43d72_000534" kind="lesson" confidence="0.70">
  Recalled — last time the Node 24 Playwright extract step hung in CI; you
  unblocked it by pinning the browser revision.
</recall>
```

- The body is **declarative attributed prose** — the conclusion, not a heading *(O2)*. It is self-contained: the agent can act on it without dereferencing anything. Bounded to a single short paragraph.
- `ref` is preserved as an attribute purely for **provenance/traceability** — the Stream H eval parser depends on a `ref` contract (§15 updates the parser to read `<recall ref=...>` rather than `<memory ref=...>`), and an injection detector can trace any recollection to its source. It is **not** a handle the agent is expected to fetch in order to understand the recollection *(N1)*. Keep the opaque id; do **not** add a human-readable alias (it would create a second identifier that diverges across devices and breaks eval-fixture byte-stability).
- `kind` ∈ { `lesson`, `state`, `open-loop`, `staged-note`, `fact` } drives presentation and the lesson boost.
- `confidence` is preserved and **drives the evidence framing (R9, §7.3)**: low-confidence or system-derived recollections render with an external-attribution lead-in ("A prior memory reports…"), not the agent's own voice. `updated`/`source` are dropped from the surface form (provenance recoverable via `memory_get <ref>`), eliminating ~50 chars/entry of scaffolding the model cannot use as prose. Microsecond timestamps and opaque source kinds do not appear in the injected text.
- **No empty elements.** A recollection with no usable body is not surfaced at all (this both fixes and generalizes `500a60d`'s empty-snippet omission, and retires the v0.6 "snippet always present" contradiction along with the `<memory>` element).

### 7.2 Block envelope

The top-level `<memory-recall ...>` envelope and the `<recall-explanation>` accounting block (v0.6 §3.3, §9.6) are retained — they carry the omission/budget metadata Stream G and the eval harness consume. What changes is the *content* sections: orientation renders the continuity model + skeleton + desk as labeled prose groups; T1/T2 render a flat list of `<recall>` units. Per-turn dynamic content is placed at the uncached tail per §9.5.

### 7.3 Attributed-declarative rendering, with external-evidence framing for untrusted sources

Every surfaced body passes `neutralize_imperative_prose` (now contract, not best-effort — §2 inv 1). An imperative source ("Always run the gate before pushing") renders as reported fact ("Recalled — the standing practice has been to run the gate before pushing"). Declarative sources pass through byte-identically so the determinism tuple stays stable.

**Declarative is not enough on its own** *(R9, §2 inv 2).* A wrong or system-synthesized memory still steers as a *reported fact*. So the lead-in is keyed to trust: a **user-authored, high-confidence** recollection (or `StateClaim.derived == UserAuthored`) may render as a direct recollection ("Recalled — …"), but a **system-synthesized or low-confidence** recollection must render as third-party evidence the agent should weigh — "A prior memory reports…", "An earlier note claims…". The agent is never handed a system-derived claim in its own voice. The chosen lead-in is a deterministic function of `kind`/`source`/`confidence`/`derived`, so it is part of the byte-stability tuple.

### 7.4 Budget on rendered bytes

The root cause of the original overflow: selection counted **summary tokens only** (`estimated_tokens(&summary)`), while each rendered entry also carried ~120 chars of scaffolding, a 35-char ref, and a timestamp — so "within budget" undercounted real size by multiples, and ~30 entries blew past the 10 k-char cap. v3.0 budgets on the **full rendered cost** of each unit (attributes + body + envelope scaffolding). The per-entry-count cap becomes a redundant safety net rather than the primary defense.

### 7.5 Well-formed truncation

Today's `cap_passive_block` cuts to the last newline and appends `</memory-recall>`; if the cut lands inside an entry, the result is an unclosed element — malformed XML (observed in the original paste: an unclosed `<memory>` followed by `<recall-truncated/>`). v3.0 requires truncation to cut at a **recollection boundary** (a complete `<recall>…</recall>` unit), so the emitted block is always well-formed. The backstop should almost never fire given byte-budgeting (§7.4), but when it does it must not emit malformed output.

### 7.6 Per-trigger budgets

| Trigger | Target | Hard cap | Default entries |
| --- | ---: | ---: | ---: |
| T0 orientation | 300 | 600 | continuity model + skeleton + desk |
| T1 associative | margin (often 0) | 360 | ≤ 3 |
| T2 re-orientation | 120 | 200 | 1 + current desk state |

---

## 8. The desk read (repo state ingestion) — ships Phase 4

Today git is read *only* for project binding (`git remote get-url origin`, `git rev-parse --show-toplevel`, both under a 2 s deadline). No branch, status, log, PR, or CI is ingested.

### 8.0 Hot-path discipline: network off-path, local git bounded, projection deferred *(B4 re-priced)*

v1.0 specified reading the desk by running `git status --porcelain`, `git log`, and `gh pr view` synchronously on the hook path. The fusion review found two distinct hazards; v3.0 prices them separately for n=1:

- **Network (`gh pr view` + CI) stays off the hot path — unconditionally.** Network on a synchronous recall path is a real latency/flakiness hit even at n=1. PR/CI are refreshed by a slow background cadence and are always optional; if absent, orientation proceeds without them.
- **Local git (branch/status/log) runs synchronously at session boundaries only, under a tight fail-open deadline.** Desk is read at T0/T2 (once per session segment), not per-turn, on the user's own normal-size repos. A synchronous `git status --porcelain` / `git log --oneline -n N` there, under a desk-specific deadline (target p95 ≤ 60 ms, hard timeout well inside the v0.6 800 ms hook budget), degrading to memory-only on timeout/`.git/index.lock` contention, is acceptable at n=1. The judge's monorepo/`index.lock` concern is real but rare and fully absorbed by fail-open.
- **The daemon-cached desk projection is a deferred optimization (§16).** A background file-watcher (`Substrate::watch()` over `.git`) + `interval_at` refresher maintaining an in-memory `DeskProjection` read O(1) on the hook eliminates even the bounded synchronous cost. It reuses the daemon's established background-task idioms (reality-check scheduler, embedding worker; per the Codex verification). Build it **if** a session start ever stalls in practice; it is not on the core n=1 sequence.

### 8.1 Inputs

- current branch (`git rev-parse --abbrev-ref HEAD`);
- uncommitted-work summary (`git status --porcelain`, counted/summarized, never file contents);
- recent commits (`git log --oneline -n N`);
- open PR for the branch (`gh pr view`, if `gh` is present and authenticated) — **off-path only**, slower cadence, optional;
- CI status (from the PR view, if available) — off-path only, optional.

### 8.2 Join with memory

The desk is crossed with the continuity model + skeleton: "you're on `codex/x-spend-opt`, 8 commits deep — your continuity state says radar tuning is the next thread." The desk anchors orientation; memory annotates it. **Desk-contradiction is the lite-R5 check** *(R5 re-priced):* when the desk plainly contradicts a `volatile`/open-loop claim (the continuity state says "focus: radar tuning" but the desk shows three sessions of billing commits), the contradicted claim is downranked and rendered with a staleness hedge rather than as settled state. This is the cheap n=1 substitute for v1.1's full continuity-claim-invalidation loop — the single user would catch a stale claim in one glance anyway; the desk-contradiction check just front-runs it.

### 8.3 Read-only, fail-open, bounded

No command mutates the repo. The local git reads run only at session boundaries (§8.0) under the desk-specific deadline; any failure (no git, no `gh`, timeout, lock contention) degrades orientation silently to memory-only. Network/`gh` runs only off-path.

### 8.4 Determinism

The desk is live state and therefore not deterministic across real time — but the byte-stability invariant (§9) is, as in v0.6, conditioned on "the same repo state," operationalized as **the same desk snapshot**. Given a fixed snapshot, request context, clock fixture, and continuity-state version, T0/T2 output is byte-identical. Tests fix the desk by injecting a fixture snapshot, exactly as v0.6 fixes the index.

---

## 9. Determinism, caching, and performance

### 9.1 Byte-stability

Carries forward v0.6 §2.7, extended: given the same **desk snapshot, request context, budget, clock fixture, and continuity-state version**, T0/T1/T2 emit byte-identical blocks. The desk snapshot id and the continuity-state `version` are part of the cache key. (Per v0.6's per-device note, cross-device fp drift from the embedding model is acceptable; byte-stability holds on one device.) Byte-stability is about the *content* of the block; placement (§9.5) governs how that block interacts with the harness's own prompt cache.

### 9.2 The per-turn cost

T1 already pays a chunk/vector retrieval per prompt today (v0.6). The friction pre-gate (§6.3) is a cost-saver *only on genuinely empty turns* (bare acks); substantive turns pay the same relevance retrieval the delta path already pays today, so **net per-turn cost is at parity with the current v0.6 delta path, not below it.** (v1.1's framing of the pre-gate as a broad per-turn cost reduction was an overclaim once the gate was narrowed to no-ops, §6.3.)

### 9.3 Performance budgets

Release-gate fixture sizes (warm path), adapting v0.6 §13:

- T0 orientation, 1 000 memories: p95 ≤ 250 ms (continuity/skeleton read is one indexed lookup; the desk read is a bounded synchronous git under the §8.0 deadline, or O(1) once the projection ships).
- T1 routine turn (pre-gate rejects an obvious no-op), 1 000 memories: p95 ≤ 40 ms.
- T1 surfacing turn (five matching entities): p95 ≤ 120 ms.
- T2 re-orientation: p95 ≤ 120 ms.
- Desk read at session boundary: target p95 ≤ 60 ms, hard fail-open well inside the 800 ms hook deadline (§8.0).
- C0 closeout write: p95 ≤ 200 ms; never exceeds the 800 ms hook deadline.

Cold-start (first call after boot) ≤ 600 ms at 1 000 memories, as v0.6.

### 9.4 Observability counters

Extends v0.6 §13.1 additively:

- `recall.orientation_invoked_total`, `recall.reentry_invoked_total`, `recall.closeout_invoked_total`;
- `recall.t1_surfaced_total`, `recall.t1_silent_total` (the silence rate is a primary health metric — a system that surfaces on most turns is mis-gated);
- `recall.friction_gate_rejected_noop_total` (no-op rejections) and `recall.t1_miss_signal_total` (a `memory_search`/`memory_get` shortly after a silent T1 — the relevance/gate-miss precision metric, §6.3);
- `recall.passive_surfaced_total`, `recall.recall_used_total` (the trust/usefulness ratio; both logged out-of-band, §5.3);
- `recall.continuity_degraded_total` (closeouts that registered `degraded`, the B2 health metric) and `recall.desk_read_degraded_total{reason}`.

### 9.5 Injection position and the harness prompt cache *(B8 — re-affirms v0.6)*

Modern harnesses cache the prompt **prefix** — the cache hits only while the early, static portion of the prompt is byte-identical across turns. A per-turn recall block (0–3 memories, content changing every turn) injected **into the cached prefix** would invalidate it every turn, turning each turn into a full uncached re-evaluation — a latency and token-cost blowup, *even though each block is itself byte-deterministic (§9.1)*.

**Contract (already the v0.6 contract, restated for the prose unit):** the **per-turn** trigger (T1, UserPromptSubmit) is delivered as **turn-local conversation context at the uncached tail, adjacent to the latest user prompt** — never by mutating the static system/developer prefix. The **session-boundary** triggers (T0 at SessionStart `startup`, T2 at SessionStart `compact`/`resume`) fire once per segment, before a stable per-turn prefix exists, so they do not repeatedly invalidate a cache; they still must not be spliced into a *shared static system prompt* that persists across sessions.

**Verification result (confirmed — Codex, recorded with the fusion review).** In current Claude Code and Codex, per-turn `UserPromptSubmit` hook context is placed in the **conversation, not the static system prompt** (Claude Code wraps `additionalContext` "inserted into the conversation at the point where the hook fired" and the SDK states conversation-injected content "doesn't affect the system prompt cache"; Codex records the user prompt first and the hook's additional context after it). `SessionStart` context appears "at the start of the conversation, before the first prompt." **So in both target harnesses the B8 defect does not currently bite — and v0.6's 2026-06-19 amendment already mandates the per-turn delta at the uncached tail.** The contract is therefore a **documentary invariant** for the new prose unit plus a Phase-1 placement-assertion test, not new machinery.

## 10. Privacy and safety (Stream D)

Unchanged authority: Stream D owns classification, encryption, and reveal. v3.0 adds two consumption points:

- Every synthesized recollection body passes `safe_plaintext_fragment` before emission; non-`Allow` fragments are dropped, not encrypted into prose.
- Every `StateClaim`/`OpenLoop`/`StagedNote` text passes `safe_plaintext_fragment` before persistence; the continuity-state object and closeout summary are subject to full classification at write time; a closeout summary carrying secret-class content is refused exactly as any write (`SecretRefused`), never silently stored. A write where any fragment is dropped is marked `degraded: true` and must not silently supersede last-known-good as authoritative orientation (R7, §4.4).

`memory_startup`/T0/T1/T2 still never call `memory_reveal` and never emit ciphertext or masked-body projections.

---

## 11. Cross-stream surface changes

Implementation lands these additive surfaces on shipped streams. Like v0.6 §1.1, they are part of this contract. Each is tagged with the phase it lands in.

- **Protocol (Stream A/daemon):**
  - `RequestPayload::Reorient { cwd, session_id, harness }` for T2 + `ResponsePayload` analog, dispatched when the **existing** SessionStart matcher fires with source `compact`/`resume` — **no new hook event** (§3.3). *(Phase 4.)*
  - `RequestPayload::Closeout { cwd, session_id, harness, summary: Option<CloseoutSummary> }` *(N2: corrected spelling)*; SessionEnd added to the matcher tables / `HOOK_EVENTS` — the single new hook wire. *(Phase 2.)*
- **Daemon — out-of-band telemetry endpoint (B1):** a separate, decoupled, asynchronous post-render write endpoint (distinct from the read-path recall requests) recording `PassiveSurfaced`/`RecallUsed` and the staged-note `consumed` clear. Recall responses carry no write side effects. *(Phase 1 plumbing; populated Phase 2/3.)*
- **Daemon — desk read (B4 re-priced):** a bounded synchronous local-git read at session boundaries (§8.0), network off-path. The in-memory `DeskProjection` background refresher is a deferred optimization (§16). *(Phase 4.)*
- **Stream A memory kind:** the reserved `continuity-state` pinned memory per project (§4.2), whole-object supersede (item ids carried for a future item-level merge, B3 re-priced). No schema change — an ordinary memory with a reserved tag. *(Phase 2.)*
- **Stream A events:** new `EventKind::PassiveSurfaced` and `EventKind::RecallUsed` (§5.3), alongside existing `RecallHit`. Written **only** via the out-of-band endpoint, never on a recall read path. *(Phase 1/3.)*
- **Stream C governance:** governs the continuity-state write/supersede like a pass-2 candidate **with no carve-out** (B7). A system-authored continuity update passes `dream_source` confidence gating like any synthesized write; `StateClaim.derived` distinguishes system-derived from user-authored content for R9 framing. *(Phase 2.)*
- **Stream F dreaming:** the continuity-maintenance pass (§5.2) added to the nightly pipeline. *(Phase 2.)*
- **Stream D:** no new surface; consumes existing `safe_plaintext_fragment`. *(Phase 1/2.)*
- **Stream G observability:** the new counters (§9.4) and a trust/usefulness panel (`passive_surfaced` vs `recall_used`), plus a `continuity_degraded` health line. *(Phase 1/2.)*
- **Stream H eval:** parser reads `<recall ref=...>` (§15) and the per-trigger empty-wrapper form (§6.4, N5). *(Phase 1.)*

---

## 12. Invariants (consolidated)

A change failing any of these fails review:

1. **Recall (T0/T1/T2/T3) is strictly read-only** and never reveals ciphertext. No write is *caused by* a recall hook on its synchronous path; `PassiveSurfaced`/`RecallUsed`/note-consume writes are out-of-band (§2 inv 4, §5.3). *(B1; re-affirms v0.6.)*
2. Only `active`/`pinned`, `passive_recall = true`, non-pending-review memories surface as facts (v0.6 §2.3–2.5).
3. Output is byte-stable given desk snapshot + request context + budget + clock + continuity-state version (§9.1).
4. Every surfaced recollection is declarative, attributed, and carries a recoverable `ref`; **system-derived or low-confidence recollections are framed as external evidence, not the agent's own voice** (§2 inv 1–2, §7.3). *(R9.)*
5. No contentless entry is ever surfaced (§7.1).
6. Truncation, if it fires, cuts at a recollection boundary and emits well-formed output (§7.5).
7. **No network call on any synchronous recall hot path; local git runs only at session boundaries (T0/T2) under a fail-open deadline; the per-turn path (T1) spawns no subprocess** (§8.0, §9.3). *(B4, re-priced from v1.0's blanket no-subprocess rule.)*
8. The per-turn trigger (T1) renders at the uncached tail, outside the cached prompt prefix; no recall block mutates a shared static system prompt (§9.5). *(B8; re-affirms v0.6.)*
9. The desk read is fail-open and never blocks recall; a missing/timed-out read degrades to memory-only orientation (§8.3).
10. Silence is a valid output; the per-trigger empty wrapper is always emitted (§6.4).
11. Cold-start never degrades to a recency dump (§6.5). *(R2.)*
12. **The continuity-state object is a governed, classified canonical memory with no governance carve-out — never a private store, never carrying secret/unreviewed content, and never rendered as authoritative when degraded/hollow** (§4.0, §4.2, §4.4). *(B2, B7, R7.)*
13. Closeout is fail-open, passes the B2 acceptance gate, and never blocks the harness beyond the hook deadline (§5.1). *(B2.)*

---

## 13. Phased build plan

Each phase is independently shippable and testable; value lands before the whole is built. **v3.0 re-sequences v1.1** (Option B for n=1): ship the noise fix, then the continuity engine, then relevance, then desk. There is **no adoption-gate** between phases — the single user (developer + dogfooder + evaluator) decides what to build next by using it.

**Phase 1 — Rendering, budget, and injection safety (stops the noise).** Prose `<recall>` unit (§7.1); byte-budgeting (§7.4); entry-boundary truncation (§7.5); discoverable guidance string without card-catalog over-advertising (§3.4, N1); retire the `<memory>`/empty-snippet contract; per-trigger empty-wrapper form (§6.4, N5); **uncached-tail placement** confirmed per harness (§9.5, B8); **external-evidence framing** for system-derived/low-confidence recollections (§7.3, R9); the **out-of-band telemetry endpoint** plumbing (§5.3, B1); shrink T0 to high-signal. No new triggers beyond plumbing, no continuity object yet, no writes on the read path. **Scope (R8): this phase _stops the noise_ — it does not by itself make the channel "trustworthy"** (relevance, continuity, and desk land later).

**Phase 2 — The minimal continuity engine (remembered, not recent).** The `ContinuityState` object (§4) with whole-object supersede and stable item ids carried (B3 re-priced); the **SessionEnd closeout (C0) hook** + `RequestPayload::Closeout` writing the cheap deterministic layer (§5.1); the **B2 substance/acceptance gate** with a two-state rich-vs-hollow test (§4.0); dream-time continuity maintenance (§5.2) with `SystemSynthesized` marking; **no governance carve-out** (B7); R9 framing for synthesized claims; staged notes. T0 reads the continuity model (held back to skeleton fallback when degraded). This is the *remembered-not-retrieved* inversion — the conceptual payoff, pulled forward.

**Phase 3 — Relevance and gating (makes T1 real).** Add the relevance-to-cue term reusing the v0.6 RRF lane, with testable ranges (§6.2, N4); the **narrowed friction pre-gate** (no-ops only) plus the **miss-signal** so the gate is debuggable (§6.3, B6); independent `lesson_boost` path (N6); conversation-context dedup with turn-distance and per-context subagent scope (§6.4, N3); silence-as-output; the **relevance floor permissive by default, tuned by dogfooding** (§6.4, R4), calibrated against the **structural-only base** since the passive path emits no inclusion `RecallHit` (R6); **out-of-band positive-only `PassiveSurfaced`/`RecallUsed` logging for measurement** (§5.3, B1/B5).

**Phase 4 — Desk and re-entry (anchor on the one persistent channel).** The bounded **synchronous local-git desk read** at session boundaries, network off-path (§8.0, B4); **desk-contradiction lite-R5** downranking stale continuity claims (§8.2, R5); defined **cold-start** (§6.5, R2); the T0 desk-first join (§3.1); T2 re-orientation on SessionStart(compact)/(resume) rendering the **current absolute desk state** (§3.3, R10). *(The daemon-cached desk projection remains a deferred optimization, §16 — build it only if a session start stalls.)*

---

## 14. Open questions (decisions needed before/while building)

The fusion review and the n=1 reframing settled most v1.0/v1.1 open questions; those are marked **resolved**. The rest are genuinely open.

1. **Re-orientation hook across harnesses.** *(Resolved — verified.)* `PreCompact` is block-only with no `additionalContext`; both harnesses expose post-compaction re-entry via `SessionStart` source `compact`. T2 rides SessionStart(compact)/(resume) with no new hook wire (§3.3).
2. **Relevance floor calibration.** *(Approach resolved — R4 re-priced; value still empirical.)* Ships permissive, tuned by dogfooding. No labeled corpus is built for n=1.
3. **`RecallUsed` usability.** *(Resolved on the safe side — B5.)* `memory_get`-on-ref is logged **positive-only**; disuse is never inferred from its absence. A *validated softer disuse signal* is a prerequisite before any decay ships (§5.3, §16).
4. **Continuity-state merge semantics.** *(Resolved for n=1 — B3 re-priced.)* Whole-object last-writer-wins supersede ships now; stable item ids are carried so item-level merge is an additive build if concurrent-closeout clobber ever proves real in dogfooding (§4.2, §16).
5. **Closeout authorship.** *(Decided — agent-authored with auto-snapshot fallback.)* The agent authors the summary; the auto-snapshot fallback is by construction `degraded` and so cannot become authoritative (B2). Open sub-question: the lightest ergonomic convention for the agent to author the summary at SessionEnd — a structured closeout field vs reuse of `memory_note`. Decide during Phase 2.
6. **The B2 substance threshold.** *(Open — needs teeth in Phase 2.)* The exact "substantive" bar (non-empty `volatile` / minimum open-loop evidence) needs a concrete, testable definition before C0 is wired. This is the single most important Phase-2 design detail, since it is the lone gate on the whole continuity engine.

---

## 15. Acceptance signals

Implementation of a phase is complete when its tests/docs exist and pass. Per phase (re-sequenced for v3.0):

- **Phase 1 (rendering, budget, injection safety):** `recall_render` tests assert the `<recall>` prose unit, no empty elements, byte-budgeting (a fixture that overflowed under summary-only budgeting now selects correctly), entry-boundary truncation (a forced-overflow fixture emits well-formed output), the per-trigger empty-wrapper exact form (N5), the guidance string names `memory_search`/`memory_get` without card-catalog phrasing (N1), and **external-evidence framing** — a system-derived/low-confidence fixture renders with a third-party lead-in, a user-authored high-confidence one does not (R9). A **placement test** confirms T1 output is at the uncached tail for each harness (B8). A **telemetry test** asserts the out-of-band endpoint exists and the read path performs no write (B1). Stream H eval parser updated to `<recall ref=...>` with a passing regression. *(Claim scope: "stops the noise," R8.)*
- **Phase 2 (continuity engine):** `continuity_state` tests assert closeout writes/supersedes the pinned object; the **B2 acceptance gate** rejects a hollow/degraded snapshot as authoritative (a fresh-timestamped empty auto-snapshot does not become orientation, and registers `degraded`); whole-object supersede converges under the two-clone equality check (B3 re-priced — item ids present but item-level merge not yet required); governance gates a system-authored update with **no carve-out** (B7); dream-time refines the object deterministically and marks claims `SystemSynthesized`; staged notes surface once then clear (the clear is an out-of-band write, B1); the object passes governance + `safe_plaintext_fragment` (a secret-class closeout summary is refused; a partial-drop write is marked `degraded` and does not supersede last-known-good, R7). T0 renders the continuity model when substantive and the skeleton fallback when degraded.
- **Phase 3 (relevance and gating):** `recall_gating` tests assert the pre-gate rejects **only** obvious no-ops (a substantive friction-word-free prompt still reaches the relevance gate, B6), surfacing on relevant fixtures, silence when nothing clears the (permissive) floor (R4), the relevance term changes ordering vs structural-only (calibrated on the structural-only base, R6), `lesson_boost` fires independent of the pre-gate (N6) within its testable range (N4), dedup suppresses an already-surfaced ref **but allows re-surfacing past the turn-distance threshold** (N3) with per-context subagent scope, and the **miss-signal** emits correctly. A telemetry test asserts `PassiveSurfaced`/`RecallUsed` are positive-only and `memory_get`-absence never decrements strength (B5). Determinism test extended to the cue path.
- **Phase 4 (desk and re-entry):** `desk_read` tests assert the session-boundary read spawns **no network call** and **no per-turn subprocess** (B4), runs local git under the fail-open deadline and degrades to memory-only on timeout/lock (§8.0), byte-stability given an injected fixture snapshot, **cold-start** emits desk-or-empty and never a recency dump (R2), the T0 join with the continuity model, **desk-contradiction downranks a stale continuity claim** (lite-R5, §8.2), and T2 renders **current absolute desk state** with no stored baseline (R10).
- Docs: `docs/api/stream-e-ambient-recall-api.md`; updates to the Stream A/C/F/G/H API docs for the §11 surfaces; `CLAUDE.md` authoritative-docs table repointed; `STREAM_E_POLICY` bumped from `stream-e-v0.6` to `stream-e-v3.0` — all only after the relevant phase's tests pass.

## 16. Explicit deferrals (re-priced enhancements — build only if real usage warrants)

- **Item-level cross-device continuity merge** *(B3 re-priced)*: whole-object supersede ships now; item-level IDs are already carried (§4.1), so item-wise union/version reconciliation + a two-device concurrent-closeout merge test is an additive build, taken on only if dogfooding shows concurrent-closeout clobber actually bites.
- **The daemon-cached `DeskProjection` + background refresher** *(B4 re-priced)*: the in-memory git/desk projection read O(1) on the hook (reusing the reality-check/embedding-worker idioms + `Substrate::watch()`), built only if a synchronous session-boundary desk read ever stalls. Until then, §8.0's bounded synchronous read stands.
- **A validated softer disuse signal** beyond `memory_get`-on-ref (next-turn n-gram overlap, file-touch correlation) — a hard prerequisite for any strength decay/re-weighting; `memory_get`-absence is never a disuse signal (B5, §5.3).
- **Forced-sampling deterministic self-test for T1 precision** *(B6 re-priced)*: the ~1%-keyed-by-session-id forced-surfacing mode for measuring gate precision without a human watching — unneeded while the single user is the evaluator; returns if multi-user does.
- **A labeled "should-this-surface?" corpus** *(R4 re-priced)*: replaced by dogfooding for n=1; returns for multi-user calibration.
- **Embedding-centroid topic-drift** as a Tier-2 friction salience signal (§6.3) — once per-turn cost is measured.
- **Claim-level provenance hardening beyond `derived`** *(B7 re-priced)*: the full system-vs-user governance-distinguishability surface, beyond the `StateClaim.derived` flag + R9 framing that ship now.
- **Cross-session real-time continuity merge UI** (Stream I surface).
- **A daemon-cached doctor projection in `<pending-attention>`** (inherited deferral from v0.6).

If a phase's acceptance tests cannot pass without one of these, revise this spec before coding continues. Note the asymmetry deliberately: every item here is an enhancement whose absence is *safe* at n=1 — the one continuity hazard that is *not* safe to defer (B2, confident-wrongness poisoning orientation) is the one prerequisite kept as a hard Phase-2 gate.
