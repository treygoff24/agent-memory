# Plan: Memora lessons → Memorum upgrades

**Status:** r2 DRAFT — foundry execution plan, in review. Do not execute until Trey approves.

## Plan revision history

- **r1 (2026-07-10):** design analysis + task list from a source read of `microsoft/Memora` (committed `8ca1532`).
- **r2 (2026-07-10):** expanded into a foundry execution plan: wave structure, lane routing (Fable coordinator + sol/terra/luna/cursor/devin/muse/native-opus roster), delegate-workflows orchestration, gates, budget, loss function, spec-bump decision points.

**Context:** Memora (Microsoft Research, ICML 2026, MIT) sets SOTA on LoCoMo (86.3%) and LongMemEval (87.4%), beating Mem0/Zep/RAG/full-context, with ~half Mem0's entry count and up to 98% fewer context tokens. Its three mechanisms map cleanly onto Memorum gaps — including the import-duplication bug logged in `docs/issues.md` the same day this plan was written. Clone at `~/Code/Memora`.

## What Memora actually does (from source, not the blog)

1. **The retrieval key is a short abstraction, not the content.** Each memory = `index` (6–8 word phrase, e.g. "Updated Project Orion timeline agreed by Dave and Sarah") + `value` (rich content). Only the index phrase is embedded: `local_memory_store.py:157` upserts `documents=[index]`. Values are never embedded.
2. **Cue anchors are tiny linked index entries.** 1–3 phrases (2–4 words, `[Main Entity] + [Key Aspect]`) generated per memory at write time (`cue_index_generator.py`), stored as their own embedded rows with `linked_memory` pointing at primary entries. Retrieval searches primary + cue lanes + BM25, merges with weighted RRF (primary 2.0, cue 1.0, hybrid 1.0; `memory.py:231`).
3. **Merge-on-write.** New extraction → exact-index dup check → vector search over existing *abstractions* (top-5, cosine ≥ 0.8 `update_score_threshold`) → LLM decides update-vs-add → update rewrites value+index, regenerates cues, keeps history (`memory_builder.py:365-579`). This is why Memora stores 344 entries where Mem0 stores 651.
4. **Policy-guided retrieval.** Iterative loop with three actions — EXPAND (pull frontier items reachable via shared cues/links), RE_QUERY (reformulate; handles "relative answer" pointers like "same college as Sarah"), STOP — driven by a prompted LLM or an RL-distilled small model (`prompted_policy_retriever.py`). Biggest wins on multi-hop.

**What we are deliberately NOT adopting:** ChromaDB/centralized store (our substrate is canonical files + git); the natural-language phrase as the *primary key* (their own code grows warts from it — index-collision rename hacks at `memory_builder.py:528`, episodic "(2)" suffixing at `memory.py:607`; our stable `mem_*` ids + abstraction-as-field is strictly better); synchronous LLM calls inside the write path (violates Memorum's daemon architecture — see W3 for where that decision moves).

## Fit with Memorum today

- Memorum embeds **body chunks** (`crates/memoryd/src/embedding/worker.rs`, per-chunk `body_hash`) — exactly the "content-fragmentation" pole Memora's paper argues against. Our `summary` frontmatter is already ~an abstraction; it just isn't the retrieval key.
- The **v4.0 trigger index** (`docs/specs/stream-e-ambient-recall-v4.0.md` §5) is convergent with cue anchors but dream-compiled and deterministic-match-only. Memora's lesson: also mint cues *at write time* from the memory value, and give them a vector lane, not just exact activation-condition matching.
- The **import duplication bug** (`docs/issues.md`, found by the 7/10 dream run) is the exact failure mode merge-on-write exists to prevent.
- The **API embedding lane** (11–17 MB live) makes abstraction-only embedding extra attractive: tiny inputs, lower cost, and a possible privacy unlock (W5 spike).

---

# Execution plan (foundry, feature-scale)

## Roles

| Role | Who | Notes |
| --- | --- | --- |
| **Coordinator / judge** | **Fable (this session)** | Owns triage, riskiest-file reads, all gates, all commits, spec edits, BUILD-STATE.md. Never delegates judgment. |
| **Author (decision-dense waves)** | **Sol** (`delegate codex work --model sol --reasoning-effort high`) | W2 dream consolidation, W4 abstraction/cue retrieval. Trust-critical Rust. |
| **Author (bounded waves)** | **Terra** (`--model terra --reasoning-effort medium`) | W1 import dedup. Coordinator owns all cargo gates — Terra's run window cannot hold a heavy compile (2026-07-09 journal). |
| **Fast reviewer / fan-out** | **Luna** (`--model luna --reasoning-effort high`) | Cheap per-wave second reviewer (matched Opus on depth, 7/09 journal); exploration and mechanical sweeps. |
| **Adversarial reviewer** | **Cursor (Grok 4.5)** (`delegate cursor safe`) | Cross-family attacker on every wave diff. Probes artifacts, computes evidence. Coordinator ranks severity. |
| **Fix lane** | **Devin (swe-1.7)** (`delegate devin work`) | Findings-list repairs only — never ambiguity. Third family preserves author≠reviewer≠fixer. |
| **Comparative test lane** | **Muse Spark 1.1** (`delegate opencode safe --model muse`) | One review slot per review round max, journal-mandated. Unranked; never load-bearing alone. |
| **Judgment support** | **Native Opus subagents** | plan-reviewer passes, W5 privacy-memo research, triage support on big finding sets. |

Decorrelation per wave: Codex-family authors → Grok-family reviews (+ Luna same-family cheap pass + Muse comparative) → Devin fixes → Fable decides. Cursor and any grok-harness lane share one decorrelation slot.

## Wave structure

Dependency order. W1 and W0 are independent and launch in parallel; W2 needs W1's repaired corpus semantics; W4 is eval-gated on W0; W5 informs W4's API-lane posture but doesn't block its local-lane form.

### W0 — Benchmark harness: LoCoMo + LongMemEval into Stream H (T6)

- **Author:** Sol high, `--isolation worktree`. Port dataset loaders + runners (reference: `~/Code/Memora/app/locomo/`, `app/longmemeval/`, MIT) into the Stream H eval harness; produce a `memorum-bench` runner that drives the real daemon surface (import → recall → LLM-judge scoring), plus a pinned-dataset fetch script (datasets are public; do not vendor if license forbids).
- **Deliverable metric:** baseline Memorum scores on both benchmarks, recorded in BUILD-STATE.md and `docs/reviews/`.
- **Review:** Cursor safe + Luna high on the diff; Devin fixes.
- **Gate:** `cargo test -p <eval crate>`; one full benchmark run end-to-end with numbers that are *explained* (a 0% or 100% is a harness bug until proven otherwise).
- **Caveat (from r1 risks):** benchmarks measure conversational QA recall, not governance/privacy value — retrieval-quality gate only.

### W1 — Import dedup by source identity (T1, the bug fix)

- **Author:** Terra medium, `--isolation worktree`. Key file-sourced imported memories by stable source identity (source file path + profile/repo), not per-import batch. Unchanged file re-import = no-op; changed file = supersede via existing Stream A supersession machinery. Includes a `memoryd doctor`-adjacent repair pass for the live duplicates (the dream-flagged sets in `docs/issues.md`).
- **Review:** Cursor safe; Luna high. **Coordinator riskiest-file read:** the supersession path — a wrong supersede silently destroys history.
- **Gate:** `cargo test -p <import crate> -- --test-threads=2`; live repro from `docs/issues.md` (double re-import → stable id count); repair pass on a *copy* of `~/memorum` first, live only after diff review.
- **Explicitly NOT:** any LLM/semantic dedup — that's W2. This wave is deterministic only.

### W2 — Merge-on-dream: near-duplicate consolidation (T2)

- **Author:** Sol high (xhigh if the first attempt shows design strain), `--isolation worktree`.
- Daemon-side: deterministic near-duplicate candidate detection (cosine over summary/abstraction embeddings, floor ≈ 0.8 — Memora's `update_score_threshold`, tunable via W0 eval). Dream-side: a consolidation pass consumes candidate sets and proposes supersede/merge ops through **existing Stream C governance** (quarantine → review → approve). Nothing auto-merges.
- **Review:** Cursor safe + Muse comparative slot. **Coordinator riskiest-file read:** merge-proposal construction — a merged memory must preserve provenance and the stricter of the two privacy classifications (Stream D invariant; a merge is a *write* and carries a `ClassificationOutcome`).
- **Gate:** `cargo test -p <governance/dream crates>`; seeded-corpus dream run proposes the right merges and *only* the right merges; governance refusal paths exercised.

### W3 — Spec amendments (coordinator-owned, no delegation)

Fable writes, Trey ratifies. Required before W4 code lands (W4 may build in a worktree against the draft):

1. Stream A frontmatter: `abstraction` (≤8 words) + `cues` (0–3 phrases) — additive amendment candidate.
2. Stream E retrieval fusion (RRF lanes + weights): **behavior change → version bump — requires Trey's explicit approval per repo convention.**
3. v4.0 ambient-recall spec: write-time cues feeding the trigger index — additive amendment.
4. CLI contract v1: `memoryd remember` accepts abstraction/cues; skill (`skills/using-memorum/SKILL.md`) gains Memora-style cue guidelines (adapt `cue_index_generator.py` prompt patterns).
5. Embedding storage: abstraction/cue vectors are **distinct row kinds**, never mixed into chunk tables — invariant #3 (triple identity) untouched; no re-interpretation of existing vectors.

### W4 — Abstraction + cue retrieval lanes with RRF fusion (T3 + T4)

- **Author:** Sol high, `--isolation worktree`. Additional vector lanes (abstraction, cue) alongside — not replacing — chunk/FTS lanes; weighted RRF fusion (start 2.0/1.0/1.0); write path accepts agent-supplied abstraction/cues (no daemon LLM calls — the writing agent is the LLM); dream backfill pass for weak/missing abstractions on the existing corpus.
- **Hermetic invariant preserved:** the PostToolUse work-stream path stays no-network/no-subprocess — cues register as trigger-index *terms* there; vector lanes serve prompt/desk cues only (source-guard tests per v4 idiom).
- **Review:** Cursor safe + Luna high; Devin fixes. **Coordinator riskiest-file read:** RRF merge + lane weighting (silent ranking corruption), and the write-path validation of agent-supplied fields (trust boundary — length caps, no markup, classification carried).
- **Gate — eval-gated merge (loss function below):** A/B on W0 benchmarks in the worktree BEFORE fast-forwarding main (eval-gated merge order, project memory). Ship only on a measured win.

### W5 — SPIKE: abstraction-only API-lane transit (T5, memo only)

- **Owner:** Fable + one native Opus research subagent; **cross-family read:** Sol xhigh on the finished memo (judgment-dense synthesis is its validated strength).
- Deliverable: 1–2 page memo — can a Stream-D-classified, transit-safe abstraction be embedded via the API lane while the value stays local? Must address: abstraction-of-a-secret leakage, independent classification of the abstraction itself, whether a separate consent ceremony is needed, and fail-closed behavior. **No code. No fence changes without a ratified spec amendment + Trey sign-off.**

### Out of scope (recorded, not built)

- **T7 one-hop frontier expansion** → folds into the ambient-recall v4.0 implementation plan.
- RE_QUERY-style iterative retrieval in passive recall (latency budget); RL-distilled retrieval policy; episodic-memory layer.

## Orchestration mechanics

- **Substrate:** author lanes run as plain `delegate <lane> work --isolation worktree` background runs (one-shot bounded tasks — workflow overhead unjustified). **Per-wave review+fix cycles run as delegate workflows**: `parallel([cursor safe, luna safe(, muse safe)])` findings fan-out → coordinator triage (never scripted) → `devin work` fix stage → scoped re-review, under `--budget` caps. Known bug guard (delegate-agent#12): review lanes return **no-schema text**, never trust schema envelopes; verify work lanes by file outputs.
- **Worktrees:** repo convention (`../agent-memory-wt/` idiom) via `--isolation worktree --include-dirty` as needed; `Cargo.lock` merges are coordinator-only; never touch Codex-orchestrator in-flight worktrees.
- **CPU discipline (verbatim into every lane prompt):** inner loop is `cargo check/clippy/test -p <crate>` only; never bare workspace-wide cargo commands; never `scripts/check.sh` in a worktree. Coordinator runs the one blessed full gate on integrated `main` at the end, redirected to a file with `$?` echoed — never piped.
- **Commits:** coordinator-only, per-wave, ungated locally; **no pushes without Trey, ever**.
- **BUILD-STATE.md** in a gitignored scratch dir (`thoughts/` exists and is untracked — use `thoughts/memora-build/`): task ledger + append-only lessons; every lane prompt cites it.
- **Model journal:** every delegate run journaled in `docs/reviews/2026-07-10-memora-arc-model-journal.md` per the delegate-agent mandate (Muse entries mandatory-detailed).

## Loop discipline & budget

- **Hard stops per cycle:** 3 rounds max; same failure surviving 2 fix rounds → halt + diagnose (stuck-twice = Fable diagnosis pass — free, I'm the session); zero-new-accepted-findings ends the cycle.
- **Loss function (W4 A/B and any RRF/threshold tuning):** target = LLM-judge accuracy on LoCoMo + LongMemEval, direction UP, with recall-block token count NOT increasing >10%. Scorer = the W0 `memorum-bench` command, exact invocation recorded in BUILD-STATE.md before round 1. Eval set = full benchmark suites (large; not the seeded fixture sets — those are for development only). Anti-gaming: no benchmark-conditional code paths; no touching scorer code in tuning rounds; fixture sets never scored.
- **Spend:** Sol/Terra/Luna + Cursor are subscription lanes (flat). Metered: Devin (per-fix-round, small), Muse (test slots only), Gemini API embedding calls during benchmarks (pennies at 768-dim). Ceiling: if metered spend trends past ~$50 without Trey, stop and surface.
- **Fable gates:** session runs on Fable, so plan-review and pre-ship judgment are in-session (no extra metered spawn). The one named pre-approval requested at plan approval: **pre-ship gate** = Fable coordinator read of the full integrated diff before the final `scripts/check.sh` + closeout.

## Decision points for Trey (at plan approval, not mid-build)

1. **Stream E version bump** authorization (retrieval fusion behavior change) + Stream A/v4/CLI-contract amendments (additive).
2. **W1 live repair pass** touches `~/memorum` (backed by git history + a pre-run copy) — confirm.
3. **Benchmark LLM-judge spend**: LoCoMo/LongMemEval scoring uses an LLM judge — route through Codex subscription (`delegate codex call --read-only`) rather than metered APIs. Confirm.
4. Muse Spark review slots (2–3 total across the build) — metered test spend. Confirm.

## Risks

- **Abstraction quality is now load-bearing** (W4): a vague abstraction is a lost memory. Mitigations: chunk lane never removed from fusion, dream repair pass, eval gate.
- **RRF weights are Memora's, tuned on their benchmarks** — 2.0/1.0/1.0 is a starting point; W0-gated sweeps only (deterministic eval makes knob sweeps cheap).
- **W2 threshold too aggressive** merges distinct memories → governance review is the backstop; nothing merges un-reviewed.
- **Benchmark seduction** (W0): LoCoMo optimizes conversational QA; Memorum's differentiators (privacy, governance, git durability, multi-device) score zero there. The leaderboard gates retrieval changes; it does not steer the product.
- **Codex author-blindness** (both Sol waves): its green tests have hidden ship-blockers before — cross-family review is mandatory, never skipped, plus coordinator riskiest-file reads named per wave above.
- **Agent-supplied abstraction/cues are a trust boundary** (W4): validate length/charset, cap counts, carry classification — a cue is content and classifies like content.
