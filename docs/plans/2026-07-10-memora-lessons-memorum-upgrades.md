# Plan: Memora lessons → Memorum upgrades

**Status:** r3 DRAFT — foundry execution plan, review round 1 folded in. Do not execute until Trey approves.

## Plan revision history

- **r1 (2026-07-10):** design analysis + task list from a source read of `microsoft/Memora` (committed `8ca1532`).
- **r2 (2026-07-10):** foundry execution plan — waves, lanes, workflows, gates, budget (`d419b2b`).
- **r3 (2026-07-10):** round-1 review rebuild. Reviewers: Sol xhigh (cross-family) + native Opus plan-reviewer; 8 blockers, ~15 majors accepted (full dispositions: `thoughts/memora-build/triage-round1.md`). Structural changes: wave order inverted (abstraction/cue substrate now precedes merge-on-dream); a governance **merge-proposal operation** added as ratified spec work; cues are vector-only (all trigger-index work deferred to ambient-recall v4 P2); privacy classification extended over abstraction/cues on every write path; import fix re-scoped to the verified root cause (`ImportState::default()` amnesia at `crates/memoryd/src/cli/import.rs:30`); backfill extracted into its own separately-approved migration wave; benchmark ingestion/judging contracts specified; loss function moved to dev/holdout splits; review-cycle orchestration switched from a single workflow to plain runs + findings artifacts.

**Context:** Memora (Microsoft Research, ICML 2026, MIT) sets SOTA on LoCoMo (86.3%) and LongMemEval (87.4%), beating Mem0/Zep/RAG/full-context, with ~half Mem0's entry count and up to 98% fewer context tokens. Clone at `~/Code/Memora`. Its mechanisms map onto Memorum gaps, including the import-duplication bug in `docs/issues.md`.

## What Memora actually does (from source, not the blog)

1. **The retrieval key is a short abstraction, not the content.** Each memory = `index` (6–8 word phrase) + `value` (rich content). Only the index phrase is embedded (`local_memory_store.py:157`, `documents=[index]`). Values are never embedded.
2. **Cue anchors are tiny linked index entries.** 1–3 phrases (2–4 words, `[Main Entity] + [Key Aspect]`) generated at write time (`cue_index_generator.py`), stored as embedded rows with `linked_memory` back-pointers. Retrieval searches primary + cue lanes + BM25, merged with weighted RRF (`memory.py:231`).
3. **Merge-on-write.** New extraction → exact-index dup check → vector search over existing abstractions (top-5, cosine ≥ 0.8) → LLM update-vs-add decision → update rewrites value+index, regenerates cues, keeps history (`memory_builder.py:365-579`). Why Memora stores 344 entries where Mem0 stores 651.
4. **Policy-guided retrieval.** Iterative EXPAND / RE_QUERY / STOP loop (`prompted_policy_retriever.py`); biggest wins on multi-hop.

**Deliberately NOT adopting:** ChromaDB/centralized store; natural-language phrase as primary key (their own collision warts: `memory_builder.py:528`, `memory.py:607` — our stable `mem_*` ids + abstraction-as-field is strictly better); synchronous LLM calls in the write path (violates daemon architecture — the decision moves to dreaming, W3).

## Fit with Memorum today

- Memorum embeds **body chunks only** (`crates/memoryd/src/embedding/worker.rs`; chunking is body-only; fusion is exactly two lanes, BM25 + chunk-vector in `recall/fusion.rs`). `summary` frontmatter is ~an abstraction but is not embedded and not a retrieval key.
- The **v4.0 trigger index** (`docs/specs/stream-e-ambient-recall-v4.0.md` §5) is convergent with cue anchors but machine-verified and dream-compiled, and owns index schema migration 5→6. This plan does not touch it (see W2 schema decision point).
- The **import duplication bug** (`docs/issues.md`, corrected 2026-07-10): dedup machinery ships and works (`plan_action_for_record`, `execute.rs:369-380` — SkipUnchanged/Supersede on content hash), but the CLI path feeds it `ImportState::default()` (`cli/import.rs:30`), so every run starts amnesiac.
- The **API embedding lane** (11–17 MB live) makes abstraction-only embedding attractive: tiny inputs, low cost, possible privacy unlock (W6 spike).

---

# Execution plan (foundry, feature-scale)

## Roles

| Role | Who | Notes |
| --- | --- | --- |
| **Coordinator / judge** | **Fable (this session)** | Triage, riskiest-file reads, all gates, all commits, all spec edits, BUILD-STATE. Judgment never scripted or delegated. |
| **Author (decision-dense)** | **Sol** (`delegate codex work --model sol --reasoning-effort high`) | W2 substrate foundation, W3 merge-on-dream, W4 fusion. |
| **Author (bounded)** | **Terra** (`--model terra --reasoning-effort medium`) | W1 import fix. Coordinator owns all cargo gates (Terra's window can't hold heavy compiles). |
| **Fast reviewer / fan-out** | **Luna** (`--model luna --reasoning-effort high`) | Cheap per-wave second reviewer; mechanical sweeps. |
| **Adversarial reviewer** | **Cursor (Grok 4.5)** (`delegate cursor safe`) | Cross-family attacker on every wave diff. |
| **Fix lane** | **Devin (swe-1.7)** (`delegate devin work`) | Findings-list repairs only. Third family preserves author≠reviewer≠fixer. |
| **Comparative test lane** | **Muse Spark 1.1** (`delegate opencode safe --model muse`) | Max one review slot per round; journal-mandated; never load-bearing alone. |
| **Judgment support** | **Native Opus subagents** | Plan-review continuity, W6 research, triage support. |

## Wave structure (dependency order — REORDERED in r3)

```
W0 (benchmarks) ──────────────┐
W1 (import fix) ──┐           ├─→ W4 (fusion, eval-gated)
W2 (substrate) ───┼─→ W3 (merge-on-dream)
                  └─→ W5 (backfill migration, separately approved)
W6 (privacy spike memo) — independent
```

W0 ∥ W1 launch in parallel (integration contract: first-done fast-forwards `main`; the second rebases its worktree onto new `main` before its coordinator commit — never merge a stale-base branch).

### W0 — Benchmark harness: LoCoMo + LongMemEval into Stream H

- **Author:** Sol high, worktree isolation.
- **Ingestion contract (r3):** every benchmark item is written through the **real daemon write surface** with: a pinned dataset artifact as grounding provenance, an explicit fixture `ClassificationOutcome` (public/internal — the datasets are public), and an **asserted final status** per write. The runner reports promoted/quarantined/refused counts; quarantine/refusal above a small floor fails the harness run (measure retrieval, not governance rejection — but through the real path, so governance drag is *visible and quantified*, not silently bypassed).
- **Scoring contract (r3):** LLM judge pinned (model identity + prompt fixed for the arc, via `delegate codex call --read-only --output-schema`); **scores are internal-only** — within-harness paired A/B deltas are the tradable currency; comparison against Memora's published 86.3/87.4 is prohibited in reports (different judge = different scale). Deterministic secondary metrics (exact-match/F1) recorded alongside.
- **Splits (r3):** pre-registered dev/holdout split per dataset, recorded in BUILD-STATE before any tuning. Tuning sees dev only; holdout is scored once per candidate config freeze.
- **Gate:** `cargo test -p memorum-eval`; one full end-to-end run with explained numbers and disposition counts; baseline recorded in BUILD-STATE + `docs/reviews/`.

### W1 — Import dedup: root-cause fix (re-scoped in r3)

- **Author:** Terra medium, worktree isolation.
- **Verified root cause:** `crates/memoryd/src/cli/import.rs:30` constructs `ImportState::default()` — the skip/supersede planner (`import/pipeline/execute.rs:369-380`) never sees prior records, so every re-import plans `WriteNew`. **Task 1 is diagnosis-complete-first:** confirm where state should persist/load, and why the daemon-side `governance::contradiction` layer also failed to catch the dups (state file is documented as perf cache, not the correctness mechanism — `import/state.rs:3-7`). Fix the load-bearing layer(s), not a third keying scheme.
- **Identity contract (r3):** primary anchor = recovered canonical `mem_*` id from source frontmatter when present (survives rename AND edit; already recovered at `plan.rs:63,132`); fallback = portable tuple (harness, stable profile identity, canonical project id where applicable, root-relative path, section key). Enumerated edge cases with tests: rename-only, content+rename, repo/home move, profile-symlink same-file, two profiles with identical relative paths, path reuse by unrelated content. Ambiguous historical matches are **report-only**.
- **Live repair pass:** separate step, on a copy of `~/memorum` first, diff reviewed, then live **only after Trey's decision point 3**. Repairs the dream-flagged duplicate sets and drains the 31-item review-queue noise.
- **Review:** Cursor safe + Luna high. **Coordinator riskiest-file read:** the supersede path (wrong supersede silently destroys history).
- **Gate:** `cargo test -p memoryd -- --test-threads=2` (import + supersession tests, including the new edge-case set); live repro from `docs/issues.md` (double re-import → stable id count) on a fixture profile.

### W2 — Abstraction + cue substrate foundation (NEW position — was inside old W3/W4)

Everything that must exist before merge-on-dream or fusion can:

- **Spec tasks (coordinator-owned, Trey ratifies before code merges):**
  1. Stream A: `abstraction` (≤8 words) + `cues` (0–3 phrases) frontmatter — **routed as a version-bump decision, not presumed additive** (touches canonical serialization → merge driver + two-clone convergence, invariants #5/#6). Semantics specified: optional fields, normalization, validation, merge rules (proposal: `cues` set-union, `abstraction` ours-wins + dream repair), convergence tests.
  2. Embedding row kinds: identity = row kind + memory/cue id + content hash; the `(provider, model_ref, dimension)` triple unchanged (invariant #3). Full lifecycle enumerated per kind: enqueue, stale-write fence, delete, reconcile, triple switch, drop-triple, reindex, doctor/status counts.
  3. **Privacy (r3, from review blocker):** every write path — write, supersede, import, review promotion, dream backfill — classifies the **combined** body/title/summary/abstraction/cues payload; strictest outcome controls the write; `secret` refuses before any disk effect. Mandatory tests: secret-in-cue, sensitive-abstraction-on-public-body.
  4. Index migration: schema 5→6 bump with pre-migration DB copy + doctor check + rollback path. **Schema-number ownership vs ambient-recall v4 P2 is Trey's decision point 5.**
  5. CLI surface: `memoryd write` / `write-note` (NOT "remember" — that command doesn't exist) accept abstraction/cues via meta; protocol DTO, generated schema, envelope tests, and `skills/using-memorum/SKILL.md` cue-writing guidelines (adapted from Memora's `cue_index_generator.py` patterns) updated together.
- **Author:** Sol high, worktree isolation, after specs ratified.
- **Zero trigger-index writes** (r3, from review blocker): cues are vector-only in this plan. All trigger registration — including machine-verification of activation conditions per v4 §5 — stays owned by ambient-recall v4 P2.
- **Review:** Cursor safe + Luna high; Devin fixes. **Coordinator riskiest-file read:** classification composition (the strictest-outcome rule) and the stale-write fence for new row kinds.
- **Gate:** `cargo test -p memory-substrate -p memoryd -- --test-threads=2`; migration up/rollback on a copied live DB; export/import round-trip preserving new frontmatter; two-clone convergence test for cue edits; doctor reports new row kinds.

### W3 — Merge-on-dream: near-duplicate consolidation (needs W2)

- **Spec task first (r3, from review blocker):** a governance **merge-proposal operation** — proposal carries source ids (≥1), replacement content, classification, provenance; **approval is one atomic transaction**: activate replacement + supersede every source. Nothing auto-merges; rejection leaves all sources untouched. Ratified before implementation (Stream C surface change).
- **Candidate fences (r3):** active lifecycle only; same scope + canonical namespace; same memory type; privacy-compatible; W1 source-lineage sets excluded (already handled deterministically). Cosine floor ≈ 0.8 over abstraction vectors (Memora's threshold; tuned on W0 dev split only).
- **Author:** Sol high (xhigh on design strain), worktree isolation.
- **Review:** Cursor safe + Muse comparative slot. **Coordinator riskiest-file read:** the approval transaction (atomicity, provenance, strictest-classification carry).
- **Gate:** `cargo test -p memory-governance -p memoryd -- --test-threads=2`; seeded-corpus dream run proposes right merges and only right merges; cross-namespace fence tests; refusal paths exercised.

### W4 — Retrieval fusion: abstraction + cue lanes with RRF (needs W0 + W2)

- **Author:** Sol high, worktree isolation.
- **Exact fusion contract (r3):** four named primitive lanes — chunk-vector, BM25/FTS, abstraction-vector, cue-vector — one RRF formula (k=60) with per-lane weights starting 1.0/1.0/2.0/1.0; **cue hits collapse to best-rank-per-memory before fusion** (no multi-cue ranking boost — anti-gaming); missing-lane behavior, recency interaction, and deterministic tie-break specified in code and spec.
- **One query embedding reused across all vector lanes** (same triple).
- **Latency gate (r3):** p50/p95/p99 for `search`, prompt cue, desk cue, work-stream cue, on both local and API lanes; work-stream path untouched (hermetic, trigger-index only — owned by v4); every timeout produces the documented degraded/fallback result (existing `DEGRADED_EMBEDDING_DORMANT` idiom). API lane budget: query-embed 750ms within the 800ms hook-client deadline — fusion must fit the residue.
- **Review:** Cursor safe + Luna high; Devin fixes. **Coordinator riskiest-file read:** RRF merge + lane weighting; write-path validation of agent-supplied fields (length caps, charset, count caps, classification composition from W2).
- **Gate — eval-gated merge:** A/B against W0 baseline on the **dev split** in the worktree; freeze config; **single holdout scoring** must show a win with no material per-dataset regression, before fast-forwarding `main`. Loss function below.

### W5 — Live-corpus backfill migration (separately approved — Trey decision point 4)

- Backfill abstractions/cues for the 786 live memories via manual `memoryd dream now` passes (scheduler re-wire NOT required; it remains an open dogfood decision). Volume note: ~4 embeds/memory ≈ 3k API calls — pennies, logged anyway.
- **Rigor (r3):** dry-run manifest first; full rehearsal on a copy of `~/memorum`; resumable/idempotent state; per-disposition counts; encrypted/ineligible memories skipped fail-closed; export/import round-trip + reindex + doctor verification after; rollback = pre-run copy.
- **Owner:** coordinator drives; Luna assists on mechanical verification sweeps.

### W6 — SPIKE: abstraction-only API-lane transit (memo only)

- **Owner:** Fable + one native Opus research subagent; Sol xhigh reads the finished memo.
- Question: can a Stream-D-classified, transit-safe abstraction embed via the API lane while the value stays local? Must address abstraction-of-a-secret leakage, independent classification of the abstraction itself, consent-ceremony scope, fail-closed behavior. **No code; no fence changes without ratified spec + Trey sign-off.**

### Out of scope (recorded, not built)

- Trigger-index anything (v4 P2 owns it — including write-time cue term registration, which requires v4's machine-verification rules).
- T7 one-hop frontier expansion → folds into the v4 implementation plan.
- RE_QUERY iterative retrieval in passive recall; RL-distilled policy; episodic layer.

## Orchestration mechanics (revised r3)

- **Author lanes:** plain `delegate <lane> work --isolation worktree --forbid-commit` background runs. Integration contract for parallel waves: first-done integrates to `main` (coordinator commit), second rebases before its coordinator commit.
- **Review cycles:** plain **parallel delegate runs** (Cursor + Luna [+ Muse slot]), coordinator triage into a findings artifact (`thoughts/memora-build/findings-w<N>-r<M>.md`), then a plain Devin `work` run consuming exactly that artifact. (r3: replaced the single-workflow design — a delegate workflow cannot host unscripted coordinator judgment mid-graph.) `delegate workflow` is reserved for fully-deterministic graphs: the W0 benchmark scoring fan-out and mechanical verification sweeps, run with `--budget` caps.
- **Alias discipline:** never bare-alias `wait`/`run-output` — record the numbered alias at launch (stale-alias misread already happened once this arc, during plan review).
- **Worktrees:** delegate-managed (accept its paths/branches — don't conflate with the Codex-orchestrator `../agent-memory-wt` convention); `Cargo.lock` merges coordinator-only; never touch Codex-orchestrator in-flight worktrees.
- **CPU discipline (verbatim in every lane prompt):** crate-scoped `cargo check/clippy/test -p <crate>` only; never workspace-wide cargo; never `scripts/check.sh` in a worktree. Coordinator runs the one blessed full gate on integrated `main` at arc end, output redirected to a file with `$?` echoed — never piped.
- **Commits:** coordinator-only, per-wave; **no pushes without Trey, ever.**
- **Scratch:** `thoughts/` added to `.gitignore` (r3 — it was untracked-but-not-ignored, which both dirties safe-lane snapshots and blocks worktree launches). BUILD-STATE.md + prompt files + findings artifacts live there; lane prompts receive scoped artifacts, not dirty-tree copies.
- **Model journal:** every delegate run journaled in `docs/reviews/2026-07-10-memora-arc-model-journal.md` (Muse entries mandatory-detailed).

## Loop discipline & budget

- **Hard stops per cycle:** 3 rounds max; same failure surviving 2 fix rounds → halt + coordinator diagnosis; zero-new-accepted-findings ends the cycle.
- **Loss function (W4, and W3 threshold tuning):** target = pinned-judge accuracy on the **dev splits**, direction UP, recall-block tokens not up >10%, no material regression per dataset or major question type on holdout. Scorer = the W0 `memorum-eval` runner, exact invocation recorded in BUILD-STATE before round 1. Anti-gaming: no benchmark-conditional code; scorer code frozen during tuning; fixture sets never scored; cue-collapse rule (W4) closes the multi-cue boost; holdout scored once per config freeze.
- **Spend:** Sol/Terra/Luna/Cursor subscription-flat. Metered: Devin fix rounds (small), Muse slots (2–3 total), Gemini embedding calls (pennies), pinned-judge calls via Codex subscription (flat). Ceiling: metered spend trending past ~$50 → stop and surface.
- **Fable gates:** session runs on Fable — plan review and pre-ship judgment are in-session. Named pre-approval requested: **pre-ship gate** = coordinator read of the full integrated diff before the final `scripts/check.sh` + closeout.

## Decision points for Trey (at plan approval)

1. **Stream A version bump** for `abstraction`/`cues` frontmatter (r3: upgraded from "additive amendment" after review — canonical serialization + merge semantics are contract-affecting). Plus Stream E version bump for fusion (unchanged from r2), v4-spec cross-reference note, CLI-contract field addition.
2. **Index schema 6 ownership:** this plan's W2 takes 5→6 and ambient-recall v4 P2 re-points to 6→7, or W2 waits behind P2. (They collide as specced today.)
3. **W1 live repair pass** on `~/memorum` (rehearsed on a copy first, diff reviewed).
4. **W5 backfill migration** on the live corpus (separately approved, rehearsed first).
5. **Judge scoring route:** internal-only pinned judge via Codex subscription (recommended, r3 default) — accepts non-comparability with published numbers.
6. Muse Spark review slots (2–3, metered test spend).

## Risks

- **Abstraction quality is load-bearing** (W4): vague abstraction = lost memory. Mitigations: chunk lane never removed from fusion; dream repair; eval gate.
- **RRF weights are Memora's, tuned on their benchmarks** — starting points only; dev-split sweeps.
- **W3 threshold too aggressive** merges distinct memories → merge-proposal review is the backstop; nothing auto-merges; namespace fences hard-fail.
- **Benchmark seduction:** the leaderboard gates retrieval changes; it does not steer the product. Governance/privacy differentiators score zero on LoCoMo — by design.
- **Codex author-blindness** (all Sol waves): cross-family review mandatory; named coordinator riskiest-file reads per wave.
- **Agent-supplied abstraction/cues are a trust boundary AND a classification surface** (W2): validated for shape and classified for content on every path; a cue is content and classifies like content.
- **Schema/migration blast radius** (W2, W5): pre-migration copies, rehearsals, rollback paths, doctor verification — new in r3, non-negotiable.
