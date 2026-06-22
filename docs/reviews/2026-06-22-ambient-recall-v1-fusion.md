# Fusion council review — Stream E Ambient Recall v1.0

**Artifact:** `docs/specs/stream-e-ambient-recall-v1.0.md` (draft-for-review; ground-up Stream E
passive-recall redesign, superseding the *approach* of `stream-e-passive-recall-v0.5.md`).
**Conceptual narrative:** `docs/explainers/2026-06-22-ideal-agent-memory-hooks.html`.
**Date:** 2026-06-22. **Run:** cross-vendor fusion (propose → critique → defend → synthesize →
external judge). **Scope:** architecture bet — *is this the right design, and where does it break?* —
not a copy-edit. This review does **not** edit the spec.

**Council (decorrelated families):** proposers — Codex (OpenAI), Cursor/Composer (Anysphere),
Grok-4.3 (xAI), DeepSeek-V4-Pro (DeepSeek). Host synthesizer — Opus (Anthropic). Clean external
judge — Gemini-3.5 (Google), a family absent from both the proposer bench and the synthesis spine.
**Participation note:** the Kimi (Moonshot) lane was the originally-planned 4th proposer but failed
to launch three times (standalone `kimi` binary auth, unrelated to the Fireworks key); it was
replaced by DeepSeek-V4-Pro to preserve four distinct families. Every stage ran in work mode in
disposable scratch repos; the spec was never touched. All eight expected delegate runs (4 propose,
4 critique) + 1 defend + 1 judge completed.

**Ground-check before convening (verified against live code, so the council critiqued reality):**
all five of the spec's load-bearing build-boundary claims hold — `build_delta_response` exists
(`recall/delta.rs:51`); there is **no** session-end/closeout surface in the daemon protocol; the
ranking `score_candidate` (`rank.rs:86`) has no semantic relevance-to-cue term (entity-match is a
partial proxy); strength's frequency term is driven by `RecallHit` = inclusion, and the passive path
emits none; confidence is a near-flat default (0.7 import / 0.85 hand-written). The one nuance fed to
the council: O1's "ranking lacks a relevance term *entirely*" slightly overstates, since entity-match
+ vector retrieval already give partial cue-relevance (the spec's own §6.2 concedes this).

---

## The 5 things to actually change (read this first)

1. **Recall must stay strictly read-only — do not "narrow the invariant" to allow it.** The spec
   declares recall read-only (inv §12.1), yet `PassiveSurfaced`/`RecallUsed` event appends (§5.3) and
   `StagedNote.consumed` clearing (§4.1, acceptance §15) are writes *caused by* recall. The fix is
   **not** to relax the invariant; it is to execute all telemetry/consume writes **out-of-band** —
   harness-driven, or a separate decoupled post-render daemon endpoint — with zero side effects on
   the read path. (B1. The judge overrode the synthesis's softer phrasing here; this is the single
   highest-confidence finding in the run.)

2. **The continuity loop is unsafe as specified — gate it on substance, not a timestamp, and don't
   ship the object until merge + read-only are solved.** Closeout-writes-startup with no acceptance
   gate inverts the failure mode from v0.5's *noisy irrelevance* (ignorable) to *confident wrongness*
   (trusted, because it's prose, attributed, small, desk-anchored). An `updated_at` freshness gate is
   defeated by the spec's own fail-open auto-snapshot, which writes a *fresh* timestamp on a *hollow*
   object. Require a completeness/quality contract + desk-contradiction check + "remembered, not
   verified" framing (B2); specify item-level cross-device merge semantics before Phase 3 (B3). (All
   four families flagged the poison pill; the fresh-but-hollow defeat was sharpened at the critique
   stage.)

3. **Get subprocesses off the synchronous recall hot path.** `gh pr view` + CI (§8.1) are network
   calls on T0/T2 — a direct violation of inv §12.10 (B4). The judge generalized this: even
   `git status --porcelain` can block on `.git/index.lock` (concurrent IDE/editor) or take seconds in
   a monorepo, blowing the 150 ms budget. The desk read must be a **daemon-cached background
   projection** read in O(1), not synchronous subprocess calls inside the hook.

4. **Decide the real architectural split — and it is a split, not a consensus.** All four families
   showed a viable *continuity-free 80% design* exists (desk-first orientation + relevance-gated
   delta + prose rendering). But they genuinely disagree on whether to build it: **Option A
   (defer-and-simplify** — Codex/Cursor: ship the simpler channel, prove adoption, defer the
   `ContinuityState`+closeout cathedral until its hazards are solved) vs **Option B (continuity-first**
   — Grok/DeepSeek: passive recall is structurally useless without an automated maintained continuity
   model; build it early despite the hazards). This is your call, not the council's — see "The central
   decision" below.

5. **`memory_get`-on-ref is a positive-only signal; never infer disuse from its absence.** Ambient
   recall succeeds precisely when the agent acts on the self-contained prose *without* fetching, so
   absence ≠ unused. Negative decay from absence (§5.3, made live by Phase 4 acceptance tests) will
   cool exactly the recollections that worked (B5).

---

## Verdict

**The direction is right; the build order and center of gravity are wrong, and several mechanisms
are unsafe as specified.** (External judge: **RATIFY-WITH-CORRECTIONS**, high confidence on the
verdict and on all five core blockers.)

The diagnosis of v0.5 is correct, and the durable wins should land: the prose recollection unit,
byte-budgeting (§7.4), well-formed truncation (§7.5), the desk-read *concept* (§8), tool
discoverability (§3.4), and keeping new state as a governed Stream A memory rather than a private
store. The risky center is the **continuity loop + feedback model** (closeout-writes-startup, the
`ContinuityState` object, `RecallUsed`-driven decay), which as written trade noisy irrelevance for
confident wrongness.

**Confidence:** *high* that the blockers below are real and the continuity loop should not ship as
specified; *medium* on the exact phase plan, because the central Option-A/Option-B decision is a
genuine product call the page cannot settle.

---

## The central decision (Option A vs Option B) — for the human gate

The synthesis initially framed the four families as *converging* on a simpler design. The external
judge correctly flagged that this flattens a real split, and that a manual pinned focus-note is not
the same artifact as an automated system-maintained continuity state. Stated honestly:

- **What is genuinely shared:** a continuity-free design (desk-first T0 + relevance-keyed delta +
  prose rendering) is viable and captures most of the user-visible value (clean, relevant, trusted
  recall) at ~40–50% of the build, with none of the merge/poisoning/read-only hazards. Every family
  sketched a version of it.
- **Where they truly split:**
  - **Option A — Defer-and-Simplify (Codex, Cursor).** Ship the simpler channel first. The
    `ContinuityState` struct, the `RequestPayload::Closeout` protocol, and the dream-maintenance pass
    introduce severe merge, freshness, poisoning, and read-only hazards; defer them until adoption is
    proven and those hazards are specified away.
  - **Option B — Continuity-First (Grok, DeepSeek).** A prettier recency channel is a cosmetic
    bandage; the *remembered-not-retrieved* inversion is the whole point. Build a minimal maintained
    continuity model (skeleton + volatile) early — Grok in Phase 1, DeepSeek in Phase 2 — and accept
    the hazards as work to be done, not reasons to defer.

**Recommendation (not a council mandate):** lead with the Option-A spine because it ships value while
every Option-B hazard (B1–B5, B7) is still unresolved — but treat the choice as yours. If you take
Option B, B1/B2/B3/B7 become hard pre-requisites, not deferrals.

---

## Disagreement map (where the families split — the signal)

**1. §7.1 — prose body that still carries a `ref` attribute. (Framing split, 3–1.)**
Codex: "right compromise." Cursor: "necessary fudge, fix the wording." DeepSeek: "right, but the
opaque hash is the wrong *kind* of pointer — add a human-readable alias." Grok: "category error,
resolve before Phase 2." **Resolution:** keep `ref` (inv §2.2/§12.4 require recoverable provenance;
the Stream H parser needs it); fix O2's "conclusions, not pointers" wording to "conclusions in body;
pointer in attribute for traceability." Do **not** add DeepSeek's alias — it creates a second
identifier that diverges across devices and breaks eval-fixture stability (its own critic refuted
it). Grok's "category error" is the minority view; its valid kernel — that over-advertising
`memory_get <ref>` trains card-catalog behavior — is folded into N1. **Nit.**

**2. Where continuity goes in the plan. (The deepest split — see "The central decision.")**
Codex/Cursor: last. Grok: Phase 1 (skeleton+volatile). DeepSeek: Phase 2. Unresolvable from the page.

**3. Will `memory_get` adoption make `RecallUsed` usable? (Empirical crux.)**
DeepSeek: effective §3.4 guidance will train agents to call `memory_get`, yielding clean telemetry.
Codex/Grok: ambient recall works *without* the fetch, so the signal is near-zero by design.
**Resolution:** unresolvable from the page → human gate / telemetry. The safe move is invariant to
the outcome: positive-only signal, never negative inference (B5).

**4. Does Phase 1 have a source of prose conclusions? (Crux — resolved here by code inspection.)**
Codex: a deterministic renderer can't turn `reference-ingest — Tables that matter` into a lesson.
Cursor/Grok/DeepSeek: canonical bodies exist; render them and drop contentless units. **Resolution
(verified against the code):** the render surfaces `entry.summary` (+ optional `snippet`); for the
incident class — imported reference-doc chunks — `summary` is the heading fragment and the snippet
was empty, while `body` is raw section text, not a distilled lesson. **Both are partly right:** Phase
1 *does* fix the incident (drop contentless, byte-budget, render existing declarative summaries from
hand-written lessons/decisions as real prose), but it *cannot* deliver O2 "conclusions" for the
imported-doc class without a synthesis/gist source the spec defers to dream-time. **Implication:**
scope Phase 1's "makes the channel trustworthy" claim to "stops the noise" (R8).

---

## v1.1 punch list

### Blockers (design is wrong or will break as specified)

- **B1 — Read-only invariant contradiction.** `PassiveSurfaced`/`RecallUsed` appends (§5.3) and
  `StagedNote.consumed` clearing (§4.1; acceptance §15 "staged notes surface once then clear") are
  recall-time writes; inv §12.1 says recall is read-only. **Keep recall strictly read-only; execute
  telemetry/consume out-of-band (harness or a decoupled async post-render endpoint), never on the
  read path.** *(Codex at critique — the council's correlated blind spot; no proposer caught it.
  Judge: highest-confidence finding, and overrode the synthesis's "or narrow the invariant" option.)*

- **B2 — Closeout orientation has no acceptance gate, and a timestamp gate won't fix it.** T0 renders
  continuity-state as authoritative prose; fail-open closeout writes a fresh-timestamped *hollow*
  auto-snapshot in the common case. Require a completeness/quality contract (non-empty volatile /
  minimum evidence / explicit `degraded:true`), a desk-contradiction check, and "remembered, not
  verified" framing — before Phase 3. *(All four; fresh-but-hollow defeat sharpened by Cursor + Grok
  at critique.)*

- **B3 — Cross-device continuity merge is unspecified (§14.5).** Concurrent supersedes from two
  devices need item-level deterministic merge (stable IDs on `open_loops`/`staged_notes`, consume
  events, version reconciliation) or the open-loop list becomes a graveyard. Ship a two-device
  concurrent-closeout merge test as a Phase 3 gate; "confirm it fits canonical-content equality" is
  insufficient. *(All four; "single-supersede needs item-level semantics" is the durable framing —
  append-only-notes is one alternative, not a mandate.)*

- **B4 — Subprocess/network on the synchronous hot path.** `gh pr view` + CI (§8.1) violate inv
  §12.10 (network on hot path); even `git status --porcelain` can block on `.git/index.lock` or take
  seconds in a monorepo, blowing the 150 ms budget (§9.3) and starving large-repo users of desk
  context entirely. **The desk read must be a daemon-cached background projection** (file-watcher →
  in-memory desk state), read O(1) by T0/T2 — not synchronous subprocesses. *(Codex caught the
  network half — unique; judge added the git-lock/monorepo half. Same fix.)*

- **B5 — `RecallUsed`-from-`memory_get`-absence drives decay that cools working recollections.** §5.3
  + Phase 4 acceptance make disuse-decay live, not deferred (survived the spine author's own defense
  as "not merely a deferral"). Make `memory_get` positive-only; gate any negative signal on
  validation. *(Codex/Grok/DeepSeek blocker; Cursor risk.)*

- **B6 — Friction pre-gate is lexical, silent, and unfalsifiable.** *(Elevated from risk by the
  judge.)* A purely lexical gate (errors / keywords / novel entities) has a large silent
  false-negative rate on exactly the turns where memory matters most — procedural reuse ("do the same
  fix in billing"), social ("reply to Adam"), status continuations — and because the gate is
  unlogged, the failure is unfalsifiable in production, making T1 untestable. Mitigations: restrict
  the pre-gate to obvious no-ops (run the relevance gate on substantive prompts regardless of friction
  words); add a miss-signal (a `memory_search` shortly after a silent T1 = gate miss); add a
  deterministic self-test mode (sample ~1% of turns with forced surfacing, keyed by session-id so
  byte-stability holds, to measure precision without a labeled set). *(Grok + Codex + DeepSeek;
  DeepSeek's self-test with Codex's determinism caveat.)*

- **B7 — The §11 governance carve-out is an un-governed poisoning backdoor.** *(Elevated from risk by
  the judge.)* Letting system-authored continuity updates skip `dream_source` confidence gating
  directly violates inv §7 (Stream C stays authoritative) and promotes dream-synthesized
  hallucinations to pinned, authoritative startup orientation. Specify exact carve-out semantics +
  claim-level provenance (system-derived vs user-authored must be distinguishable in the surface).
  *(DeepSeek at critique.)*

- **B8 — Cue-driven T1 injection can bust the harness prompt cache.** *(Judge — missed by the entire
  council.)* If a per-turn recall block (0–3 memories, varying every turn) is injected into the
  cached prompt prefix, it invalidates prefix caching for the whole session history, turning every
  turn into a full uncached re-evaluation (latency + compounding token cost). This defeats the spec's
  own cacheability rationale regardless of per-block byte-determinism. **Mandate that dynamic T1/T2
  blocks render at the *bottom* of context (immediately before the latest user prompt), isolated from
  the static prefix.** *Caveat to verify:* severity depends on where each harness places hook output
  — if UserPromptSubmit hooks already append at the bottom, the prefix is safe; the defect is that the
  spec does not specify injection position at all. Confirm against actual Claude/Codex hook placement
  before sizing this.

### Risks (real hazards needing explicit mitigation)

- **R2 — Cold-start / bootstrap undefined.** The spec never says what T0 does when no
  continuity-state exists (first session per project, or a deleted object). If it degrades to a
  recency dump, the core bet fails on first contact. Define cold-start explicitly: desk + pinned
  invariants, no continuity object. *(DeepSeek at critique — unique.)*

- **R4 — Relevance floor uncalibrated and it gates the whole feature.** Don't default-on T1 gating
  until Stream H has a labeled "should-surface?" set; ship behind a flag with a permissive floor and
  tighten via eval. *(All four.)*

- **R5 — Continuity-state claims are outside the compost/decay loop.** The disuse signal covers
  recollections, not T0 orientation claims; a wrong "current focus" never decays. Add
  staleness/contradiction invalidation for continuity claims (desk mismatch, superseded refs).
  *(Cursor — unique failure mode.)*

- **R6 — Passive strength term stays inclusion-polluted through Phases 2–3.** The ranking base
  score's strength term counts `RecallHit` (inclusion), and the passive path emits none, so Phase 2
  eval calibration runs on a miscalibrated base. Sequence the feedback-signal fix relative to gating
  calibration, or calibrate explicitly on the structural-only base. *(Cursor at critique.)*

- **R7 — Privacy partial-drop can invert meaning.** `safe_plaintext_fragment` dropping a fragment can
  flip a claim ("don't mention X until legal" → "don't mention until legal"). A partially-classified
  continuity object should carry a degraded flag and not silently supersede last-known-good. *(Codex.)*

- **R8 — Phase 1 is oversold.** Even granting it renders existing bodies, "this phase alone makes the
  channel trustworthy" exceeds the evidence: relevance, gating calibration, feedback semantics, and
  continuity safety are all deferred. Scope the claim to "stops the noise." *(All four; see
  disagreement-map item 4.)*

- **R9 — Declarative rephrasing is insufficient injection safety.** *(Restored from the spine review
  by the judge.)* `neutralize_imperative_prose` is incomplete: a poisoned memory still steers as a
  reported fact ("Recalled — the standing practice has been to run script X"). Frame system-derived
  or low-confidence recollections as external, non-authoritative evidence ("A prior memory
  reports…"), not as internal facts. *(Codex; dropped in synthesis, restored by judge.)*

- **R10 — T2 "desk delta since session start" has nowhere to store its baseline.** *(Judge — missed
  by the council.)* Computing the delta needs a T0 baseline: on disk → second persistence layer (inv
  violation); as a Stream A memory → recall-time write (read-only violation); in-memory → stateful
  daemon that drifts on restart/crash. **Simplify T2 to render the *current absolute* desk state, not
  a delta** — eliminating the baseline and preserving both invariants.

### Nits

- **N1 — §7.1 wording.** Fix O2 "conclusions, not pointers" → "conclusions in body; pointer in
  attribute for traceability." Don't over-advertise `memory_get <ref>` to the point of training
  card-catalog behavior. (Do not add a human-readable alias.)
- **N2 — Typo** `ClosoutSummary` → `CloseoutSummary` (§11).
- **N3 — Dedup is too aggressive.** Add a turn-distance threshold so a recollection can re-surface
  after a meaningful gap / topic shift, instead of permanent intra-session suppression. *(DeepSeek.)*
- **N4 — Underspecified score ranges.** §6.2 needs testable ranges for `relevance_to_cue` and
  `lesson_boost`, or the ranking behavior is untestable. *(Codex.)*
- **N5 — Empty-wrapper exactness.** Specify the exact empty-wrapper form per trigger so harness
  parsers don't invent variants. *(Codex.)*
- **N6 — Protective-recall coupling.** `lesson_boost` fires only when the friction pre-gate fires, so
  a missed decision point kills protective recall even when the relevance gate would have selected the
  lesson. Give it an independent path or document the coupling. *(DeepSeek.)*
- **N7 — O1 conflates two relevances.** "Relevance over recency" bundles cue-relevance (T1, semantic
  match) and project-salience (T0, stakes ordering) under one slogan; they have different signals and
  failure modes. Separate them in the objective text. *(DeepSeek + Cursor.)*

---

## Preserved dissent

- **Grok alone** holds §7.1 is a "category error" to resolve before Phase 2 (majority: keep `ref`,
  fix wording).
- **The Option-A / Option-B architecture split is preserved as a live decision**, not resolved — see
  "The central decision." Grok and DeepSeek explicitly reject the defer-continuity framing as a
  cosmetic bandage even while sketching a continuity-free 80% design; that internal tension is real
  and is the honest state of the council.
- **The `RecallUsed` usability crux** (DeepSeek "guidance will train fetches" vs Codex/Grok
  "near-zero by design") is left to the human gate / telemetry.

## Provenance

- **Spine:** Codex (read-only contradiction, network-on-hot-path, merge, RecallUsed, Phase-1-source
  crux) — unanimously ranked #1 by all four critics, including Grok ranking its own brief last.
- **Grafts:** Cursor (severity discipline, continuity-claim-invalidation, fresh-but-hollow snapshot,
  strength-pollution); Grok (friction-gate audit trail, degenerate-closeout-common-case); DeepSeek
  (cold-start, carve-out poisoning vector, self-test mode, dedup turn-distance, O1 two-relevances).
- **External judge (Gemini, Google):** overrode the B1 invariant-narrowing option (C1), corrected the
  overstated convergence into the honest Option-A/B split (C2), elevated B6 + B7 from risk to blocker
  (C3/C4), restored R9 (C5), and contributed three defects the council missed (B8 cache-busting, the
  git-lock/monorepo half of B4, and R10 T2-delta-persistence).
- **Host (Anthropic):** orchestration, code-grounded crux resolution (Phase-1 source), and this
  synthesis.

## Methodology

Cross-vendor adversarial loop: 4 isolated proposals → blind anonymized cross-critique (each critic
refuted all four briefs and ranked them) → spine author defends the top-ranked brief (SURVIVES /
FALLS / CRUX triage) → host select-and-graft synthesis → clean external judge (different family from
every proposer and from the spine) for verdict, calibrated confidence, and latent-defect hunt. The
gain is decorrelated errors across families: the deepest finding (B1, the read-only contradiction)
came only from the critique stage, B4's network half from a single family the others missed, and
B8/R10 only from the external judge — none would have surfaced from a single model.
