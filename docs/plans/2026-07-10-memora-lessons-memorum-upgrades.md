# Plan: Memora lessons → Memorum upgrades

**Status:** r4 DRAFT — foundry execution plan, review rounds 1–2 folded in; round-3 convergence re-read pending. Do not execute until Trey approves.

## Plan revision history

- **r1 (2026-07-10):** design analysis from a source read of `microsoft/Memora` (`8ca1532`).
- **r2 (2026-07-10):** foundry execution plan — waves, lanes, gates, budget (`d419b2b`).
- **r3 (2026-07-10):** round-1 rebuild (Sol xhigh + Opus plan-reviewer; 8 blockers accepted) — wave reorder, governance merge op, cues vector-only, privacy over new fields, verified import root cause, backfill extracted, benchmark contracts, dev/holdout splits (`c882b08`). Dispositions: `docs/reviews/memora-arc/triage-round1.md`.
- **r4 (2026-07-10):** round-2 rebuild (Opus re-attack + Cursor/Grok-4.5 + Muse Spark; three-way convergence on the structural trio the r3 fixes created). Changes: **abstraction/cue generation job** added as W2 deliverable 6 with a W4-prep eval-corpus backfill step (the generation orphan made W4's gate a placebo); W3 merge apply re-specified as **journaled/idempotent/resumable** (the substrate has no N-source atomic primitive — "atomic" dropped); **API-lane hook path pre-decided unchanged** (750ms embed inside the 800ms hook deadline leaves no residue for 4-lane fusion at tail — new lanes serve local-lane hooks + the `search` pull path); W0 ingestion contract rewritten to real DTO semantics (expected-sensitivity assertion + observed governance status + auto-approve loop — `ClassificationOutcome` is not client-suppliable); W2 privacy task enumerates all write entrypoints incl. `review approve` and dream paths + sensitivity-upgrade revokes API vectors; multi-table KNN query APIs into the row-kind lifecycle; schema-migration guard mechanics; W3 fences expanded; cue merge rule capped; tracked review artifacts in `docs/reviews/memora-arc/`; decision-point numbering fixed. Dispositions: `docs/reviews/memora-arc/triage-round2.md`.

**Context:** Memora (Microsoft Research, ICML 2026, MIT) sets SOTA on LoCoMo (86.3%) and LongMemEval (87.4%) with ~half Mem0's entry count and up to 98% fewer context tokens. Clone at `~/Code/Memora`. Its mechanisms map onto Memorum gaps, including the import-duplication bug in `docs/issues.md`.

## What Memora actually does (from source, not the blog)

1. **The retrieval key is a short abstraction, not the content.** Each memory = `index` (6–8 word phrase) + `value` (rich content). Only the index phrase is embedded (`local_memory_store.py:157`, `documents=[index]`).
2. **Cue anchors are tiny linked index entries.** 1–3 phrases (2–4 words, `[Main Entity] + [Key Aspect]`) generated at write time (`cue_index_generator.py`), embedded rows with `linked_memory` back-pointers; retrieval fuses primary + cue + BM25 lanes with weighted RRF (`memory.py:231`).
3. **Merge-on-write.** Vector search over existing abstractions (top-5, cosine ≥ 0.8) → LLM update-vs-add → update rewrites value+index, keeps history (`memory_builder.py:365-579`). Why Memora stores 344 entries where Mem0 stores 651.
4. **Policy-guided retrieval.** EXPAND / RE_QUERY / STOP loop (`prompted_policy_retriever.py`); biggest wins on multi-hop.

**Deliberately NOT adopting:** ChromaDB/centralized store; natural-language phrase as primary key (their collision warts: `memory_builder.py:528`, `memory.py:607`); synchronous LLM calls in the write path (the decision moves to dreaming).

## Fit with Memorum today

- Memorum embeds **body chunks only** (`crates/memoryd/src/embedding/worker.rs`; fusion is two lanes, BM25 + chunk-vector, `recall/fusion.rs`). `summary` is ~an abstraction but is not embedded and not a retrieval key.
- The **v4.0 trigger index** (`docs/specs/stream-e-ambient-recall-v4.0.md` §5) is convergent with cue anchors but machine-verified, dream-compiled, and owns its own index-schema migration. This plan writes nothing into it.
- The **import duplication bug** (`docs/issues.md`, corrected): dedup machinery ships and is correct (`plan_action_for_record`, `execute.rs:369-380`), but the CLI path feeds it `ImportState::default()` (`cli/import.rs:30`) — every run starts amnesiac.
- The **API embedding lane** (11–17 MB live) makes abstraction-only embedding attractive: tiny inputs, low cost, possible privacy unlock (W6 spike).

---

# Execution plan (foundry, feature-scale)

## Roles

| Role | Who | Notes |
| --- | --- | --- |
| **Coordinator / judge** | **Fable (this session)** | Triage, riskiest-file reads, all gates, all commits, all spec edits, BUILD-STATE. Judgment never scripted or delegated. |
| **Author (decision-dense)** | **Sol** (`delegate codex work --model sol --reasoning-effort high`) | W2 substrate+generation, W3 merge-on-dream, W4 fusion. |
| **Author (bounded)** | **Terra** (`--model terra --reasoning-effort medium`) | W1 import fix. Coordinator owns all cargo gates. |
| **Fast reviewer / fan-out** | **Luna** (`--model luna --reasoning-effort high`) | Cheap per-wave second reviewer; mechanical sweeps. |
| **Adversarial reviewer** | **Cursor (Grok 4.5)** (`delegate cursor safe`) | Cross-family attacker on every wave diff. |
| **Fix lane** | **Devin (swe-1.7)** (`delegate devin work`) | Findings-list repairs only; prompt refuses an empty accepted-findings list. |
| **Comparative test lane** | **Muse Spark 1.1** (`delegate opencode safe --model muse`) | Max one review slot per round; journal-mandated. Field note: its round-2 plan review was code-grounded at Sol depth. |
| **Judgment support** | **Native Opus subagents** | Persistent plan-review continuity, W6 research, triage support. |

## Wave structure (r4 graph)

```
W0 (benchmarks: ingest + baseline₀) ──┐
W1 (import fix) ──────────────────────┤
W2 (substrate + generation job) ──────┼─→ W4-prep (eval-corpus abstraction backfill via W2 job)
                                      │        └─→ W4 (fusion; A/B baseline₁ → holdout)
W2 ─→ W3 (merge-on-dream; fixture-testable at W2, live-value after W5)
W5 (live-corpus backfill; separately approved; uses W2 job) — after W2, independent of W4
W6 (privacy spike memo) — independent
```

W0 ∥ W1 in parallel (integration: first-done fast-forwards `main`; second rebases before its coordinator commit and runs `cargo build --locked`; lockfile conflicts resolved coordinator-only via targeted `cargo update -p`). W1→W3 is a *data* dependency only (W3's fences exclude W1 lineage). **Live W5 is NOT a prerequisite for any W3/W4 gate.**

### W0 — Benchmark harness: LoCoMo + LongMemEval into Stream H

- **Author:** Sol high, worktree isolation.
- **Ingestion contract (r4 — real DTO semantics):** every benchmark item is written through the real daemon write surface. Sensitivity is an **expected value, not an injected one**: the harness passes the standard meta hint, records the classifier's actual outcome, and reports expected-vs-actual mismatches (they are data, not failures). Provenance/grounding: dialogue turns write as `SourceKind::User` with `explicit_user_context=true`; dataset-artifact facts write as `AgentPrimary` with a pinned `file:` ref to the dataset artifact. Governance status is **observed and asserted per item**; the eval tree runs an auto-`review approve` loop for items landing in review. Quarantine/refusal floors are evaluated **per source kind** and reported as governance drag; retrieval metrics are computed over the promoted set. (The known grounding catch-22 for self-referential ungrounded writes — CLAUDE.md open follow-up — is cross-referenced, not solved here.)
- **Scoring contract:** pinned LLM judge (model identity + prompt frozen for the arc) via `delegate codex call --read-only --output-schema`; scores **internal-only** (within-harness paired deltas; comparison to published numbers prohibited). Deterministic secondary metrics recorded alongside.
- **Splits:** pre-registered dev/holdout per dataset, recorded in BUILD-STATE before any tuning.
- **Deliverable:** **baseline₀** (pre-abstraction corpus) on both datasets + disposition counts.
- **Gate:** `cargo test -p memorum-eval`; one full end-to-end run with explained numbers.

### W1 — Import dedup: root-cause fix

- **Author:** Terra medium, worktree isolation.
- **Verified root cause:** `cli/import.rs:30` constructs `ImportState::default()` — the skip/supersede planner never sees prior records. Task 1 is diagnosis-complete-first: where state should persist/load, and why the daemon-side `governance::contradiction` layer also missed the dups (state file is documented as perf cache, not the correctness mechanism). Fix the load-bearing layer(s).
- **Identity contract:** primary anchor = recovered canonical `mem_*` id from source frontmatter when present; fallback = portable tuple (harness, stable profile identity, canonical project id, root-relative path, section key). Edge-case tests: rename-only, content+rename, repo/home move, profile-symlink same-file, identical relative paths across profiles, path reuse. Ambiguous historical matches report-only.
- **Live repair pass:** separate step, rehearsed on a copy of `~/memorum`, diff reviewed, live only after decision point 3. Drains the 31-item review-queue noise.
- **Review:** Cursor safe + Luna high. **Riskiest-file read:** the supersede path.
- **Gate:** `cargo test -p memoryd -- --test-threads=2` (import + supersession + edge-case set); double re-import on a fixture profile → stable id count.

### W2 — Abstraction + cue substrate foundation AND generation

Spec tasks (coordinator-owned, Trey ratifies before code merges):

1. **Stream A frontmatter** — `abstraction` (≤8 words) + `cues` (0–3 phrases): **version-bump decision (DP1)**. Semantics: optional fields; normalization (case, charset, length); merge rules — `abstraction` ours-wins (peer's may be better; dream repair closes the window — accepted risk), `cues` union → case-insensitive dedup → deterministic sort → **truncate to 3, ours-first**; two-clone convergence tests.
2. **Embedding row kinds + full lifecycle** — identity = row kind + memory/cue id + content hash; triple unchanged (invariant #3). Lifecycle per kind: enqueue (worker drains **all** row kinds), stale-write fence, delete, reconcile, triple switch, drop-triple, reindex, doctor/status counts, **and the query path**: `query_abstraction_vectors` / `query_cue_vectors` (same triple, shared placeholder bucketing) — no query API, no lane. Row kinds are **derived and rebuildable; they do not sync** (Stream I posture stated).
3. **Privacy composition — all write entrypoints enumerated:** write, supersede, import execute, `review approve` (currently writes `Trusted` — must compose), dream fragment→memory, dream abstraction-compile, backfill. Each classifies the **combined** body/title/summary/abstraction/cues payload; strictest outcome controls; `secret` refuses before any disk effect; generated-abstraction-turns-out-sensitive → **drop abstraction, keep body** (fail-closed). **Sensitivity upgrade revokes API-lane vectors** for that memory and re-enqueues held-local. Tests: secret-in-cue, sensitive-abstraction-on-public-body, upgrade-revocation.
4. **Index migration** — 5→6 written as additive `CREATE TABLE IF NOT EXISTS` with table-exists guards; pre-migration DB copy; rollback path; doctor cross-checks for both this plan's tables and v4-P2's absence. **Schema-number ownership is DP2, settled before any code.**
5. **CLI surface** — `memoryd write` / `write-note` accept abstraction/cues via meta; protocol DTO, generated schema, envelope tests, and `skills/using-memorum/SKILL.md` cue guidelines (adapted from Memora's `cue_index_generator.py` patterns) updated together. Validation at the trust boundary: length caps, charset, count caps, then classification per task 3.
6. **`abstraction_compile` generation job (r4 — the round-2 blocker):** a dream-pass job that mints `abstraction` + `cues` for memories lacking them — runs through the existing harness-CLI dream machinery (no daemon-resident LLM), output machine-verified (length/charset/count caps, privacy composition per task 3), written as a governed supersede. Structural fallback when no harness CLI is available: derive abstraction from `summary` (truncated), no cues. This job is the single generation mechanism used by W4-prep, W5, and ongoing dream repair — production parity by construction. Per-item cost: one harness call (subscription-flat, wall-time noted in W4-prep/W5).

Stated non-goals (decisions, not oversights): FTS/BM25 stays body-chunk-only (lexical abstraction hits deliberately not added this arc); recall-block rendering unchanged (memories surface with today's summary/snippet regardless of which lane found them); zero trigger-index writes (v4 P2 owns all of it).

- **Author:** Sol high, worktree isolation, after specs ratified.
- **Review:** Cursor safe + Luna high; Devin fixes. **Riskiest-file read:** classification composition + stale-write fence for new row kinds.
- **Gate:** `cargo test -p memory-substrate -p memoryd -- --test-threads=2`; migration up/rollback on a copied live DB; export/import round-trip; two-clone cue-merge convergence; doctor reports new row kinds; generation job produces valid fields on a fixture corpus.

### W3 — Merge-on-dream: near-duplicate consolidation (needs W2)

- **Spec task first — merge-proposal operation (r4 rewrite):** proposal carries source ids (≥1), replacement content, classification, provenance (union of source entities/evidence/refs unless overridden; classification = strictest source). Approval apply is **journaled, idempotent, and resumable — not claimed atomic** (the substrate cannot atomically write N canonical files + git + index; `api/write.rs` documents the 1→1 constraint): pre-flight re-validation of **all** source statuses at approval time (any source no longer active/pinned, tombstoned, claim-locked, or privacy-incompatible → proposal invalidated whole); defined ordering — stage replacement, supersede sources one-by-one, activate replacement last; crash recovery via the startup/dream reconcile path (the journal makes a half-applied merge detectable and resumable/rollbackable); event emission per source supersede + terminal merge event; two-clone behavior = idempotent replay of an approved proposal. **Alternative the spec must decide:** a true multi-supersede substrate primitive (one git commit + one index txn) — budgeted as Stream A work if chosen.
- **Candidate fences (r4 expanded):** status ∈ {active, pinned}; not pending review, not candidate/quarantined; not encrypted-tier; passive-recall-eligible; same scope + canonical namespace; same memory type; sensitivity-compatible (no-downgrade rule — public+confidential is NOT compatible); W1 source-lineage sets and the W5 backfill-manifest exclusion set excluded. Cosine floor ≈ 0.8 over abstraction vectors, tuned on the W0 dev split; live-corpus nearest-neighbor distance histogram reported before any live proposals.
- **Sequencing fact:** W3 is fixture-testable at W2-merge (hand-written abstractions); it produces live value only after W5 populates the live corpus. Ships dark until then — by design.
- **Author:** Sol high (xhigh on design strain), worktree isolation.
- **Review:** Cursor safe + Muse slot. **Riskiest-file read:** the approval apply (journal, re-validation, resume).
- **Gate:** `cargo test -p memory-governance -p memoryd -- --test-threads=2`; seeded-corpus run proposes right merges only; fence tests incl. privacy-incompat; **crash-injection test on the apply path** (kill between supersedes → reconcile completes or rolls back cleanly).

### W4-prep — Eval-corpus abstraction backfill (after W2, before W4 A/B)

Run the W2 `abstraction_compile` job over the benchmark corpus (dev+holdout), re-embed, record **baseline₁** (post-abstraction, pre-fusion). Volume: thousands of harness calls — subscription-flat, wall-time bounded by running it as a background goal-mode lane; disposition counts reported. This is what makes W4's A/B able to detect a win from the new lanes at all.

### W4 — Retrieval fusion: abstraction + cue lanes with RRF (needs W0 + W2 + W4-prep)

- **Author:** Sol high, worktree isolation.
- **Fusion contract:** four named primitive lanes — chunk-vector, BM25/FTS, abstraction-vector, cue-vector — one RRF (k=60), weights start 1.0/1.0/2.0/1.0; **cue hits collapse to best-rank-per-memory before fusion**; missing-lane behavior, recency interaction, deterministic tie-break specified. One query embedding reused across all vector lanes.
- **Surface split (r4 pre-decision):** the **API-lane hook path (prompt/desk passive recall) is unchanged** — today's FTS + chunk-vector under today's budgets (750ms embed inside the 800ms hook deadline leaves no tail residue for more lanes; measured, not discovered at gate time). The new lanes serve: (a) the **local-lane hook path**, under a shared-deadline formula `embed_budget = min(lane timeout, deadline − measured_fusion_reserve)`; (b) the **`memoryd search` pull path** on both lanes, under its own longer timeout. Raising the hook deadline for fusion-bearing cues stays an option inside the spec if local-lane measurement demands it.
- **Latency gate:** p50/p95/p99 for `search` (both lanes), prompt cue, desk cue (local lane); every timeout produces the documented degraded/fallback result. Work-stream cue excluded (v4 P2 owns it; unwired today).
- **Review:** Cursor safe + Luna high; Devin fixes. **Riskiest-file read:** RRF merge + lane weighting; meta-field validation and classification composition.
- **Gate — eval-gated merge:** A/B against **baseline₁** on the dev split in the worktree; freeze config; single holdout scoring must show a win with no material per-dataset regression, before fast-forwarding `main`.

### W5 — Live-corpus backfill migration (separately approved — DP4)

- Run the W2 `abstraction_compile` job over the 786 live memories via manual `memoryd dream now` passes (scheduler re-wire not required; still an open dogfood decision). ~4 embeds + 1 generation call per memory; pennies + bounded wall-time; logged.
- **Rigor:** dry-run manifest; full rehearsal on a copy of `~/memorum`; resumable/idempotent; per-disposition counts; encrypted/ineligible skipped fail-closed; export/import round-trip + reindex + doctor after; rollback = pre-run copy. The manifest's exclusion set feeds W3's candidate fence.
- **Owner:** coordinator drives; Luna assists mechanical verification.

### W6 — SPIKE: abstraction-only API-lane transit (memo only)

- **Owner:** Fable + one native Opus research subagent; Sol xhigh reads the finished memo.
- Question: can a Stream-D-classified, transit-safe abstraction embed via the API lane while the value stays local? Must address abstraction-of-a-secret leakage, independent classification, consent-ceremony scope, fail-closed behavior, and now also the W2 upgrade-revocation interaction. **No code; no fence changes without ratified spec + Trey sign-off.**

### Out of scope (recorded, not built)

Trigger-index anything (v4 P2); one-hop frontier expansion (→ v4 plan); RE_QUERY iterative retrieval in passive recall; RL-distilled policy; episodic layer; FTS-over-abstraction lanes.

## Orchestration mechanics

- **Author lanes:** plain `delegate <lane> work --isolation worktree --forbid-commit` background runs, numbered alias recorded at launch (never bare-alias wait/run-output — stale-alias misread already happened once this arc).
- **Review cycles:** parallel delegate runs (Cursor + Luna [+ Muse slot]) → coordinator triage into a **tracked** findings artifact (`docs/reviews/memora-arc/findings-w<N>-r<M>.md`, accepted/rejected/deferred with file:line) → Devin `work` run consuming exactly that artifact (prompt refuses an empty accepted list) → scoped re-review until dry under loop caps. `delegate workflow` reserved for fully-deterministic graphs (W0 scoring fan-out, W4-prep generation sweep, mechanical verification), with `--budget` caps.
- **Worktrees:** delegate-managed; `Cargo.lock` merges coordinator-only (targeted `cargo update -p`); parallel-wave integration contract as in the wave-structure section; never touch Codex-orchestrator in-flight worktrees.
- **CPU discipline (verbatim in every lane prompt):** crate-scoped `cargo check/clippy/test -p <crate>` only; never workspace-wide cargo; never `scripts/check.sh` in a worktree. Coordinator runs the one blessed full gate on integrated `main` at arc end, output to a file, `$?` echoed, never piped.
- **Commits:** coordinator-only, per-wave; **no pushes without Trey, ever.**
- **Scratch vs record:** `thoughts/memora-build/` (gitignored — done) holds prompts and BUILD-STATE; everything with history value (triage, findings, journal) lives tracked under `docs/reviews/memora-arc/` and `docs/reviews/2026-07-10-memora-arc-model-journal.md`.

## Loop discipline & budget

- **Hard stops per cycle:** 3 rounds max; same failure surviving 2 fix rounds → halt + coordinator diagnosis; zero-new-accepted-findings ends the cycle.
- **Loss function (W4; W3 threshold tuning):** target = pinned-judge accuracy on dev splits, direction UP; recall-block tokens not up >10%; no material per-dataset/question-type regression on holdout. Scorer = the W0 `memorum-eval` runner, invocation recorded in BUILD-STATE before round 1. Anti-gaming: no benchmark-conditional code; scorer frozen during tuning; fixtures never scored; cue-collapse closes the multi-cue boost; holdout scored once per config freeze; baseline₁ (not baseline₀) is the A/B control.
- **Spend:** Sol/Terra/Luna/Cursor/judge-via-Codex subscription-flat. Metered: Devin fix rounds, Muse slots (2–3), Gemini embedding (pennies). Ceiling: metered past ~$50 → stop and surface. Wall-time note: W4-prep and W5 generation sweeps are hours-scale background lanes, not token-cost risks.
- **Fable gates:** session runs on Fable — plan review and pre-ship judgment in-session. Named pre-approval requested: **pre-ship gate** (coordinator read of full integrated diff before final `scripts/check.sh` + closeout).

## Decision points for Trey (at plan approval)

1. **Stream A version bump** (`abstraction`/`cues` frontmatter — canonical serialization + merge semantics are contract-affecting) + Stream E version bump (fusion) + v4-spec cross-reference + CLI-contract field addition.
2. **Index schema 6 ownership:** recommended — this plan's W2 takes 5→6 (additive, guarded), ambient-recall v4 P2 re-points to 6→7 *before any P2 code*. Alternative: W2 waits behind P2.
3. **W1 live repair pass** on `~/memorum` (rehearsed on a copy first, diff reviewed).
4. **W5 live backfill migration** (separately approved, rehearsed first).
5. **Judge scoring route:** internal-only pinned judge via Codex subscription (recommended) — accepts non-comparability with published numbers.
6. **Muse Spark review slots** (2–3, metered test spend) — note: its round-2 comparative slot already earned its keep.

## Risks

- **Abstraction quality is load-bearing** (W4): chunk lane never removed from fusion; dream repair; eval gate on baseline₁.
- **Generation-job output quality** (W2 task 6): machine-verified constraints + privacy composition catch malformed/sensitive output; the first live dream's `malformed_pass_2_json` history says treat harness output as untrusted input — it is validated, never trusted.
- **RRF weights are Memora's** — starting points; dev-split sweeps only.
- **W3 threshold too aggressive** → merge-proposal review is the backstop; nothing auto-merges; fences hard-fail; live NN-histogram before live proposals.
- **Half-applied merge** (W3): journaled resumable apply + crash-injection test + reconcile integration — the r4 answer to the round-2 blocker.
- **Benchmark seduction:** the leaderboard gates retrieval changes; it does not steer the product.
- **Codex author-blindness:** cross-family review mandatory; named coordinator riskiest-file reads per wave.
- **Agent-supplied and dream-generated abstraction/cues are trust boundaries AND classification surfaces** (W2): validated for shape, classified for content, on every path.
- **Schema/migration blast radius** (W2, W5): guarded additive migrations, pre-migration copies, rehearsals, rollback, doctor verification.
