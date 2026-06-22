# Stream E — Ambient Recall (Passive Memory Redesign) v1.1

**Status:** Draft for review. Not yet an accepted implementation contract. This document proposes a ground-up redesign of the Stream E passive-recall surface and supersedes the *approach* of `stream-e-passive-recall-v0.5.md` while reusing much of its machinery. On acceptance, the Authoritative-documents table in `CLAUDE.md` and the `STREAM_E_POLICY` version string should be repointed here; this draft does not mutate either.

**Date:** 2026-06-22.

**Authors:** Claude, from a design session with Trey.

**Sources:** the live Stream E contract (`stream-e-passive-recall-v0.5.md`), the shipped Stream A–I surfaces, the conceptual walkthrough at `docs/explainers/2026-06-22-ideal-agent-memory-hooks.html`, the cap-recent-memory fix (`500a60d`), two ground-truth code recon passes (2026-06-22) of the hook wiring, delta path, dreaming, dynamics/strength, and confidence assignment, and the cross-vendor fusion review at `docs/reviews/2026-06-22-ambient-recall-v1-fusion.md`.

**Non-source:** older Stream E drafts (v0.1–v0.5) are historical except where they describe still-shipped machinery this spec explicitly reuses.

**Policy string:** on acceptance, the version string in policy/manifest/recall-block attributes bumps to `stream-e-v1.1`.

---

## Revision goal (v1.1)

v1.1 incorporates a cross-vendor fusion review of v1.0 (four model families + an external judge; verdict **RATIFY-WITH-CORRECTIONS**; full record at `docs/reviews/2026-06-22-ambient-recall-v1-fusion.md`). v1.0's diagnosis was upheld and its rendering/budget/relevance wins preserved, but its **center of gravity and build order were wrong, and several mechanisms were unsafe as specified.** v1.1 changes the contract in three ways:

1. **Architecture pivot — defer the continuity engine (Option A).** v1.0 made the maintained `ContinuityState` object the spine and shipped it in Phase 3. v1.1 ships the **continuity-free channel first** (desk-first orientation + relevance-gated per-turn recall + prose rendering), proves adoption, and **defers** the `ContinuityState` object, the closeout (C0) hook, dream-time continuity maintenance, and use-driven decay until their hazards are specified away. As written, those mechanisms inverted the failure mode from v0.5's *noisy irrelevance* (which the agent learns to ignore) to *confident wrongness* (which the agent learns to trust because it is prose, attributed, small, and desk-anchored). The continuity engine survives in this document as the eventual ambition (§4–§5), now gated behind hard prerequisites (§4.0).

2. **Safety corrections (apply regardless of architecture).** Recall stays **strictly read-only** — telemetry/consume writes move out-of-band, never on the read path (§2 inv 4, §5.3). The desk read becomes a **daemon-cached background projection** read in O(1), not synchronous git/`gh` subprocesses on the hot path (§8). `memory_get`-on-ref is a **positive-only** signal; disuse is never inferred from its absence (§5.3). The friction pre-gate is **narrowed to obvious no-ops** and made observable (miss-signal + deterministic self-test) so it is falsifiable (§6.3). Dynamic per-turn recall blocks **render at the bottom of context** to preserve the harness prompt cache (§9.5). Cold-start is defined (§6.5). T2 renders **current absolute desk state**, not a delta (§3.3, §8). Privacy partial-drops carry a degraded flag and never silently supersede (§4.4, §10).

3. **Build order.** The phase plan (§13) is re-sequenced: cross-cutting safety fixes → rendering/budget → relevance/gating → desk & re-entry → (gated) continuity engine.

Each correction is tagged in-line with its fusion-finding id (B1–B8, R2–R10, N1–N7) for traceability against the review. Where v1.0 prose still says "v1.0," read it as "this redesign" unless a v1.1 note overrides it.

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

And the place the analogy *breaks*, which is the most important part: a human walks in with a faded yesterday and can reconstruct from a continuous underlying memory; an agent walks in with **nothing**. If the block is noise, the agent isn't under-oriented, it's amnesiac. So the externalized state has to carry *more* load than a human's morning brain, not less.

### 0.4 Vision and objectives

Passive recall should feel like continuity — the thing a good colleague has walking back in Monday morning ("right, we were on X, Trey cares about Y, last time Z bit us"), then the specifics arriving exactly as they become relevant.

Concrete objectives:

- **O1 — Relevance over recency.** Most recall fires on what matters *now*, not on the clock. This bundles two distinct relevances with different signals and failure modes, and v1.1 keeps them separate *(N7)*: **(a) cue-relevance** — T1/T2 surface on a semantic/lexical match to the live cue (the user's message and the work in front of the agent); **(b) project-salience** — T0 orients by stakes/consequence ordering over a small set of pinned project memories (and, once built, the continuity model), with no user cue to match. Ranking gains a cue-relevance term it currently lacks entirely (§6.2); orientation ordering is salience-by-stakes (§3.1).
- **O2 — Conclusions in the body; pointer in the attribute for traceability.** *(N1)* The injected unit is a self-contained recollection — the *lesson*, in prose — that the agent can act on without dereferencing anything. It also carries a `ref` attribute purely for provenance/traceability (an invariant requirement; the Stream H parser needs it), not as a handle the agent is expected to fetch. The `ref` is an affordance for "go deeper," never a substitute for a usable body; the surface must not advertise `memory_get <ref>` so insistently that it trains card-catalog (fetch-to-understand) behavior.
- **O3 — Gate, don't truncate.** A high relevance bar decides what surfaces; silence is a valid and frequent output. Token efficiency is a *consequence* of the bar, never the optimization target.
- **O4 — Continuity is maintained, not queried *(eventual ambition — deferred in v1.1)*.** A continuity-state object written at closeout, refined at dream-time, and read back at orientation is the eventual shape (§4–§5). v1.1 **does not ship it** until its hazards are specified away (§4.0); the shipping channel reaches "remembered, not just recent" via desk-first orientation + relevance, with an optional single pinned focus note as the minimal bridge.
- **O5 — Trust is the currency.** Precision (few items, all relevant) earns the agent's trust so it leans on the channel; volume destroys it. A channel the agent distrusts is worse than no channel. v1.1 weights this objective heavily: a *confidently wrong* recollection costs more trust than a noisy-but-ignorable one, which is the core reason the continuity engine is deferred until its poisoning vectors are closed.
- **O6 — Safety preserved.** Read-only (strictly — no write is *caused by* a recall hook; telemetry is out-of-band, §2 inv 4/§5.3), no encrypted plaintext, governance-authoritative, deterministic and cache-stable — every Stream E v0.5 safety invariant carries forward, plus new injection-safety invariants for an inherently larger prompt-injection surface.

### 0.5 Goals (measurable)

- Median orientation (T0) block ≤ 300 estimated tokens; hard cap ≤ 600. Never truncates mid-entry.
- Per-turn (T1) recall surfaces nothing on a clear majority of routine turns (silence is the common case); when it surfaces, ≤ 3 recollections by default.
- Zero contentless entries: a recollection with no usable body is not surfaced.
- Injected recollections read as declarative attributed facts, never as imperatives; system-derived or low-confidence recollections are framed as *external* evidence ("A prior memory reports…"), not the agent's own voice *(R9)*.
- Output is byte-stable given the same repo (desk-projection) state, request context, clock fixture, and — once the continuity engine ships — continuity-state version (carries forward v0.5 §2.7, extended §9.1).
- No write is performed on a recall hot path: T0/T1/T2 are read-only; `PassiveSurfaced`/`RecallUsed`/note-consume writes happen out-of-band *(B1)*.

### 0.6 Non-goals

- Owning model inference. Synthesis reuses the Stream F dream pipeline, which shells to the user's own harness CLI.
- A second persistence layer. The continuity-state object is a canonical Stream A memory, governed by Stream C and classified by Stream D — not a private store.
- Replacing deliberate recall. `memory_search` / `memory_get` remain the conscious "go dig" path; this spec makes them *discoverable*, not redundant.
- Cross-harness session-transcript capture. Closeout consumes a bounded, agent- or harness-supplied summary, not a full transcript.

### 0.7 What this keeps, and what it changes

**Keeps (reuses existing machinery):** the `recall hook` dispatch handler and three wired lifecycle events; the per-turn delta path (`build_delta_response`); project/session binding and namespace resolution (v0.5 §4); candidate collection over the indexed `MemoryQuery` extension (v0.5 §6); the deterministic structural ranking core (v0.5 §8.2) as the *base* score; Stream F dreaming's three-pass pipeline and lease; the dynamics/strength ranking term; Stream D privacy helpers; the recall-explanation/omission accounting and observability counters (v0.5 §3.3, §13.1).

**Changes (net-new or reshaped).** The "ships in v1.1" column marks what is part of the accepted build vs. deferred behind §4.0 prerequisites:

| Area | v0.5 (today) | v1.1 target | Ships in v1.1? |
| --- | --- | --- | --- |
| Startup content | recency-ranked atom dump | desk read (cached projection) + pinned skeleton, salience-ordered prose; optional single pinned focus note (§3.1, §8) | **Yes** (continuity-state model deferred, §4.0) |
| Per-turn trigger | delta on every prompt | friction/relevance-gated; silence is valid (§3.2, §6) | **Yes** |
| Re-entry / compaction | not handled | new T2 trigger rendering current absolute desk + immediate focus (§3.3) | **Yes** |
| Injected unit | `<memory>` title + empty snippet | prose recollection, ref preserved as attribute (§7) | **Yes** |
| Ranking | no relevance-to-now term | adds a cue-relevance term (§6.2) | **Yes** |
| Budgeting | summary tokens only | full rendered-byte cost (§7.4) | **Yes** |
| Truncation | cuts at any newline → malformed XML | cuts at entry boundary, always well-formed (§7.5) | **Yes** |
| Tool discoverability | generic guidance string | guidance names `memory_search`/`memory_get` (§3.4) | **Yes** |
| Injection position | unspecified | dynamic blocks rendered bottom-of-context (§9.5) | **Yes** *(B8)* |
| Telemetry/feedback | inclusion counts (`RecallHit`) | out-of-band `PassiveSurfaced`/`RecallUsed` logging, positive-only (§5.3) | **Partial** — positive-only logging yes; disuse-decay deferred |
| Continuity model | none | maintained `ContinuityState` written at closeout, refined at dream-time (§4–§5) | **Deferred** *(§4.0; B2/B3/B7/R5)* |
| Closeout | no surface exists | SessionEnd hook writes continuity state (§5.1) | **Deferred** *(§4.0)* |

---

## 1. Conceptual model

### 1.1 Three data types

Passive recall operates over three distinct data types. Conflating them is the original design error.

1. **The continuity-state model** *(deferred — §4.0)* — a small, maintained, synthesized object per project: a slow-changing *skeleton* (what the project is, its architecture, hard constraints), a *volatile* layer (current focus, what's hot, what's blocked), *open loops* (unresolved threads ranked by stakes), and *staged notes* (prospective reminders left deliberately for future-me). Written at closeout and dream-time; read at orientation and re-entry. §4. **v1.1 does not ship this**; until it is earned, the shipping channel substitutes the pinned skeleton (existing `me`/`invariant`/`state`/`decision` memories) plus an optional single human- or agent-authored **pinned focus note** for "current focus."

2. **Recollections** — gist units. A recollection is one self-contained, declarative proposition with a consequence — the lesson — derived from a canonical memory, rendered as prose, carrying provenance (`ref`, `kind`, `confidence`). Recollections are what T1 surfaces. §7.

3. **The desk** — live repo state served from a **daemon-cached projection** (§8): branch, uncommitted-file summary, recent commits, and (best-effort, off-path) open PR / CI. Not memory; *context*. Read in O(1) on the hot path (never by spawning git/`gh` synchronously, *B4*) and joined with memory at orientation/re-entry so recall reflects what the session is probably about. §8.

### 1.2 The four triggers

| | Trigger | Lifecycle event | Analogy | Status |
| --- | --- | --- | --- | --- |
| **T0** | Orientation | SessionStart (`startup`) | walking into the office | reshape existing |
| **T1** | Associative recall | UserPromptSubmit (+ SubagentStart) | remembering while you work | reshape existing |
| **T2** | Re-orientation | SessionStart (`compact`/`resume`) | coming back after a gap | reuses existing matcher; new response variant (**not** PreCompact, §3.3) |
| **T3** | Deliberate recall | `memory_search` / `memory_get` | consciously trying to remember | exists; fix discoverability |

Plus one non-injection trigger that closes the loop *(deferred with the continuity engine, §4.0)*:

| | Trigger | Lifecycle event | Role | Status |
| --- | --- | --- | --- | --- |
| **C0** | Closeout | SessionEnd / Stop | write the continuity state | **net-new hook — deferred (§4.0)** |

### 1.3 The lifecycle loop

```
   work ──▶ closeout ──▶ dream-time ──▶ orientation ──▶ work
 (T1/T2)   (C0: snapshot   (refine model,   (T0: read it
            + stage note)    decay/boost)      back, + desk)
```

The session's end writes its successor's beginning; the quiet hours in between maintain the model and compost the noise. Forgetting is a feature: a recollection surfaced repeatedly and never *used* is noise the system hasn't yet learned to stop surfacing (§5.3).

**v1.1 status of the loop.** This full loop is the eventual target, **not** what v1.1 ships. v1.1 ships the right-hand arm only — **orientation from the desk + pinned skeleton, refreshed each session** — without the `closeout → dream-time` write arm. The continuity engine is deferred until its prerequisites (§4.0) are met, because the closeout-writes-startup arm, as specified in v1.0, can poison startup orientation with confidently-wrong state (B2), corrupts across devices without item-level merge (B3), and creates an un-governed write path (B7). Until then, "remembered, not retrieved" is approximated by desk-first orientation plus the optional pinned focus note (§3.1).

---

## 2. Safety invariants

All Stream E v0.5 §2 invariants carry forward unchanged: recall is read-only; no encrypted plaintext in recall; governance lifecycle is authoritative; tombstoned/superseded records do not teach; candidates/quarantines are attention, not truth; the token estimator is deterministic (`ceil(utf8_byte_len / 4)`); output is byte-stable for cacheability; errors are typed.

This spec adds:

1. **Recollections are declarative and attributed, never imperative.** An injected memory ("Always do X") is reframed as reported fact ("Recalled — the standing practice has been X") before emission. The existing `neutralize_imperative_prose` path is the mechanism; v1.1 makes attributed-declarative the contract, not a best-effort. Rationale: injected memory is a prompt-injection surface, and the more it reads as the agent's own voice, the more a poisoned memory costs.
2. **System-derived and low-confidence recollections are framed as external evidence, not internal fact.** *(R9 — new in v1.1.)* Declarative rephrasing alone is insufficient injection safety: a poisoned memory still steers as a *reported fact* ("the standing practice has been to run script X"). Any recollection that is system-synthesized (not directly user-authored) or below a confidence threshold must be rendered as third-party evidence the agent should weigh — "A prior memory reports…", "An earlier note claims…" — never as a settled fact in the agent's own voice. The framing is part of the rendered-byte determinism tuple.
3. **Provenance is always recoverable.** Every surfaced recollection carries a `ref` to its canonical memory. The agent (and an injection detector) can always trace a recollection to its source; recall never emits free-floating instructions.
4. **Recall is strictly read-only — no write is *caused by* a recall hook.** *(B1 — new in v1.1.)* T0/T1/T2 perform zero disk/event mutations on the synchronous read path. Telemetry (`PassiveSurfaced`, `RecallUsed`) and any note-consume state-change are executed **out-of-band** — either harness-driven, or via a separate, decoupled, asynchronous post-render daemon endpoint — with no side effect on, and no ordering dependency from, the read path. This preserves concurrency safety, testability, and prompt-cache stability; the option of "narrowing the invariant to permit deterministic recall-time telemetry writes" is explicitly rejected (§5.3).
5. **The desk read is read-only and fail-open.** Serving git/desk state never mutates the repo and never blocks recall; the hot path reads a cached projection (§8) and any staleness/failure degrades silently to memory-only orientation (§8.3).
6. **The continuity-state object is governed and classified like any memory** *(applies when the continuity engine ships — §4.0)*. It is written through the Stream A write path, passes Stream C governance with **no carve-out** (B7), and is classified by Stream D. It must never embed encrypted plaintext, secret-class content, or unreviewed candidate claims (§4.4).
7. **Closeout is read-mostly and bounded** *(applies when the continuity engine ships — §4.0)*. The SessionEnd hook writes at most the continuity-state memory and staged notes through the governed write path; it never bulk-imports a transcript and never blocks harness shutdown beyond its deadline (§5.1).

---

## 3. The four triggers

Each trigger is dispatched by the existing unified `recall hook` handler (`cli/recall_hook.rs`), which maps a `hook_event_name` to a daemon request. v0.5 wires three events (SessionStart, UserPromptSubmit, SubagentStart) via `hooks_wire.rs`. **v1.1 adds no new hook wire for its shipping set:** T2 re-orientation rides the *existing* SessionStart matcher (`startup|resume|clear|compact`) on the `compact`/`resume` sources (§3.3) — it is a new response variant, not a new event. **PreCompact is deliberately not used** (it is block-only, with no context-injection surface in Claude Code or Codex; §3.3, §14.1), and **SessionEnd (C0) is deferred** with the continuity engine (§4.0). The net new hook-wiring in v1.1 is therefore zero; the only future hook wire is SessionEnd, when the continuity engine ships.

### 3.1 T0 — Orientation (SessionStart)

**Fires:** SessionStart (existing matcher `startup|resume|clear|compact` for Claude; matcher-free for Codex).

**Reads (v1.1 shipping set):**
- the **skeleton**: pinned/active `me` identity + project `invariant`/`state`/`decision` memories (the existing `<identity>` / `<project-state>` candidate sources), salience-ordered by stakes;
- the optional single **pinned focus note** for "current focus" (a normal pinned memory via `memory_note`, agent- or human-authored), if present;
- the **desk** (§8): branch, uncommitted-file summary, recent commits, and best-effort PR/CI — all read from the daemon-cached projection, never by spawning subprocesses on this path *(B4)*.
- *(deferred)* the project's **continuity-state model** (§4), once the continuity engine ships; until then T0 does **not** read it.

**Cold-start (R2).** When the inputs above are empty — first session in a project, no pinned skeleton, no focus note, no/empty desk — T0 emits **desk-only orientation (if any) plus pinned identity/invariants, and otherwise the empty wrapper.** It never falls back to a recency dump; a recency-ranked atom list is exactly the v0.5 failure mode this redesign exists to kill. See §6.5.

**Injects:** a glance — what's nagging, where we left off, the desk crossed with memory. Leads with **unresolved × consequential** (project-salience, O1b), not latest. Flags recent pivots that override a stale assumption ("priority changed last session"). Renders as prose recollections (§7), not an atom list.

**Budget:** target ≤ 300 estimated tokens, hard cap ≤ 600 (replaces `HOOK_STARTUP_BUDGET_TOKENS = 1900`). Never truncates mid-entry; if over budget, drops lowest-salience items and records them as omissions.

**Net-new vs today:** desk projection read, prose rendering, salience-by-stakes ordering, the smaller budget, defined cold-start. The candidate-collection and namespace machinery is reused. (Continuity-state read is deferred, §4.0.)

### 3.2 T1 — Associative recall (UserPromptSubmit, SubagentStart)

**Fires:** UserPromptSubmit (and SubagentStart, with the subagent's task as cue). Maps to the existing `Delta` request path (`build_delta_response`, `passive: true`).

**Cue:** the submitted message + a bounded window of recent conversation/tool state (§6.1). This is the spreading-activation input.

**Gating — the heart of the change (§6):**
- A cheap **friction pre-gate** suppresses only **obvious no-ops** *(B6)* — bare acknowledgements ("yes", "ok", "do that", "thanks") and empty/trivial prompts. It is *not* the primary relevance filter: every substantive prompt proceeds to the relevance gate **regardless of whether it contains friction words**. (v1.0's pre-gate gated *all* surfacing on lexical friction signals, silently starving procedural reuse ("same fix in billing"), social ("reply to Adam"), and status continuations — and because the gate is unlogged, that false-negative was unfalsifiable. §6.3 narrows the gate and adds observability.)
- A **relevance gate** then admits only recollections whose activation clears a high bar. If none clear it, inject nothing — and that is the system working correctly.
- **Lessons** (`feedback`/correction memories) get a salience boost (`lesson_boost`) when the cue carries a decision/difficulty signal — protective recall, surfaced exactly when the agent is about to do the hard thing again. The boost has an **independent path**: it is *not* gated on the friction pre-gate firing, so a missed lexical decision-point does not also kill protective recall when the relevance gate would otherwise select the lesson *(N6)*.
- Dedup against what is already in context this session (§6.4): never re-surface a recollection already shown (subject to the turn-distance threshold, N3), and never restate what the native memory head already carries.

**Injects:** ≤ 3 recollections by default, as prose conclusions (§7), rendered bottom-of-context (§9.5).

**Budget:** ≤ 360 estimated tokens (reuses `HOOK_DELTA_BUDGET_TOKENS`), but spent on the *margin* — usually far under, often zero.

**Net-new vs today:** the narrowed friction pre-gate, the relevance term in ranking (§6.2), conversation-context dedup, silence-as-valid-output, prose rendering, bottom-of-context placement. The delta retrieval path and budget constant are reused.

### 3.3 T2 — Re-orientation (post-compaction / resume SessionStart) — reuses the existing matcher

**Fires:** **`SessionStart` with source `compact`** (the post-compaction session start), and `SessionStart` source `resume` after a long idle gap. **Not `PreCompact`** — see the harness note below. The `compact`/`resume` tokens are already in the existing SessionStart matcher (`startup|resume|clear|compact`), so T2 needs **no new hook wire**; it is a new *response variant* (`RequestPayload::Reorient`) dispatched when SessionStart fires with the compact/resume source.

**Reads:** the immediate sub-task — the rolling session focus and the most recent relevant recollection — not the whole project. Finer than orientation, more local than per-turn.

**Injects:** "here's what we were *just* doing" — a single compact re-orientation recollection plus the **current absolute desk state** (the live cached projection: branch, dirty-file summary). Delivered via the SessionStart(compact) `additionalContext` surface, i.e. at the start of the post-compaction conversation segment (a session-boundary trigger, not a per-turn one — §9.5).

**No desk delta** *(R10).* v1.0 specified "the live desk delta since the session started," which requires persisting a T0 baseline somewhere — on disk (second-persistence-layer violation), as a memory (read-only-write violation), or in-process (stateful daemon that drifts on restart/crash). v1.1 renders the **current absolute desk state** instead. The baseline disappears, and both the no-second-persistence and read-only invariants hold; "what changed" is something the agent can see from the desk itself plus its own retained context.

**Budget:** ≤ 200 estimated tokens.

**Harness note (verified — §14.1).** `PreCompact` is **not a context-injection surface** in either Claude Code or Codex: it is a pre-compaction event that can only *block* (exit code 2 / `decision: "block"` / `continue: false`) and exposes no `additionalContext` output shape (Claude Code Hooks docs; OpenAI Codex Hooks docs). Re-orientation context must therefore be injected at the *post*-compaction boundary, which both harnesses expose as `SessionStart` with source `compact` — Codex's documented SessionStart sources are exactly `startup|resume|clear|compact`, and Codex source records SessionStart additional contexts as conversation items. So T2 rides SessionStart(compact) on both harnesses; PreCompact is not used. (If a future need arises to act *before* compaction, that is a separate block-only hook, not this trigger.)

**Rationale:** context compaction is exactly the "got interrupted, need to re-orient to the immediate task" moment, and today nothing fires there. This is the cheapest high-value net-new trigger — and on the corrected wiring it costs no new hook event.

### 3.4 T3 — Deliberate recall (the tools)

`memory_search` (`{ query, limit, include_body }`) and `memory_get` (`{ id, include_provenance }`) already exist (`mcp.rs`). v1.1 changes one thing: **discoverability.** The `guidance` string returned by T0/T1 (today the generic `"Memorum passive recall assembled from read-only index projections."`) names the tools and notes that a recollection's `ref` *can* be dereferenced for full provenance — e.g. *"Recollections are self-contained; to go deeper, `memory_get <ref>` returns a recollection's full source, and `memory_search` queries memory directly."* The framing keeps the affordance available without implying the agent must fetch to understand a recollection *(N1)* — the body already carries the conclusion (O2). This makes the passive/deliberate split legible without training card-catalog behavior.

---

## 4. The continuity-state object — DEFERRED (the eventual ambition)

> **v1.1 deferral.** This section is the eventual target, **not** part of the accepted v1.1 build. v1.0 made the continuity-state object the spine and shipped it in Phase 3; the fusion review found that, as specified, it converts the failure mode from ignorable noise to trusted-but-wrong orientation. v1.1 ships the continuity-free channel first (§3.1, §6, §7, §8), proves adoption, and builds this object only after the §4.0 prerequisites are met. The design below stands as the contract for **when** it is built.

Today nothing maintains a "where we left off" or project-state summary — the `<project-state>` block is just the project identity binding (`project_body` emits only the project id + namespace). The maintained model below is how that gap is eventually closed; in the interim the shipping channel substitutes the pinned skeleton + an optional pinned focus note (§1.1, §3.1).

### 4.0 Prerequisites before the continuity engine may ship (hard gate)

The continuity-state object, the closeout (C0) hook, dream-time continuity maintenance (§5.2), and use-driven decay (§5.3) **must not ship** until **all** of the following are specified, implemented, and gated by tests. Each maps to a fusion blocker/risk:

1. **Out-of-band writes (B1).** All continuity/telemetry writes triggered around recall go through the decoupled post-render write path (§2 inv 4) — never the synchronous read path. *(This one is implemented early because the continuity-free channel needs `PassiveSurfaced` logging; it is listed here because the continuity engine depends on it.)*
2. **Acceptance gate on closeout orientation (B2).** T0 must refuse to render a degraded/empty continuity-state as authoritative. A timestamp/`updated_at` freshness gate is **insufficient** — the v1.0 fail-open auto-snapshot writes a *fresh* timestamp on a *hollow* object. Required instead: a completeness/quality contract (non-empty `volatile` / minimum open-loop evidence, or an explicit `degraded: true` flag), a desk-contradiction cross-check, and "remembered, not verified" surface framing. A two-state test (rich vs. hollow closeout) gates this.
3. **Item-level cross-device merge (B3).** A single monolithic continuity-state memory superseded concurrently on two clones is a merge graveyard. Required: stable item-level IDs on `open_loops`/`staged_notes`, explicit consume events, and version reconciliation, with a **two-device concurrent-closeout merge test** as the gate. "Confirm it fits canonical-content equality" is not sufficient.
4. **No governance carve-out (B7).** System-authored continuity updates must pass Stream C `dream_source` confidence gating like any synthesized write — **no exemption** (this reverses v1.0 §11's proposed carve-out, which would let a dream-synthesized hallucination become pinned authoritative orientation, violating invariant #7). Claim-level provenance must distinguish system-derived from user-authored content in the surface.
5. **Continuity-claim invalidation (R5).** Continuity-state claims must participate in a staleness/contradiction-invalidation loop (desk mismatch, superseded refs), so a wrong "current focus" decays instead of living forever. The disuse signal (§5.3) covers recollections, not orientation claims; this is a separate mechanism.

Until all five hold, T0 does not read a continuity-state object and C0 is not wired.

### 4.1 Shape

```rust
struct ContinuityState {
    project: String,              // canonical project id
    version: u64,                 // monotonic; increments each rewrite
    updated_at: DateTime<Utc>,
    skeleton: Vec<StateClaim>,    // slow-changing: what this project is, architecture, hard constraints
    volatile: Vec<StateClaim>,    // fast-changing: current focus, what's hot, what's blocked
    open_loops: Vec<OpenLoop>,    // unresolved threads, ranked by stakes
    staged_notes: Vec<StagedNote>,// prospective reminders left deliberately for future-me
}

struct StateClaim {
    text: String,                 // declarative prose, bounded to 240 UTF-8 bytes
    refs: Vec<String>,            // canonical memory ids this claim summarizes
}

struct OpenLoop {
    text: String,                 // "radar tuning pass — flagged, not started"
    stakes: Stakes,               // High | Medium | Low — drives orientation ordering
    opened_session: String,       // session id that opened it
    refs: Vec<String>,
}

struct StagedNote {
    text: String,                 // "next session, start by checking X"
    author: NoteAuthor,           // Agent | Human
    created_at: DateTime<Utc>,
    consumed: bool,               // cleared once surfaced and acknowledged
}
```

### 4.2 Storage

The continuity-state object is a **canonical Stream A memory**, not a private store (preserves the v0.5 §1 "no hidden second persistence layer" invariant). One per project, scoped `project:<canonical_id>`, reserved tag `continuity-state`, pinned status. It is rewritten by **supersede** (Stream C `WriteMode`), so its history is the event log. `version` increments each rewrite.

**Merge is not free here** *(B3, §4.0.3).* Because two devices can supersede the same object concurrently at their respective session-ends, whole-object supersede over the existing merge driver is **not sufficient** — it produces a last-writer-wins graveyard for `open_loops`/`staged_notes`. The object's internal lists must carry **stable item-level IDs** and be merged item-wise (union with per-item version/consume reconciliation), gated by a two-device concurrent-closeout test. This is a prerequisite, not an implementation detail.

This means orientation reads it via the ordinary indexed `MemoryQuery` (status `pinned`, namespace `project:<id>`, tag `continuity-state`) — cheap, deterministic, no new query surface.

### 4.3 Who writes it

- **Closeout (C0, §5.1)** writes the cheap, deterministic layer: appends/updates `open_loops` and `staged_notes` from the session's agent-supplied summary, bumps `volatile` minimally. No LLM call on the hot path.
- **Dream-time (§5.2)** writes the synthesized layer: refines `skeleton` and `volatile`, dedups and re-ranks `open_loops`, prunes resolved/stale loops, using the existing pass-1/pass-2 machinery. This is where the heavy synthesis lives, off the hot path.

### 4.4 Privacy and governance

The continuity-state memory passes Stream C governance (with **no carve-out**, B7/§4.0.4) and Stream D classification like any write. It must never embed encrypted plaintext, secret-class content, or unreviewed candidate claims. Each `StateClaim.text` and `OpenLoop.text` runs through `safe_plaintext_fragment` before persistence; a fragment that classifies non-`Allow` is dropped (not encrypted into the summary).

**Partial-drop is a degraded write, not a silent edit** *(R7).* Dropping a classified fragment can invert meaning ("don't mention X until legal" → "don't mention until legal"). When any fragment of a continuity-state write is dropped, the resulting object is marked **`degraded: true`** and **must not silently supersede the last-known-good** version as authoritative orientation — a degraded object is held back or surfaced as explicitly partial, never rendered as settled state. Because the object is `project`-scoped and synced, it is subject to the same two-clone convergence guarantee as any canonical memory (with the item-level merge of §4.2).

---

## 5. The lifecycle loop

### 5.1 Closeout (C0) — net-new SessionEnd hook — DEFERRED (§4.0)

> **Deferred with the continuity engine (§4.0).** Closeout-writes-startup is the structural heart of the continuity loop and carries its central hazard (B2: a fail-open hollow snapshot poisoning startup). It ships only after the §4.0 prerequisites hold. The design below is the contract for when it is built.

**Fires:** a new SessionEnd (Claude `Stop` / `SessionEnd`; Codex equivalent) hook, wired into `hooks_wire.rs` matcher tables and `HOOK_EVENTS`. The daemon gains a `RequestPayload::Closeout` variant (the first session-end surface in the protocol). The continuity-state write it performs is **not** a recall-hook write (it is a SessionEnd write, not a T0/T1/T2 read), so it does not violate the strict read-only invariant (§2 inv 4); it must still carry the §4.0.2 acceptance contract so a hollow summary cannot become authoritative.

**Input:** a bounded, agent- or harness-supplied **session summary** — at most a few hundred tokens describing where things landed, the unfinished thread, and any deliberate staged note. The agent authors this (e.g. via the existing `memory_note` surface or a closeout-specific structured field); if absent, closeout falls back to a minimal auto-snapshot (active project + the session's touched entities). Closeout never ingests a full transcript.

**Effect:** updates the continuity-state object (§4.3): records the open loop(s), appends staged notes, minimally refreshes `volatile`. Through the governed write path; fail-open; bounded by a deadline (≤ the 2 s hook timeout) so it never blocks harness shutdown.

**Determinism note:** closeout *writes*, so it is exempt from the read-path byte-stability invariant — but its writes go through the normal event log and merge driver and are themselves reproducible given the same summary input.

### 5.2 Dream-time maintenance — DEFERRED (§4.0)

> **Deferred with the continuity engine (§4.0).** This pass exists only once the continuity-state object exists.

Reuses the Stream F nightly pipeline (launchd, lease-elected, shells to the harness CLI). It adds a continuity-maintenance pass alongside the existing three:

- **Refine** `skeleton`/`volatile` from the period's active+candidate memories and the accumulated closeout snapshots.
- **Re-rank and prune** `open_loops`: drop loops whose referenced work is resolved (the referenced memories superseded/closed), merge duplicates, re-score `stakes`.
- **Decay/boost** feeds from the use-feedback signal (§5.3), not from inclusion counts.

This pass writes a new continuity-state version via supersede. It is the only place the *synthesized* layer changes; closeout only touches the cheap layer. Reuses pass-1 masked-reflection and pass-2 candidate-write machinery; the continuity-state write is governed exactly like a pass-2 candidate except it targets the reserved pinned `continuity-state` memory.

### 5.3 The use-feedback signal

Today, strength's frequency term is driven by `RecallHit` events, which fire for every memory *included in a rendered active-path block* — i.e. **inclusion, not use.** The passive path emits nothing, so passive surfacing currently feeds the model not at all. v1.1 separates the signals and corrects two flaws the fusion review found.

**Events (logged out-of-band, B1).**

- **`PassiveSurfaced`** — a recollection was injected passively. Lets us measure surfaced-but-unused.
- **`RecallHit`** — included in a block (existing; rename-neutral).
- **`RecallUsed`** — the agent demonstrably acted on a surfaced recollection; v1.1 operationalizes this conservatively as an explicit `memory_get` on a surfaced `ref` within the same session.

These events are **writes, and recall is strictly read-only (§2 inv 4)** — so none of them are emitted on the T0/T1/T2 synchronous path. They are produced **out-of-band**: harness-driven, or via the decoupled asynchronous post-render daemon endpoint, after the block has been rendered and returned. The read path computes its response and emits nothing.

**`RecallUsed` is a positive-only signal; disuse is never inferred from its absence** *(B5).* Ambient recall *succeeds* precisely when the agent reads the self-contained prose and acts on it **without** calling `memory_get` — so a missing `memory_get` is the expected outcome of a *working* recollection, not evidence it was unused. v1.0's plan to decay `PassiveSurfaced`-without-`RecallUsed` as a disuse signal would therefore cool exactly the recollections that worked. v1.1 forbids that inference: `memory_get`-on-ref counts only **toward** strength (positive), never against it.

**Strength re-weighting and disuse-decay are deferred** to the continuity-engine phase (§4.0) and gated on a **validated** disuse signal. A reliable "surfaced-but-unused" measure needs a softer signal than `memory_get`-absence (e.g. next-turn n-gram overlap with the recollection, or same-session edits to the files/entities it concerns — §16), validated against labeled data before it is allowed to drive decay. Until then, v1.1 ships the events for **measurement only** (the trust/usefulness ratio in §9.4 and Stream H), and the strength frequency term keeps its current `RecallHit` basis. *(See R6: because the passive path still emits no inclusion `RecallHit`, gating calibration in the relevance phase must be done against the structural-only base explicitly, not a strength term that silently reads zero for passive candidates.)* No persisted strength column is introduced.

---

## 6. Relevance, gating, and the cue

### 6.1 The cue

- **T0:** no user message yet → the cue is the desk projection (§8) + the pinned skeleton and optional focus note (and, once built, the continuity-state object). Orientation is not a semantic retrieval; it is reading state and ordering by salience. The cue-relevance term (§6.2) is zero here (O1a vs O1b).
- **T1:** the submitted message + a bounded rolling window of recent turns/tool state.
- **T2:** the rolling session focus + most recent relevant recollection.

### 6.2 Activation scoring

The existing structural score (v0.5 §8.2: status + scope + entity-match + recency + confidence + source, plus the bounded strength term) becomes the **base**. v1.0 adds the missing organ: a **relevance-to-cue term** for the T1/T2 paths.

```
activation = base_structural_score
           + relevance_to_cue        // NEW: semantic/lexical match of memory ↔ live cue
           + lesson_boost            // NEW: feedback/correction memories at decision/difficulty points
```

`relevance_to_cue` reuses the existing chunk/vector retrieval already on the delta path (`query_chunks` over the message) plus entity-seed overlap; it is the term that makes recall *relevant* rather than merely recent. On T0 there is no cue, so this term is zero and orientation rests entirely on the skeleton/focus-note + desk — by design (the deferred continuity model joins this set when built).

**Testable ranges** *(N4).* Both new terms have specified, bounded, testable ranges so ranking behavior is verifiable: `relevance_to_cue ∈ [0.0, R_max]` and `lesson_boost ∈ {0, L}` (applied iff the cue carries a decision/difficulty signal, independent of the friction pre-gate — N6). The concrete `R_max`, `L`, and the relevance floor (§6.4) are config constants tuned via Stream H (R4), not magic numbers; the spec fixes their ranges and the acceptance tests assert that a higher cue-match reorders candidates and that `lesson_boost` is additive and bounded.

Determinism is preserved per §9: given the same index state and cue, scoring is reproducible; the vector path already has a `vector_recall_degraded` soft-fail flag that keeps recall structural-only on retrieval failure.

### 6.3 The friction pre-gate (T1)

**The pre-gate suppresses only obvious no-ops; it is not the relevance filter** *(B6).* v1.0's pre-gate gated *all* surfacing on lexical friction signals (error output, decision words, entity novelty). The fusion review (judge-elevated to a blocker) showed that produces a large, **silent, unfalsifiable** false-negative rate on exactly the turns where memory matters most but no friction word appears — procedural reuse ("do the same fix in billing"), social context ("reply to Adam"), terse status continuations ("ship it"). v1.1 inverts the design:

- **The pre-gate rejects only obvious no-ops:** bare acknowledgements ("yes", "ok", "do that", "thanks", "👍"), empty or whitespace-only prompts, and pure tool-result acks with no new content. These never need recall.
- **Every other (substantive) prompt proceeds to the relevance gate**, whether or not it contains friction words. Relevance — not lexical friction — decides what surfaces. Silence then comes from *nothing clearing the relevance floor*, which is measurable, rather than from a pre-gate that fired invisibly.
- **Friction signals become a salience input, not a switch.** Error output in recent tool state, decision/difficulty cues, and entity novelty raise priority and trigger `lesson_boost` (§6.2) — they *promote* protective recall, they no longer *authorize* recall.

**Observability so the gate is falsifiable (B6).** Because a silent gate cannot be tuned:

- **Miss-signal.** A `memory_search` or `memory_get` issued by the agent shortly after a silent T1 is logged (out-of-band) as a candidate **gate/relevance miss** — the agent went looking for something the channel should arguably have surfaced. This is a primary precision-debugging metric.
- **Deterministic self-test mode.** A configurable sampling mode forces surfacing on ~1% of turns, **keyed deterministically by session-id** (so byte-stability per §9.1 holds — the same session always samples the same turns), to measure surfacing precision in production without a hand-labeled set. Off by default; on in eval/shadow runs.

Tier 2 (optional, configurable): an embedding-centroid drift signal for topic shift, deferred to a later phase to keep per-turn cost down (§16). Net effect: the pre-gate is now a cheap cost-saver on genuinely empty turns, and the *relevance floor* (§6.4) — which is tunable and observable — carries the precision burden.

### 6.4 Gating discipline

- **Gate, don't truncate.** Selection admits a recollection only while its activation clears the relevance floor *and* its full rendered cost fits the budget. Stop when either fails. Truncation (§7.5) is a last-resort backstop, not the primary bound.
- **The relevance floor ships behind a flag, permissive by default** *(R4).* The floor is the whole game and is empirical; defaulting it tight before calibration would silently starve the channel (and a wrong floor gates the entire feature). v1.1 ships T1 relevance gating **behind a config flag with a deliberately permissive floor**, and tightens it only against a labeled "should-this-have-surfaced?" set in Stream H (§14.3). Until that set exists, the floor is not allowed to default to a tight value.
- **Silence is valid output.** Zero recollections is a correct, common T1 result. The CLI/daemon still emits the empty wrapper so downstream parsing never branches on emptiness — consistent with the v0.5 delta-empty contract. **The exact empty-wrapper form is specified per trigger** *(N5)* so harness parsers never invent variants: T0/T1/T2 each emit a single self-closing `<memory-recall empty="true" trigger="t0|t1|t2" policy="stream-e-v1.1" />` with no child content. The form is fixed by the spec and asserted by the Stream H parser regression.
- **Dedup against working context, with a turn-distance threshold** *(N3).* Extend the existing native-memory-head dedup (`read_native_memory_head`) to also suppress recollections already surfaced earlier in the same session and content already present in the loaded CLAUDE.md/AGENTS.md head. Suppression is **not permanent for the whole session**: a recollection may re-surface after a turn-distance threshold (a meaningful gap or topic shift), so a genuinely re-relevant memory is not silenced for the entire session by a single early surfacing. Don't re-tell the agent what's already on screen *now*; do allow it back when it becomes relevant again later.

### 6.5 Cold-start (first contact) *(R2)*

The single most important orientation case is the one v1.0 left undefined: **no continuity-state, no pinned skeleton, no focus note** — the first session in a project, or after the relevant memories were deleted. T0 must **not** degrade to a recency dump (that is the v0.5 failure this redesign exists to kill). Defined behavior:

1. If a desk projection exists, render **desk-only orientation** (branch, dirty summary, recent commits) as the anchor.
2. Add any pinned identity/invariants that do exist (these are global/`me`-scoped and usually present even on a brand-new project).
3. If neither exists, emit the **empty wrapper** (§6.4) — orientation is allowed to be empty on true first contact.

Cold-start is a Phase-1/Phase-3 acceptance fixture (a project with zero project-scoped memories), asserting desk-or-empty output and the absence of any recency-ranked atom list.

---

## 7. Injection format and rendering

### 7.1 The recollection unit

The `<memory>` element of v0.5 §5 (title + always-present, often-empty `<snippet>`) is replaced. The unit becomes a prose recollection that preserves machine-readable provenance:

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
- **No empty elements.** A recollection with no usable body is not surfaced at all (this both fixes and generalizes `500a60d`'s empty-snippet omission, and resolves the v0.5 §5 "snippet always present" contradiction — that contract is retired with the `<memory>` element).

### 7.2 Block envelope

The top-level `<memory-recall ...>` envelope and the `<recall-explanation>` accounting block (v0.5 §3.3, §9.6) are retained — they carry the omission/budget metadata Stream G and the eval harness consume. What changes is the *content* sections: orientation renders the skeleton/focus-note + desk as labeled prose groups (and, once built, the continuity-state); T1/T2 render a flat list of `<recall>` units. All dynamic content is placed bottom-of-context per §9.5.

### 7.3 Attributed-declarative rendering, with external-evidence framing for untrusted sources

Every surfaced body passes `neutralize_imperative_prose` (now contract, not best-effort — §2 inv 1). An imperative source ("Always run the gate before pushing") renders as reported fact ("Recalled — the standing practice has been to run the gate before pushing"). Declarative sources pass through byte-identically so the determinism tuple stays stable.

**Declarative is not enough on its own** *(R9, §2 inv 2).* A poisoned or wrong memory still steers as a *reported fact*. So the lead-in is keyed to trust: a **user-authored, high-confidence** recollection may render as a direct recollection ("Recalled — …"), but a **system-synthesized or low-confidence** recollection must render as third-party evidence the agent should weigh — "A prior memory reports…", "An earlier note claims…". The agent is never handed a system-derived claim in its own voice. The chosen lead-in is a deterministic function of `kind`/`source`/`confidence`, so it is part of the byte-stability tuple.

### 7.4 Budget on rendered bytes

The root cause of the original overflow: selection counted **summary tokens only** (`estimated_tokens(&summary)`), while each rendered entry also carried ~120 chars of scaffolding, a 35-char ref, and a timestamp — so "within budget" undercounted real size by multiples, and ~30 entries blew past the 10 k-char cap. v1.0 budgets on the **full rendered cost** of each unit (attributes + body + envelope scaffolding). The per-entry-count cap (`HOOK_RECENT_MEMORY_MAX_ENTRIES`) becomes a redundant safety net rather than the primary defense.

### 7.5 Well-formed truncation

Today's `cap_passive_block` cuts to the last newline and appends `</memory-recall>`; if the cut lands inside an entry, the result is an unclosed element — malformed XML (observed in the original paste: an unclosed `<memory>` followed by `<recall-truncated/>`). v1.0 requires truncation to cut at a **recollection boundary** (a complete `<recall>…</recall>` unit), so the emitted block is always well-formed. The backstop should almost never fire given byte-budgeting (§7.4), but when it does it must not emit malformed output.

### 7.6 Per-trigger budgets

| Trigger | Target | Hard cap | Default entries |
| --- | ---: | ---: | ---: |
| T0 orientation | 300 | 600 | skeleton + focus note + desk (continuity model when built) |
| T1 associative | margin (often 0) | 360 | ≤ 3 |
| T2 re-orientation | 120 | 200 | 1 + current desk state |

---

## 8. The desk read (repo state ingestion) — net-new, via a daemon-cached projection

Today git is read *only* for project binding (`git remote get-url origin`, `git rev-parse --show-toplevel`, both under a 2 s deadline). No branch, status, log, PR, or CI is ingested.

### 8.0 The hot path never spawns git/`gh` *(B4)*

v1.0 specified reading the desk by running `git status --porcelain`, `git log`, and `gh pr view` synchronously on the T0/T2 hook path. The fusion review found this unsafe:

- `gh pr view` and PR-derived CI are **network calls on the synchronous recall hot path** — a direct violation of invariant #10 (§12).
- Even local `git status --porcelain` can **block on `.git/index.lock`** when an IDE, editor commit-hook, or background fetch holds the index, and in a large monorepo a cold `git status` routinely takes **seconds** — which would blow the 150 ms desk budget (§9.3) and, under a hard timeout, permanently starve large-repo users of desk context.

**Fix:** the desk read on T0/T2 is served from a **daemon-cached desk projection** — an in-memory snapshot the daemon maintains in the background and the hook reads in O(1). No recall hook ever spawns a git or `gh` subprocess. The projection is refreshed off the hot path (file-watch on `.git/HEAD`, the index, and `refs/`, plus a debounced periodic refresh); PR/CI, being network, are refreshed on a slower background cadence and are always optional. If the projection is missing or stale past a threshold, the desk degrades to "unavailable" and orientation proceeds memory-only — it never blocks to compute fresh state.

### 8.1 Inputs (refreshed in the background, read O(1) on the hot path)

- current branch (from `.git/HEAD` / `git rev-parse --abbrev-ref HEAD`, run by the background refresher);
- uncommitted-work summary (`git status --porcelain`, counted/summarized, never file contents);
- recent commits (`git log --oneline -n N`);
- open PR for the branch (`gh pr view`, if `gh` is present and authenticated) — **off-path only**, slower cadence, optional;
- CI status (from the PR view, if available) — off-path only, optional.

The hook consumes whatever the projection currently holds; it does not wait on a refresh.

### 8.2 Join with memory

The desk is crossed with the skeleton + focus note (and, once built, the continuity-state object): "you're on `codex/x-spend-opt`, 8 commits deep — your focus note says radar tuning is the next thread." The desk anchors orientation; memory annotates it.

### 8.3 Read-only, fail-open, bounded

No command mutates the repo, and none runs on the recall hot path (§8.0). The background refresher runs each command under a deadline; any failure (no git, no `gh`, timeout, lock contention) leaves the last projection in place or marks it unavailable, and orientation degrades silently to memory-only. The hot-path read is a pure in-memory lookup and cannot block.

### 8.4 Determinism

The desk is live state and therefore not deterministic across real time — but the byte-stability invariant (§9) is, as in v0.5, conditioned on "the same repo state," now operationalized as **the same desk-projection snapshot**. Given a fixed projection snapshot, request context, clock fixture (and, once built, continuity-state version), T0/T2 output is byte-identical. Tests fix the desk by injecting a fixture projection, exactly as v0.5 fixes the index — and because the hot path reads the projection rather than shelling out, tests no longer need a live fixture repo with a real `.git`.

---

## 9. Determinism, caching, and performance

### 9.1 Byte-stability

Carries forward v0.5 §2.7, extended: given the same **desk-projection snapshot, request context, budget, clock fixture** (and, once the continuity engine ships, **continuity-state version**), T0/T1/T2 emit byte-identical blocks. The desk-projection snapshot id is part of the cache key; the continuity-state `version` joins it when that object exists. Note byte-stability is about the *content* of the block; placement (§9.5) governs how that block interacts with the harness's own prompt cache.

### 9.2 The per-turn cost

T1 already pays a chunk/vector retrieval per prompt today. The friction pre-gate (§6.3) is a *reduction*: routine turns skip ranked surfacing entirely. The relevance term reuses the retrieval already on the path. Net per-turn cost should not exceed today's delta path; the pre-gate should reduce it on the common (routine) turn.

### 9.3 Performance budgets

Release-gate fixture sizes (warm path), adapting v0.5 §13:

- T0 orientation, 1 000 memories: p95 ≤ 250 ms (skeleton/focus-note read is one indexed lookup; the desk read is an **O(1) in-memory projection lookup**, not a subprocess — §8.0).
- T1 routine turn (pre-gate rejects an obvious no-op), 1 000 memories: p95 ≤ 40 ms.
- T1 surfacing turn (five matching entities): p95 ≤ 120 ms.
- T2 re-orientation: p95 ≤ 120 ms.
- Desk projection read on the hot path must add **≤ 5 ms p95** to T0/T2 (it is a memory read); the background refresher's git/`gh` work is **off the hot path** and is never counted in these budgets.
- *(deferred)* C0 closeout write, once built: p95 ≤ 200 ms; never exceeds the hook deadline.

Cold-start (first call after boot) ≤ 600 ms at 1 000 memories, as v0.5. The desk projection may be cold (empty) on the very first hook after boot; T0 then renders memory-only and the projection warms in the background (§6.5).

### 9.4 Observability counters

Extends v0.5 §13.1 additively:

- `recall.orientation_invoked_total`, `recall.reentry_invoked_total` (`recall.closeout_invoked_total` lands with the deferred C0 hook);
- `recall.t1_surfaced_total`, `recall.t1_silent_total` (the silence rate is a primary health metric — a system that surfaces on most turns is mis-gated);
- `recall.friction_gate_rejected_noop_total` (no-op rejections — should be the common case) and `recall.t1_miss_signal_total` (a `memory_search`/`memory_get` shortly after a silent T1 — the relevance/gate-miss precision metric, §6.3);
- `recall.passive_surfaced_total`, `recall.recall_used_total` (the trust/usefulness ratio; both logged out-of-band, §5.3);
- `recall.desk_read_degraded_total{reason}` and `recall.desk_projection_stale_total` (projection older than the staleness threshold at read time).

---

### 9.5 Injection position and the harness prompt cache *(B8)*

Modern harnesses cache the prompt **prefix** — the cache hits only while the early, static portion of the prompt is byte-identical across turns. A per-turn recall block (0–3 memories, content changing every turn) injected **into the cached prefix** would invalidate that prefix on every turn, turning each turn into a full uncached re-evaluation of the whole session — a latency and token-cost blowup that defeats this spec's own cacheability rationale, *even though each block is itself byte-deterministic (§9.1)*.

**Contract:** the **per-turn** trigger (T1, UserPromptSubmit, fires every turn) must be delivered as **turn-local conversation context adjacent to the latest user prompt** — never by mutating the static system/developer prefix, because its content changes every turn. The **session-boundary** triggers (T0 at SessionStart `startup`, T2 at SessionStart `compact`/`resume`) fire once per session segment, at the start of that segment's conversation before a stable per-turn prefix exists, so they do not repeatedly invalidate a cache and are exempt from the turn-local requirement; they still must not be spliced into a *shared static system prompt* that persists across sessions.

**Verification result (confirmed — Codex, recorded with the fusion review).** In current Claude Code and Codex, per-turn `UserPromptSubmit` hook context is placed in the **conversation, not the static system prompt**: Claude Code wraps `additionalContext` in a system-reminder "inserted into the conversation at the point where the hook fired" and lists `UserPromptSubmit` as appearing "alongside the submitted prompt"; the Claude SDK explicitly states conversation-injected content (e.g. `CLAUDE.md`) "doesn't affect the system prompt cache." Codex documents the same hooks as "extra developer context" and its source records the user prompt first and the hook's additional context after it. `SessionStart` context appears "at the start of the conversation, before the first prompt." **So in both target harnesses the B8 defect does not currently bite — hook output already lands outside the cached prefix.** The contract is therefore primarily a **documentary invariant** (don't move recall into the system prompt, and don't rely on a prefix that varies per turn) plus a Phase-1 placement-assertion test, not new machinery. Both providers' caching docs independently confirm the underlying rule: cache hits require an exact prefix match, so variable content must sit *after* the stable cached prefix.

## 10. Privacy and safety (Stream D)

Unchanged authority: Stream D owns classification, encryption, and reveal. v1.1 adds two consumption points:

- Every synthesized recollection body passes `safe_plaintext_fragment` before emission; non-`Allow` fragments are dropped, not encrypted into prose. *(Ships in v1.1.)*
- *(Deferred with the continuity engine — §4.0.)* Every `StateClaim`/`OpenLoop`/`StagedNote` text passes `safe_plaintext_fragment` before persistence; the continuity-state object and closeout summary are subject to full classification at write time; a closeout summary carrying secret-class content is refused exactly as any write (`SecretRefused`), never silently stored. A write where any fragment is dropped is marked `degraded: true` and must not silently supersede last-known-good as authoritative orientation (R7, §4.4).

`memory_startup`/T0/T1/T2 still never call `memory_reveal` and never emit ciphertext or masked-body projections.

---

## 11. Cross-stream surface changes

Implementation lands these additive surfaces on shipped streams. Like v0.5 §1.1, they are part of this contract. Surfaces tagged *(deferred)* land only with the continuity engine (§4.0).

**Ships in v1.1:**

- **Protocol (Stream A/daemon):** new `RequestPayload::Reorient { cwd, session_id, harness }` variant for T2 + `ResponsePayload` analog, dispatched when the **existing** SessionStart matcher fires with source `compact`/`resume` (§3.3). **No new hook event** is wired in v1.1 — `hooks_wire.rs`/`unwire.rs` `HOOK_EVENTS` are unchanged for the shipping set (PreCompact is not used; SessionEnd is deferred).
- **Daemon — out-of-band telemetry endpoint (B1):** a separate, decoupled, asynchronous post-render write endpoint (distinct from the read-path recall requests) that records `PassiveSurfaced`/`RecallUsed`. Recall responses carry no write side effects; the harness (or the daemon, asynchronously after responding) calls this endpoint. No recall request mutates state.
- **Daemon — desk projection refresher (B4):** a background task maintaining a **net-new `DeskProjection` state object** in the daemon (there is no existing git/desk cache to extend). It reuses the daemon's established background-task idioms — the `tokio::spawn` + shutdown-aware loop and `interval_at` cadence of the reality-check scheduler (`memoryd/src/server.rs`) and embedding worker (`memoryd/src/embedding/worker.rs`), plus the substrate file-watch API (`Substrate::watch()`, `memory-substrate/src/watcher/`) over `.git`. The hot path reads the projection O(1); the refresher runs the git/`gh` work off-path. (Current git reads on the recall path are only project binding's `rev-parse` + `remote get-url origin` under `GIT_COMMAND_TIMEOUT`, `memoryd/src/recall/project.rs` — the desk read must not add `git status`/`git log`/`gh` to that path.)
- **Stream A events:** new `EventKind::PassiveSurfaced` and `EventKind::RecallUsed` (§5.3), alongside existing `RecallHit`. Written **only** via the out-of-band endpoint above, never on a recall read path.
- **Harness/CLI — injection placement (B8):** the recall hook output for T1/T2 is positioned bottom-of-context (§9.5); the wiring asserts placement against each target harness in Phase 1.
- **Stream D:** no new surface; consumes existing `safe_plaintext_fragment`.
- **Stream G observability:** the new counters (§9.4) and a trust/usefulness panel (`passive_surfaced` vs `recall_used`).
- **Stream H eval:** parser reads `<recall ref=...>` (§15) and the per-trigger empty-wrapper form (§6.4, N5).

**Deferred (with the continuity engine, §4.0):**

- **Protocol:** `RequestPayload::Closeout { cwd, session_id, harness, summary: Option<CloseoutSummary> }` *(N2: corrected spelling)*; SessionEnd added to the matcher tables / `HOOK_EVENTS`.
- **Stream A memory kind:** the reserved `continuity-state` pinned memory per project (§4.2), with item-level-merged lists (B3). No schema change — it is an ordinary memory with a reserved tag.
- **Stream C governance:** governs the continuity-state write/supersede like a pass-2 candidate **with no carve-out** *(B7 — reverses v1.0).* A system-authored continuity update passes `dream_source` confidence gating like any synthesized write; it is **not** exempted, because exemption is precisely the path by which a dream-synthesized hallucination would become pinned authoritative orientation (invariant #7). Claim-level provenance distinguishes system-derived from user-authored content in the surface.
- **Stream F dreaming:** the continuity-maintenance pass (§5.2) added to the nightly pipeline.

---

## 12. Invariants (consolidated)

A change failing any of these fails review:

1. **Recall (T0/T1/T2/T3) is strictly read-only** and never reveals ciphertext. No write is *caused by* a recall hook on its synchronous path; `PassiveSurfaced`/`RecallUsed`/note-consume writes are executed out-of-band (§2 inv 4, §5.3). *(B1.)*
2. Only `active`/`pinned`, `passive_recall = true`, non-pending-review memories surface as facts (v0.5 §2.3–2.5).
3. Output is byte-stable given desk-projection snapshot + request context + budget + clock (+ continuity-state version once that object ships) (§9.1).
4. Every surfaced recollection is declarative, attributed, and carries a recoverable `ref`; **system-derived or low-confidence recollections are framed as external evidence, not the agent's own voice** (§2 inv 1–2, §7.3). *(R9.)*
5. No contentless entry is ever surfaced (§7.1).
6. Truncation, if it fires, cuts at a recollection boundary and emits well-formed output (§7.5).
7. **No git/`gh` subprocess and no network call on any synchronous recall hot path (T0/T1/T2).** The desk is read from the daemon-cached projection in O(1); synthesis and desk refresh are off-path (§8.0, §9.3). *(B4; supersedes/sharpens v1.0 invariant 10.)*
8. The per-turn trigger (T1) renders as turn-local conversation context adjacent to the latest user prompt, outside the cached prompt prefix; no recall block mutates a shared static system prompt (§9.5). *(B8.)*
9. The desk read is fail-open and never blocks recall; a missing/stale projection degrades to memory-only orientation (§8.3).
10. Silence is a valid output; the per-trigger empty wrapper is always emitted (§6.4).
11. Cold-start never degrades to a recency dump (§6.5). *(R2.)*
12. *(When the continuity engine ships — §4.0.)* The continuity-state object is a governed, classified canonical memory with **no governance carve-out** — never a private store, never carrying secret/unreviewed content, item-level-merged across devices, and never rendered as authoritative when degraded/hollow (§4.0, §4.2, §4.4). *(B2, B3, B7, R7.)*
13. *(When the continuity engine ships.)* Closeout is fail-open and never blocks the harness beyond the hook deadline (§5.1).

---

## 13. Phased build plan

Each phase is independently shippable and testable; value lands before the whole is built. **v1.1 re-sequences v1.0's plan** (Option A): ship the continuity-free channel first, prove adoption, defer the continuity engine to a gated final phase.

**Phase 1 — Rendering, budget, and injection safety (stops the noise).** Prose `<recall>` unit (§7.1); byte-budgeting (§7.4); entry-boundary truncation (§7.5); discoverable guidance string without card-catalog over-advertising (§3.4, N1); retire the `<memory>`/empty-snippet contract; per-trigger empty-wrapper form (§6.4, N5); **bottom-of-context placement** with per-harness placement confirmed (§9.5, B8); **external-evidence framing** for system-derived/low-confidence recollections (§7.3, R9); shrink T0 to high-signal. No new triggers, no new persistence, no writes. **Scope (R8): this phase _stops the noise_ — it does not by itself make the channel "trustworthy"** (relevance, gating, desk, and continuity safety land later). Note: for imported reference-doc chunks whose canonical body is raw section text rather than a distilled lesson, Phase 1 can drop the contentless unit and render existing declarative summaries, but cannot synthesize a "conclusion" — that needs the deferred dream-time gist step.

**Phase 2 — Relevance and gating (makes T1 real).** Add the relevance-to-cue term with testable ranges (§6.2, N4); the **narrowed friction pre-gate** (no-ops only) plus the **miss-signal and deterministic self-test** so the gate is falsifiable (§6.3, B6); independent `lesson_boost` path (N6); conversation-context dedup with turn-distance (§6.4, N3); silence-as-output; the **relevance floor behind a flag, permissive by default** (§6.4, R4), calibrated explicitly against the **structural-only base** since the passive path emits no inclusion `RecallHit` (R6); **out-of-band positive-only `PassiveSurfaced`/`RecallUsed` logging for measurement** (§5.3, B1/B5). Reuses the delta retrieval path.

**Phase 3 — Desk and re-entry (anchor on the one persistent channel).** The **daemon-cached desk projection + background refresher** (§8.0, B4); defined **cold-start** (§6.5, R2); the T0 desk-first orientation join (§3.1); the optional single **pinned focus note** as the minimal continuity bridge (a `memory_note` write + a T0 read — not the continuity engine); T2 re-orientation on SessionStart(compact)/(resume) rendering the **current absolute desk state** (§3.3, R10). This delivers desk-first "remembered, not just recent" orientation with none of the continuity-engine hazards.

**Gate before Phase 4:** Stream H / telemetry shows the channel is *used* (agents act on surfaced prose; the trust/usefulness ratio is healthy). If adoption does not materialize, Phase 4 is not built — the whole continuity cathedral is saved.

**Phase 4 — DEFERRED: the continuity engine (gated on §4.0).** Only after **all** §4.0 prerequisites are specified, implemented, and test-gated: the continuity-state object with item-level cross-device merge (§4, B3); the SessionEnd closeout hook + protocol variant with a **substance-based acceptance gate** and "remembered, not verified" framing (§5.1, B2); **no governance carve-out** + claim-level provenance (§11, B7); **continuity-claim staleness/contradiction invalidation** (R5); dream-time maintenance (§5.2); staged notes; and, only on a **validated** disuse signal, strength re-weighting and decay/boost (§5.3, B5). The structural heart of the original redesign — earned, not assumed.

---

## 14. Open questions (decisions needed before/while building)

The fusion review settled several v1.0 open questions; those are marked **resolved** with the decision. The rest are genuinely open.

1. **Re-orientation hook across harnesses.** *(Resolved — verified.)* `PreCompact` is **not** a context-injection surface in Claude Code or Codex (block-only, no `additionalContext`); both expose post-compaction re-entry via `SessionStart` source `compact` (Codex sources: `startup|resume|clear|compact`). T2 therefore rides SessionStart(compact)/(resume) on **both** harnesses with no new hook wire (§3.3, §3 intro). No remaining cross-harness gap for T2.
2. **Relevance floor calibration.** *(Approach resolved — R4; value still empirical.)* The floor ships behind a flag, permissive by default, and is tuned against a labeled "should-this-have-surfaced?" set in Stream H, calibrated on the structural-only base (R6). The labeled set itself still needs to be built; until it exists the floor stays permissive.
3. **`RecallUsed` usability.** *(Resolved on the safe side — B5.)* `memory_get`-on-ref is logged **positive-only**; disuse is never inferred from its absence. Whether `memory_get` adoption ever yields a high-signal usage stream (one family argued effective tool-guidance will train fetches; others that ambient recall works *without* the fetch) is an empirical question for telemetry, not a blocker — the design is invariant to the outcome. A *validated softer disuse signal* is a prerequisite before any decay ships (§4.0, §5.3).
4. **Continuity-state merge semantics.** *(Resolved into a hard prerequisite — B3, §4.0.3.)* Whole-object supersede over the existing merge driver is **not** sufficient; the object needs stable item-level IDs and item-wise merge, gated by a two-device concurrent-closeout test. This must be specified and tested before the continuity engine ships — it is no longer an "confirm it just works" question.
5. **Closeout authorship.** *(Open — but deferred to Phase 4.)* When the continuity engine is built: agent-authored summary (higher signal, depends on the agent doing it) vs harness-captured, with the auto-snapshot fallback. Recommend agent-authored with fallback — but the fallback must satisfy the §4.0.2 acceptance gate (a hollow auto-snapshot must register as `degraded`, not as fresh authoritative state).
6. **Pinned focus note ergonomics.** *(New, minor.)* The Phase-3 single pinned focus note (§1.1, §3.1) needs a light convention for how the agent/human sets and clears it via `memory_note`. Define during Phase 3; it is deliberately a thin manual bridge, not the maintained model.

---

## 15. Acceptance signals

Implementation of a phase is complete when its tests/docs exist and pass. Per phase (re-sequenced for v1.1):

- **Phase 1 (rendering, budget, injection safety):** `recall_render` tests assert the `<recall>` prose unit, no empty elements, byte-budgeting (a fixture that overflowed under summary-only budgeting now selects correctly), entry-boundary truncation (a forced-overflow fixture emits well-formed output), the per-trigger empty-wrapper exact form (N5), the guidance string names `memory_search`/`memory_get` without card-catalog phrasing (N1), and **external-evidence framing** — a system-derived/low-confidence fixture renders with a third-party lead-in, a user-authored high-confidence one does not (R9). A **placement test** confirms T1/T2 output is positioned bottom-of-context for each target harness, or documents the harness's existing bottom placement (B8). Stream H eval parser updated to `<recall ref=...>` with a passing regression. *(Claim scope: "stops the noise," R8 — not "trustworthy.")*
- **Phase 2 (relevance and gating):** `recall_gating` tests assert the pre-gate rejects **only** obvious no-ops (a substantive friction-word-free prompt still reaches the relevance gate, B6), surfacing on relevant fixtures, silence when nothing clears the (flagged, permissive) floor (R4), the relevance term changes ordering vs structural-only (calibrated on the structural-only base, R6), `lesson_boost` fires independent of the pre-gate (N6) within its testable range (N4), dedup suppresses an already-surfaced ref **but allows re-surfacing past the turn-distance threshold** (N3), and the **miss-signal** + **deterministic self-test** emit correctly (self-test sampling is stable per session-id, B6). A telemetry test asserts `PassiveSurfaced`/`RecallUsed` are written **only via the out-of-band endpoint, never on the read path** (B1), and that `memory_get`-absence never decrements strength (B5). Determinism test extended to the cue path.
- **Phase 3 (desk and re-entry):** `desk_read` tests assert the hot path reads the **cached projection in O(1)** and spawns no git/`gh` subprocess (B4), fail-open on missing/stale projection, byte-stability given an injected fixture projection, **cold-start** emits desk-or-empty and never a recency dump (R2), the T0 join with skeleton/focus-note, T2 renders **current absolute desk state** with no stored baseline (R10), and the pinned focus note round-trips through `memory_note` → T0.
- **Phase 4 (deferred — continuity engine; gated on §4.0):** `continuity_state` tests assert closeout writes/supersedes the pinned object, the **acceptance gate** rejects a hollow/degraded snapshot as authoritative (a fresh-timestamped empty auto-snapshot does not become orientation, B2), a **two-device concurrent-closeout merge** converges item-wise (B3), governance gates a system-authored update with **no carve-out** (B7), continuity claims **invalidate on desk-contradiction/superseded refs** (R5), dream-time refines the object deterministically, staged notes surface once then clear, the object passes governance + `safe_plaintext_fragment` (a secret-class closeout summary is refused; a partial-drop write is marked `degraded` and does not supersede last-known-good, R7); `use_feedback` tests assert decay fires only from a **validated** disuse signal, never from `memory_get`-absence (B5).
- Docs: `docs/api/stream-e-ambient-recall-api.md`; updates to the Stream A/C/F/G/H API docs for the §11 surfaces; `CLAUDE.md` authoritative-docs table repointed; `STREAM_E_POLICY` bumped to `stream-e-v1.1` — all only after the relevant phase's tests pass.

## 16. Explicit deferrals

- **The entire continuity engine** *(v1.1 headline deferral, Option A)*: the maintained `ContinuityState` object (§4), the SessionEnd closeout (C0) hook (§5.1), dream-time continuity maintenance (§5.2), and use-driven strength re-weighting / decay (§5.3). Deferred to **Phase 4**, gated on **all** §4.0 prerequisites (B1 out-of-band writes, B2 acceptance gate, B3 item-level merge, B7 no carve-out, R5 invalidation) and on a Phase-3 adoption signal. The optional single pinned focus note (§3.1) is the only continuity surface that ships before Phase 4, and it is a manual `memory_note` convention, not the maintained model.
- A **validated softer disuse signal** beyond `memory_get`-on-ref (next-turn n-gram overlap, file-touch correlation) (§5.3) — a hard prerequisite for any decay, deferred until validated against labeled data. `memory_get`-absence is never a disuse signal (B5).
- Embedding-centroid topic-drift as a Tier-2 friction salience signal (§6.3) — once per-turn cost is measured.
- Cross-session real-time continuity merge UI (Stream I surface).
- Per-harness closeout-summary capture beyond agent-authored + auto-snapshot.
- A daemon-cached doctor projection in `<pending-attention>` (inherited deferral from v0.5 §9.5). *(Note: the desk projection of §8.0 is a related but distinct cached-projection surface that **does** ship in Phase 3.)*

If a phase's acceptance tests cannot pass without one of these, revise this spec before coding continues.
