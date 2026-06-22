# Stream E — Ambient Recall (Passive Memory Redesign) v1.0

**Status:** Draft for review. Not yet an accepted implementation contract. This document proposes a ground-up redesign of the Stream E passive-recall surface and supersedes the *approach* of `stream-e-passive-recall-v0.5.md` while reusing much of its machinery. On acceptance, the Authoritative-documents table in `CLAUDE.md` and the `STREAM_E_POLICY` version string should be repointed here; this draft does not mutate either.

**Date:** 2026-06-22.

**Authors:** Claude, from a design session with Trey.

**Sources:** the live Stream E contract (`stream-e-passive-recall-v0.5.md`), the shipped Stream A–I surfaces, the conceptual walkthrough at `docs/ideal-agent-memory-hooks.html`, the cap-recent-memory fix (`500a60d`), and two ground-truth code recon passes (2026-06-22) of the hook wiring, delta path, dreaming, dynamics/strength, and confidence assignment.

**Non-source:** older Stream E drafts (v0.1–v0.5) are historical except where they describe still-shipped machinery this spec explicitly reuses.

**Policy string:** on acceptance, the version string in policy/manifest/recall-block attributes bumps to `stream-e-v1.0`.

---

## 0. Preamble — how this came up, and what we're building

### 0.1 How this came up

Opening Codex in a repo injected a `<memory-recall>` block that was forty `<recent-memory>` entries deep — heading-fragments of three or four imported documents (`reference-ingest — Tables that matter`, `— Tables that don't`, `— Drop reasons`), each with an opaque 35-character `ref`, a microsecond-precision timestamp, an empty `<snippet></snippet>`, and a flat `confidence="0.70"` — ranked by recency, then cut off mid-entry by the char-cap backstop. It was noise. Worse than noise: it cost attention to read and trained the agent to ignore the whole channel.

The immediate fix (`500a60d`) capped the passive section to eight entries and dropped empty snippet tags. That stopped the bleeding but treated the symptom. Stepping back to the conceptual purpose surfaced the real problem, and a blank-canvas redesign. The conceptual walkthrough is at `docs/ideal-agent-memory-hooks.html`; this spec is its implementation contract.

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

- **O1 — Relevance over recency.** Most recall fires on the live cue (the user's message and the work in front of the agent), not on the clock. Ranking gains a relevance-to-now term it currently lacks entirely.
- **O2 — Conclusions, not pointers.** The injected unit is a self-contained recollection — the *lesson*, in prose — not a title plus a fetchable handle. A handle the agent must dereference is the tool-call-to-remember problem in disguise.
- **O3 — Gate, don't truncate.** A high relevance bar decides what surfaces; silence is a valid and frequent output. Token efficiency is a *consequence* of the bar, never the optimization target.
- **O4 — Continuity is maintained, not queried.** A continuity-state object is written at closeout and refined at dream-time, and read back at orientation.
- **O5 — Trust is the currency.** Precision (few items, all relevant) earns the agent's trust so it leans on the channel; volume destroys it. A channel the agent distrusts is worse than no channel.
- **O6 — Safety preserved.** Read-only, no encrypted plaintext, governance-authoritative, deterministic and cache-stable — every Stream E v0.5 safety invariant carries forward, plus new injection-safety invariants for an inherently larger prompt-injection surface.

### 0.5 Goals (measurable)

- Median orientation (T0) block ≤ 300 estimated tokens; hard cap ≤ 600. Never truncates mid-entry.
- Per-turn (T1) recall surfaces nothing on a clear majority of routine turns (silence is the common case); when it surfaces, ≤ 3 recollections by default.
- Zero contentless entries: a recollection with no usable body is not surfaced.
- Injected recollections read as declarative attributed facts, never as imperatives.
- Output is byte-stable given the same repo state, continuity-state version, request context, and clock fixture (carries forward v0.5 §2.7).

### 0.6 Non-goals

- Owning model inference. Synthesis reuses the Stream F dream pipeline, which shells to the user's own harness CLI.
- A second persistence layer. The continuity-state object is a canonical Stream A memory, governed by Stream C and classified by Stream D — not a private store.
- Replacing deliberate recall. `memory_search` / `memory_get` remain the conscious "go dig" path; this spec makes them *discoverable*, not redundant.
- Cross-harness session-transcript capture. Closeout consumes a bounded, agent- or harness-supplied summary, not a full transcript.

### 0.7 What this keeps, and what it changes

**Keeps (reuses existing machinery):** the `recall hook` dispatch handler and three wired lifecycle events; the per-turn delta path (`build_delta_response`); project/session binding and namespace resolution (v0.5 §4); candidate collection over the indexed `MemoryQuery` extension (v0.5 §6); the deterministic structural ranking core (v0.5 §8.2) as the *base* score; Stream F dreaming's three-pass pipeline and lease; the dynamics/strength ranking term; Stream D privacy helpers; the recall-explanation/omission accounting and observability counters (v0.5 §3.3, §13.1).

**Changes (net-new or reshaped):**

| Area | v0.5 (today) | v1.0 (this spec) |
| --- | --- | --- |
| Startup content | recency-ranked atom dump | maintained continuity-state model + desk read (§3.1, §4) |
| Per-turn trigger | delta on every prompt | friction/relevance-gated; silence is valid (§3.2, §6) |
| Re-entry / compaction | not handled | new T2 trigger (§3.3) |
| Closeout | no surface exists | new SessionEnd hook writes continuity state (§5.1) |
| Injected unit | `<memory>` title + empty snippet | prose recollection, ref preserved as attribute (§7) |
| Ranking | no relevance-to-now term | adds a cue-relevance term (§6.2) |
| Budgeting | summary tokens only | full rendered-byte cost (§7.4) |
| Truncation | cuts at any newline → malformed XML | cuts at entry boundary, always well-formed (§7.5) |
| Feedback | inclusion counts (`RecallHit`) | use-feedback signal distinct from inclusion (§5.3) |
| Tool discoverability | generic guidance string | guidance names `memory_search`/`memory_get` (§3.4) |

---

## 1. Conceptual model

### 1.1 Three data types

Passive recall in v1.0 operates over three distinct data types. Conflating them is the original design error.

1. **The continuity-state model** — a small, maintained, synthesized object per project: a slow-changing *skeleton* (what the project is, its architecture, hard constraints), a *volatile* layer (current focus, what's hot, what's blocked), *open loops* (unresolved threads ranked by stakes), and *staged notes* (prospective reminders left deliberately for future-me). Written at closeout and dream-time; read at orientation and re-entry. §4.

2. **Recollections** — gist units. A recollection is one self-contained, declarative proposition with a consequence — the lesson — derived from a canonical memory, rendered as prose, carrying provenance (`ref`, `kind`, `confidence`). Recollections are what T1 surfaces. §7.

3. **The desk** — live repo state, read fresh: branch, uncommitted files, recent commits, open PR, CI status. Not memory; *context*. Joined with memory at orientation/re-entry so recall reflects what the session is probably about. §8.

### 1.2 The four triggers

| | Trigger | Lifecycle event | Analogy | Status |
| --- | --- | --- | --- | --- |
| **T0** | Orientation | SessionStart | walking into the office | reshape existing |
| **T1** | Associative recall | UserPromptSubmit (+ SubagentStart) | remembering while you work | reshape existing |
| **T2** | Re-orientation | PreCompact / resume | coming back after a gap | **net-new hook** |
| **T3** | Deliberate recall | `memory_search` / `memory_get` | consciously trying to remember | exists; fix discoverability |

Plus one non-injection trigger that closes the loop:

| | Trigger | Lifecycle event | Role | Status |
| --- | --- | --- | --- | --- |
| **C0** | Closeout | SessionEnd / Stop | write the continuity state | **net-new hook** |

### 1.3 The lifecycle loop

```
   work ──▶ closeout ──▶ dream-time ──▶ orientation ──▶ work
 (T1/T2)   (C0: snapshot   (refine model,   (T0: read it
            + stage note)    decay/boost)      back, + desk)
```

The session's end writes its successor's beginning; the quiet hours in between maintain the model and compost the noise. Forgetting is a feature: a recollection surfaced repeatedly and never *used* is noise the system hasn't yet learned to stop surfacing (§5.3).

---

## 2. Safety invariants

All Stream E v0.5 §2 invariants carry forward unchanged: recall is read-only; no encrypted plaintext in recall; governance lifecycle is authoritative; tombstoned/superseded records do not teach; candidates/quarantines are attention, not truth; the token estimator is deterministic (`ceil(utf8_byte_len / 4)`); output is byte-stable for cacheability; errors are typed.

This spec adds:

1. **Recollections are declarative and attributed, never imperative.** An injected memory ("Always do X") is reframed as reported fact ("Recalled — the standing practice has been X") before emission. The existing `neutralize_imperative_prose` path is the mechanism; v1.0 makes attributed-declarative the contract, not a best-effort. Rationale: injected memory is a prompt-injection surface, and the more it reads as the agent's own voice, the more a poisoned memory costs.
2. **Provenance is always recoverable.** Every surfaced recollection carries a `ref` to its canonical memory. The agent (and an injection detector) can always trace a recollection to its source; recall never emits free-floating instructions.
3. **The continuity-state object is governed and classified like any memory.** It is written through the Stream A write path, passes Stream C governance, and is classified by Stream D. It must never embed encrypted plaintext, secret-class content, or unreviewed candidate claims (§4.4).
4. **The desk read is read-only and fail-open.** Reading git state never mutates the repo and never blocks recall; any failure degrades to memory-only orientation (§8.3).
5. **Closeout is read-mostly and bounded.** The SessionEnd hook writes at most the continuity-state memory and staged notes through the governed write path; it never bulk-imports a transcript and never blocks harness shutdown beyond its deadline (§5.1).

---

## 3. The four triggers

Each trigger is dispatched by the existing unified `recall hook` handler (`cli/recall_hook.rs`), which maps a `hook_event_name` to a daemon request. v0.5 wires three events (SessionStart, UserPromptSubmit, SubagentStart) via `hooks_wire.rs`; v1.0 adds PreCompact and SessionEnd to the matcher tables and the unwire `HOOK_EVENTS` set, keeping wire/unwire symmetric.

### 3.1 T0 — Orientation (SessionStart)

**Fires:** SessionStart (existing matcher `startup|resume|clear|compact` for Claude; matcher-free for Codex).

**Reads:**
- the project's **continuity-state model** (§4) — a scoped index lookup, deterministic;
- the **skeleton**: pinned/active `me` identity + project `invariant`/`state`/`decision` memories (the existing `<identity>` / `<project-state>` candidate sources);
- the **desk** (§8): branch, uncommitted-file summary, recent commits, open PR, CI status.

**Injects:** a glance — what's nagging, where we left off, the desk crossed with memory. Leads with **unresolved × consequential**, not latest. Flags recent pivots that override a stale assumption ("priority changed last session"). Renders as prose recollections (§7), not an atom list.

**Budget:** target ≤ 300 estimated tokens, hard cap ≤ 600 (replaces `HOOK_STARTUP_BUDGET_TOKENS = 1900`). Never truncates mid-entry; if over budget, drops lowest-salience open loops and records them as omissions.

**Net-new vs today:** continuity-state read, desk read, prose rendering, salience-by-stakes ordering, the smaller budget. The candidate-collection and namespace machinery is reused.

### 3.2 T1 — Associative recall (UserPromptSubmit, SubagentStart)

**Fires:** UserPromptSubmit (and SubagentStart, with the subagent's task as cue). Maps to the existing `Delta` request path (`build_delta_response`, `passive: true`).

**Cue:** the submitted message + a bounded window of recent conversation/tool state (§6.1). This is the spreading-activation input.

**Gating — the heart of the change (§6):**
- A cheap **friction pre-gate** decides whether to surface at all: routine acknowledgements ("yes, do that") surface nothing; error output, decision-point and difficulty signals, and topic novelty raise the surface probability.
- A **relevance gate** then admits only recollections whose activation clears a high bar. If none clear it, inject nothing — and that is the system working correctly.
- **Lessons** (`feedback`/correction memories) get a salience boost at decision/difficulty points — protective recall, surfaced exactly when the agent is about to do the hard thing again.
- Dedup against what is already in context this session (§6.4): never re-surface a recollection already shown, and never restate what the native memory head already carries.

**Injects:** ≤ 3 recollections by default, as prose conclusions (§7).

**Budget:** ≤ 360 estimated tokens (reuses `HOOK_DELTA_BUDGET_TOKENS`), but spent on the *margin* — usually far under, often zero.

**Net-new vs today:** the friction pre-gate, the relevance term in ranking (§6.2), conversation-context dedup, silence-as-valid-output, prose rendering. The delta retrieval path and budget constant are reused.

### 3.3 T2 — Re-orientation (PreCompact / resume) — net-new

**Fires:** a new PreCompact hook (Claude exposes it; Codex via the nearest equivalent or a resume SessionStart with the `compact` token). Also fires on explicit resume after a long idle gap.

**Reads:** the immediate sub-task — the rolling session focus and the most recent open loop — not the whole project. Finer than orientation, more local than per-turn.

**Injects:** "here's what we were *just* doing" — a single compact re-orientation recollection plus the live desk delta since the session started.

**Budget:** ≤ 200 estimated tokens.

**Rationale:** context compaction is exactly the "got interrupted, need to re-orient to the immediate task" moment, and today nothing fires there. This is the cheapest high-value net-new trigger.

### 3.4 T3 — Deliberate recall (the tools)

`memory_search` (`{ query, limit, include_body }`) and `memory_get` (`{ id, include_provenance }`) already exist (`mcp.rs`). v1.0 changes one thing: **discoverability.** The `guidance` string returned by T0/T1 (today the generic `"Memorum passive recall assembled from read-only index projections."`) must name the tools and the fact that every recollection's `ref` is a fetchable handle — e.g. *"Fetch any recollection's full source with `memory_get <ref>`; search memory directly with `memory_search`."* This converts the `ref` attribute from dead weight into an affordance and makes the passive/deliberate split legible to the agent.

---

## 4. The continuity-state object

The spine of the redesign. Today nothing maintains a "where we left off" or project-state summary — the `<project-state>` block is just the project identity binding (`project_body` emits only the project id + namespace). v1.0 introduces a maintained model.

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

The continuity-state object is a **canonical Stream A memory**, not a private store (preserves the v0.5 §1 "no hidden second persistence layer" invariant). One per project, scoped `project:<canonical_id>`, reserved tag `continuity-state`, pinned status. It is rewritten by **supersede** (Stream C `WriteMode`), so its history is the event log and it merges across devices via the existing merge driver. `version` increments each rewrite.

This means orientation reads it via the ordinary indexed `MemoryQuery` (status `pinned`, namespace `project:<id>`, tag `continuity-state`) — cheap, deterministic, no new query surface.

### 4.3 Who writes it

- **Closeout (C0, §5.1)** writes the cheap, deterministic layer: appends/updates `open_loops` and `staged_notes` from the session's agent-supplied summary, bumps `volatile` minimally. No LLM call on the hot path.
- **Dream-time (§5.2)** writes the synthesized layer: refines `skeleton` and `volatile`, dedups and re-ranks `open_loops`, prunes resolved/stale loops, using the existing pass-1/pass-2 machinery. This is where the heavy synthesis lives, off the hot path.

### 4.4 Privacy and governance

The continuity-state memory passes Stream C governance and Stream D classification like any write. It must never embed encrypted plaintext, secret-class content, or unreviewed candidate claims. Each `StateClaim.text` and `OpenLoop.text` runs through `safe_plaintext_fragment` before persistence; a fragment that classifies non-`Allow` is dropped (not encrypted into the summary). Because it is `project`-scoped and synced, it is subject to the same two-clone convergence guarantee as any canonical memory.

---

## 5. The lifecycle loop

### 5.1 Closeout (C0) — net-new SessionEnd hook

**Fires:** a new SessionEnd (Claude `Stop` / `SessionEnd`; Codex equivalent) hook, wired into `hooks_wire.rs` matcher tables and `HOOK_EVENTS`. The daemon gains a `RequestPayload::Closeout` variant (the first session-end surface in the protocol).

**Input:** a bounded, agent- or harness-supplied **session summary** — at most a few hundred tokens describing where things landed, the unfinished thread, and any deliberate staged note. The agent authors this (e.g. via the existing `memory_note` surface or a closeout-specific structured field); if absent, closeout falls back to a minimal auto-snapshot (active project + the session's touched entities). Closeout never ingests a full transcript.

**Effect:** updates the continuity-state object (§4.3): records the open loop(s), appends staged notes, minimally refreshes `volatile`. Through the governed write path; fail-open; bounded by a deadline (≤ the 2 s hook timeout) so it never blocks harness shutdown.

**Determinism note:** closeout *writes*, so it is exempt from the read-path byte-stability invariant — but its writes go through the normal event log and merge driver and are themselves reproducible given the same summary input.

### 5.2 Dream-time maintenance

Reuses the Stream F nightly pipeline (launchd, lease-elected, shells to the harness CLI). v1.0 adds a continuity-maintenance pass alongside the existing three:

- **Refine** `skeleton`/`volatile` from the period's active+candidate memories and the accumulated closeout snapshots.
- **Re-rank and prune** `open_loops`: drop loops whose referenced work is resolved (the referenced memories superseded/closed), merge duplicates, re-score `stakes`.
- **Decay/boost** feeds from the use-feedback signal (§5.3), not from inclusion counts.

This pass writes a new continuity-state version via supersede. It is the only place the *synthesized* layer changes; closeout only touches the cheap layer. Reuses pass-1 masked-reflection and pass-2 candidate-write machinery; the continuity-state write is governed exactly like a pass-2 candidate except it targets the reserved pinned `continuity-state` memory.

### 5.3 The use-feedback signal — net-new

Today, strength's frequency term is driven by `RecallHit` events, which fire for every memory *included in a rendered active-path block* — i.e. **inclusion, not use.** The passive path emits nothing, so passive surfacing currently feeds the model not at all. v1.0 separates three signals:

- **`PassiveSurfaced`** — a recollection was injected passively. New event; lets us measure surfaced-but-unused (the noise we want to compost).
- **`RecallHit`** — included in a block (existing; rename-neutral).
- **`RecallUsed`** — the agent demonstrably acted on a surfaced recollection. v1.0 operationalizes this conservatively: an explicit `memory_get` on a surfaced `ref` within the same session is a strong "used" signal; the cheapest reliable version ships first. Softer signals (next-turn n-gram overlap with the recollection; same-session edits to the files/entities the memory concerns) are deferred enhancements (§16).

Strength's frequency term (`dynamics/strength.rs`) is re-weighted to prefer `RecallUsed` over bare inclusion, and dream-time decay consumes `PassiveSurfaced`-without-`RecallUsed` as the disuse signal. This is what makes "forgetting is a feature" real: a recollection that surfaces and is never used cools off and stops surfacing. No persisted strength column is introduced — the computation stays recompute-at-recall, now over a richer event set.

---

## 6. Relevance, gating, and the cue

### 6.1 The cue

- **T0:** no user message yet → the cue is the desk (§8) + the continuity-state object. Orientation is not a semantic retrieval; it is reading a maintained model.
- **T1:** the submitted message + a bounded rolling window of recent turns/tool state.
- **T2:** the rolling session focus + most recent open loop.

### 6.2 Activation scoring

The existing structural score (v0.5 §8.2: status + scope + entity-match + recency + confidence + source, plus the bounded strength term) becomes the **base**. v1.0 adds the missing organ: a **relevance-to-cue term** for the T1/T2 paths.

```
activation = base_structural_score
           + relevance_to_cue        // NEW: semantic/lexical match of memory ↔ live cue
           + lesson_boost            // NEW: feedback/correction memories at decision/difficulty points
```

`relevance_to_cue` reuses the existing chunk/vector retrieval already on the delta path (`query_chunks` over the message) plus entity-seed overlap; it is the term that makes recall *relevant* rather than merely recent. On T0 there is no cue, so this term is zero and orientation rests entirely on the maintained model + desk — by design.

Determinism is preserved per §9: given the same index state and cue, scoring is reproducible; the vector path already has a `vector_recall_degraded` soft-fail flag that keeps recall structural-only on retrieval failure.

### 6.3 The friction pre-gate (T1)

Before paying for ranked surfacing, a cheap pre-gate decides whether this turn should surface anything at all. Tier 1 signals (lexical + tool-state, no embedding):

- recent tool output contains an error/failure;
- the message carries a decision/difficulty signal (question form; "should we", "stuck", "broken", "why", "approach", "option");
- the message introduces an entity not seen this session (novelty).

Routine turns with none of these surface nothing. Tier 2 (optional, configurable): an embedding-centroid drift signal for topic shift, deferred to a later phase to keep per-turn cost down (§16). The pre-gate decides *whether to surface*; the relevance gate decides *what*.

### 6.4 Gating discipline

- **Gate, don't truncate.** Selection admits a recollection only while its activation clears the relevance floor *and* its full rendered cost fits the budget. Stop when either fails. Truncation (§7.5) is a last-resort backstop, not the primary bound.
- **Silence is valid output.** Zero recollections is a correct, common T1 result. The CLI/daemon still emits the empty wrapper (`<memory-recall empty="true" />` analog) so downstream parsing never branches on emptiness — consistent with the v0.5 delta-empty contract.
- **Dedup against working context.** Extend the existing native-memory-head dedup (`read_native_memory_head`) to also suppress recollections already surfaced earlier in the same session and content already present in the loaded CLAUDE.md/AGENTS.md head. Don't re-tell the agent what's already on screen.

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

- The body is **declarative attributed prose** — the conclusion, not a heading. Bounded to a single short paragraph.
- `ref` is preserved as an attribute (the Stream H eval parser depends on a `ref` contract; §15 updates the parser to read `<recall ref=...>` rather than `<memory ref=...>`).
- `kind` ∈ { `lesson`, `state`, `open-loop`, `staged-note`, `fact` } drives presentation and the lesson boost.
- `confidence` is preserved; `updated`/`source` are dropped from the surface form (provenance recoverable via `memory_get <ref>`), eliminating ~50 chars/entry of scaffolding the model cannot use as prose. Microsecond timestamps and opaque source kinds do not appear in the injected text.
- **No empty elements.** A recollection with no usable body is not surfaced at all (this both fixes and generalizes `500a60d`'s empty-snippet omission, and resolves the v0.5 §5 "snippet always present" contradiction — that contract is retired with the `<memory>` element).

### 7.2 Block envelope

The top-level `<memory-recall ...>` envelope and the `<recall-explanation>` accounting block (v0.5 §3.3, §9.6) are retained — they carry the omission/budget metadata Stream G and the eval harness consume. What changes is the *content* sections: orientation renders continuity-state + desk as labeled prose groups; T1/T2 render a flat list of `<recall>` units.

### 7.3 Attributed-declarative rendering

Every surfaced body passes `neutralize_imperative_prose` (now contract, not best-effort — §2.1). An imperative source ("Always run the gate before pushing") renders as reported fact ("Recalled — the standing practice has been to run the gate before pushing"). Declarative sources pass through byte-identically so the determinism tuple stays stable.

### 7.4 Budget on rendered bytes

The root cause of the original overflow: selection counted **summary tokens only** (`estimated_tokens(&summary)`), while each rendered entry also carried ~120 chars of scaffolding, a 35-char ref, and a timestamp — so "within budget" undercounted real size by multiples, and ~30 entries blew past the 10 k-char cap. v1.0 budgets on the **full rendered cost** of each unit (attributes + body + envelope scaffolding). The per-entry-count cap (`HOOK_RECENT_MEMORY_MAX_ENTRIES`) becomes a redundant safety net rather than the primary defense.

### 7.5 Well-formed truncation

Today's `cap_passive_block` cuts to the last newline and appends `</memory-recall>`; if the cut lands inside an entry, the result is an unclosed element — malformed XML (observed in the original paste: an unclosed `<memory>` followed by `<recall-truncated/>`). v1.0 requires truncation to cut at a **recollection boundary** (a complete `<recall>…</recall>` unit), so the emitted block is always well-formed. The backstop should almost never fire given byte-budgeting (§7.4), but when it does it must not emit malformed output.

### 7.6 Per-trigger budgets

| Trigger | Target | Hard cap | Default entries |
| --- | ---: | ---: | ---: |
| T0 orientation | 300 | 600 | continuity model + desk |
| T1 associative | margin (often 0) | 360 | ≤ 3 |
| T2 re-orientation | 120 | 200 | 1 + desk delta |

---

## 8. The desk read (repo state ingestion) — net-new

Today git is read *only* for project binding (`git remote get-url origin`, `git rev-parse --show-toplevel`, both under a 2 s deadline). No branch, status, log, PR, or CI is ingested.

### 8.1 Inputs

On T0 and T2, read (each under a per-command deadline, all best-effort):

- current branch (`git rev-parse --abbrev-ref HEAD`);
- uncommitted-work summary (`git status --porcelain`, counted/summarized, never file contents);
- recent commits (`git log --oneline -n N`);
- open PR for the branch (`gh pr view`, if `gh` is present and authenticated);
- CI status (from the PR view, if available).

### 8.2 Join with memory

The desk is crossed with the continuity-state object: "you're on `codex/x-spend-opt`, 8 commits deep, CI green — last session you flagged radar tuning as the next thread." The desk anchors orientation; memory annotates it.

### 8.3 Read-only, fail-open, bounded

No command mutates the repo. Each runs under a deadline; any failure (no git, no `gh`, timeout) degrades silently to memory-only orientation. Total desk-read budget is bounded so it never dominates the T0 hot path (§9).

### 8.4 Determinism

The desk is live state and therefore not deterministic across real time — but the byte-stability invariant (§9) is, as in v0.5, conditioned on "the same repo state." Given a fixed repo snapshot, continuity-state version, and clock fixture, T0 output is byte-identical. Tests fix the desk via a fixture repo, exactly as v0.5 fixes the index.

---

## 9. Determinism, caching, and performance

### 9.1 Byte-stability

Carries forward v0.5 §2.7, extended: given the same **repo state, continuity-state version, request context, budget, and clock fixture**, T0/T1/T2 emit byte-identical blocks. The continuity-state `version` is part of the cache key.

### 9.2 The per-turn cost

T1 already pays a chunk/vector retrieval per prompt today. The friction pre-gate (§6.3) is a *reduction*: routine turns skip ranked surfacing entirely. The relevance term reuses the retrieval already on the path. Net per-turn cost should not exceed today's delta path; the pre-gate should reduce it on the common (routine) turn.

### 9.3 Performance budgets

Release-gate fixture sizes (warm path), adapting v0.5 §13:

- T0 orientation, 1 000 memories: p95 ≤ 250 ms (continuity-state read is one indexed lookup; desk read is bounded, parallelizable, and fail-open).
- T1 routine turn (pre-gate rejects), 1 000 memories: p95 ≤ 40 ms.
- T1 surfacing turn (five matching entities): p95 ≤ 120 ms.
- T2 re-orientation: p95 ≤ 120 ms.
- C0 closeout write: p95 ≤ 200 ms; never exceeds the hook deadline.
- Desk read must not add more than 150 ms p95 to T0 and is fully skippable on timeout.

Cold-start (first call after boot) ≤ 600 ms at 1 000 memories, as v0.5.

### 9.4 Observability counters

Extends v0.5 §13.1 additively:

- `recall.orientation_invoked_total`, `recall.reentry_invoked_total`, `recall.closeout_invoked_total`;
- `recall.t1_surfaced_total`, `recall.t1_silent_total` (the silence rate is a primary health metric — a system that surfaces on most turns is mis-gated);
- `recall.friction_gate_fired_total`;
- `recall.passive_surfaced_total`, `recall.recall_used_total` (the trust/usefulness ratio);
- `recall.desk_read_degraded_total{reason}`.

---

## 10. Privacy and safety (Stream D)

Unchanged authority: Stream D owns classification, encryption, and reveal. v1.0 adds two consumption points:

- Every `StateClaim`/`OpenLoop`/`StagedNote` text and every synthesized recollection body passes `safe_plaintext_fragment` before persistence/emission. Non-`Allow` fragments are dropped, not encrypted into prose.
- The continuity-state object and closeout summary are subject to full classification at write time; a closeout summary carrying secret-class content is refused exactly as any write (`SecretRefused`), never silently stored.

`memory_startup`/T0/T1/T2 still never call `memory_reveal` and never emit ciphertext or masked-body projections.

---

## 11. Cross-stream surface changes

Implementation lands these additive surfaces on shipped streams. Like v0.5 §1.1, they are part of this contract.

- **Protocol (Stream A/daemon):** new `RequestPayload::Closeout { cwd, session_id, harness, summary: Option<ClosoutSummary> }` and `RequestPayload::Reorient { … }` variants; `ResponsePayload` analogs. PreCompact/SessionEnd added to `hooks_wire.rs` matcher tables and `unwire.rs` `HOOK_EVENTS` (kept symmetric).
- **Stream A events:** new `EventKind::PassiveSurfaced` and `EventKind::RecallUsed` (§5.3), alongside existing `RecallHit`.
- **Stream A memory kind:** the reserved `continuity-state` pinned memory per project (§4.2). No schema change — it is an ordinary memory with a reserved tag.
- **Stream C governance:** governs the continuity-state write/supersede like a pass-2 candidate, with a policy carve-out so a system-authored continuity update is not gated as `dream_source` low-confidence noise.
- **Stream D:** no new surface; consumes existing `safe_plaintext_fragment`.
- **Stream F dreaming:** the continuity-maintenance pass (§5.2) added to the nightly pipeline.
- **Stream G observability:** the new counters (§9.4) and a trust/usefulness panel (`passive_surfaced` vs `recall_used`).
- **Stream H eval:** parser reads `<recall ref=...>` (§15).

---

## 12. Invariants (consolidated)

A change failing any of these fails review:

1. Recall (T0/T1/T2/T3) is read-only and never reveals ciphertext (v0.5 §2.1–2.2).
2. Only `active`/`pinned`, `passive_recall = true`, non-pending-review memories surface as facts (v0.5 §2.3–2.5).
3. Output is byte-stable given repo state + continuity-state version + request context + budget + clock (§9.1).
4. Every surfaced recollection is declarative, attributed, and carries a recoverable `ref` (§2.1–2.2).
5. No contentless entry is ever surfaced (§7.1).
6. Truncation, if it fires, cuts at a recollection boundary and emits well-formed output (§7.5).
7. The continuity-state object is a governed, classified canonical memory — never a private store, never carrying secret/unreviewed content (§4.2, §4.4).
8. Closeout and desk read are fail-open and never block the harness beyond the hook deadline (§5.1, §8.3).
9. Silence is a valid output; the empty wrapper is always emitted (§6.4).
10. No LLM/network call on any synchronous recall hot path (T0/T1/T2). Synthesis is dream-time only (§5.2).

---

## 13. Phased build plan

Each phase is independently shippable and testable; value lands before the whole is built.

**Phase 1 — Rendering and budget (fixes the pasted-noise problem outright).** Prose `<recall>` unit (§7.1); byte-budgeting (§7.4); entry-boundary truncation (§7.5); discoverable guidance string (§3.4); shrink T0 to high-signal; retire the `<memory>`/empty-snippet contract. No new triggers, no new persistence. This phase alone makes the channel trustworthy.

**Phase 2 — Relevance and gating (makes T1 real).** Add the relevance-to-cue term (§6.2); friction pre-gate (§6.3); conversation-context dedup (§6.4); silence-as-output. Reuses the delta retrieval path.

**Phase 3 — Continuity (closeout writes startup).** The continuity-state object (§4); SessionEnd closeout hook + protocol variant (§5.1); dream-time maintenance pass (§5.2); staged notes. The structural heart of the redesign.

**Phase 4 — Desk, re-entry, and feedback.** Desk read (§8); T2 PreCompact re-orientation (§3.3); the use-feedback signal and decay/boost (§5.3). The "anchor on the one persistent channel" and "forgetting is a feature" layers.

---

## 14. Open questions (decisions needed before/while building)

1. **Closeout authorship.** Should the session summary be agent-authored (the agent calls a closeout note before Stop) or harness-captured? Agent-authored is higher-signal but depends on the agent reliably doing it; the auto-snapshot fallback covers the gap. Recommend agent-authored with fallback; confirm.
2. **PreCompact availability across harnesses.** Claude exposes PreCompact; Codex's equivalent is unverified. T2 may ship Claude-first and degrade to a resume-SessionStart heuristic on Codex.
3. **Relevance floor calibration.** The high bar is the whole game and is empirical. Needs the Stream H eval harness to tune the floor against a labeled "should this have surfaced?" set — this is exactly the eval-gated-knob-sweep pattern.
4. **`RecallUsed` precision.** Starting with `memory_get`-on-ref is conservative and may under-count genuine use. The softer signals (§5.3) need their own validation before they feed decay.
5. **Continuity-state merge semantics.** Two devices ending sessions on the same project produce concurrent continuity-state supersedes. The merge driver must converge them (open-loop union, version max); confirm this fits the existing canonical-content-equality convergence rather than needing a bespoke merge.

---

## 15. Acceptance signals

Implementation of a phase is complete when its tests/docs exist and pass. Per phase:

- **Phase 1:** `recall_render` tests assert the `<recall>` prose unit, no empty elements, byte-budgeting (a fixture that overflowed under summary-only budgeting now selects correctly), entry-boundary truncation (a forced-overflow fixture emits well-formed output), and the guidance string names `memory_search`/`memory_get`. Stream H eval parser updated to `<recall ref=...>` with a passing regression.
- **Phase 2:** `recall_gating` tests assert silence on routine-turn fixtures, surfacing on friction fixtures (error / decision / novelty), the relevance term changes ordering vs structural-only, and conversation-context dedup suppresses an already-surfaced ref. Determinism test extended to the cue path.
- **Phase 3:** `continuity_state` tests assert closeout writes/supersedes the pinned object, dream-time refines it, orientation reads it deterministically, staged notes surface once then clear, and the object passes governance + `safe_plaintext_fragment` (a secret-class closeout summary is refused).
- **Phase 4:** `desk_read` tests assert fail-open on missing git/`gh`, byte-stability given a fixture repo, and the join with continuity state; `use_feedback` tests assert `PassiveSurfaced`/`RecallUsed` events fire correctly and that disuse decays a recollection out of surfacing.
- Docs: `docs/api/stream-e-ambient-recall-api.md`; updates to the Stream A/C/F/G/H API docs for the §11 surfaces; `CLAUDE.md` authoritative-docs table repointed; `STREAM_E_POLICY` bumped to `stream-e-v1.0` — all only after the relevant phase's tests pass.

## 16. Explicit deferrals

- Embedding-centroid topic-drift as a Tier-2 friction signal (§6.3) — Phase 4+ once per-turn cost is measured.
- Softer `RecallUsed` signals beyond `memory_get`-on-ref (n-gram overlap, file-touch correlation) (§5.3).
- Cross-session real-time continuity merge UI (Stream I surface).
- Per-harness closeout-summary capture beyond agent-authored + auto-snapshot.
- A daemon-cached doctor projection in `<pending-attention>` (inherited deferral from v0.5 §9.5).

If a phase's acceptance tests cannot pass without one of these, revise this spec before coding continues.
