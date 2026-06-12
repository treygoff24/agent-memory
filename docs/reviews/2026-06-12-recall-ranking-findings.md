# Recall-ranking findings & proposed way forward

**Date:** 2026-06-12
**Author:** Claude (Opus) — code review + design brief
**Status:** For Trey's review. **No code changed for this cluster.** This is the deferred half of the xhigh `/code-review` of the 6/11 overnight session.
**Scope:** The eight recall-ranking findings (#1–8 in the review) that were held back because they move the recall-quality eval numbers. The six behavior-scoped findings (#9–13, #15) are already fixed, gated green, and shipped in `d7b309f`.

**Decisions already taken (Trey, 2026-06-12):**

- **Recency cluster → Approach A** (plumb the real `updated_at` through to fusion, not revert).
- **Sequencing → agreed:** safe wins first (#7 → #6 → #3), then the eval-sensitive ones (#1, then the recency cluster).
- **Execution → not yet.** Read this first.

---

## TL;DR

The 6/11 session shipped two recall changes — a recency tie-break in `fusion.rs` (`9e60829`) and an FTS relaxed-fallback in `query.rs` (`df65f56`) — plus three Qwen query-prompt tunings. Across them, eight ranking defects are confirmed. They fall into three independent clusters:

1. **Recency tie-break (#2, #4, #5, #8)** — as shipped it is plausibly **net-negative**: it does nothing on the dominant first-run state (imported corpora) and actively distorts ranking on aged data. The root cause of the no-op (#5) is that it keys on the memory-id date prefix (insertion time) instead of the real `updated_at`, which *exists in the index but isn't plumbed to fusion*. Approach A fixes this properly.
2. **FTS relaxed fallback (#1, #3, #6)** — the fallback's intent is sound (fill under-filled recall with OR-matches) but it scores those matches as if they were first-class (#1), can still under-fill (#3), and discards exactly the short identifier anchors the prompt tuning simultaneously told the model to prize (#6).
3. **Embedding prompt budget (#7)** — the tuned ~290-char instruction prefix eats into a fixed 512-token, tail-truncated embedding window, silently truncating long queries and (worse) write-path contradiction-detection bodies.

**The cross-cutting constraint:** the 6/11 work was a recall-quality *autoresearch* loop, so some of these defects may be **load-bearing for the current eval scores** (#1 most of all). Every fix lands one at a time with a before/after eval run; the harness is the arbiter, not intuition.

---

## Cross-cutting principle: eval discipline

Before any of this is touched:

- **One fix per eval cycle.** Batching makes a score delta unattributable. Each change gets an isolated before/after run on the recall-quality corpus (`scripts/stream-e-recall-bench.sh` / `memorum-eval`, command TBD by Trey).
- **A regression is information, not a veto.** If fixing #1 *lowers* recall@k, that tells us the buggy behavior was propping up the metric — which is itself a finding about the eval set, not a reason to keep a bug.
- **Separate "pure-win" fixes from "behavior-change" fixes.** #3, #6, #7 should only ever *find more true matches* or *embed more real content* — low regression risk, eval-confirm and move on. #1 and the recency cluster genuinely change ranking and need real A/B scrutiny.

All line numbers below are against `main @ d7b309f` (these files were untouched by the 6/11 fixes that shipped today).

---

## Cluster C: Embedding prompt budget — **#7** *(do first; cheapest, likely a quiet win)*

**Finding.** `embedding/prompts.rs:17` defines a ~290-char `DEFAULT_QUERY_TASK`, formatted as `Instruct: {task}\nQuery: {query}` (line 21) — a fixed prefix of ~300 chars (~70–90 tokens). `embedding/fastembed_provider.rs:40` caps tokenization at `MAX_SEQUENCE_LENGTH = 512`, **tail-truncated**. The comment there notes the 512 was sized for *chunks* (50–500 tokens); the query/contradiction path reuses the same cap. The write-path contradiction detector (`handlers/governance/policy.rs:349`) embeds the **full candidate memory body** through `embed_query`, so it inherits the same prefix.

**Why it matters.** A 500-token body + ~80-token instruction prefix = ~580 > 512, so ~70 tokens of the body's tail are dropped *before* similarity matching. Two memories that contradict only in their later sentences can embed to near-identical truncated vectors → missed contradiction. The three 6/11 prompt-tuning commits grew the prefix, moving the truncation boundary earlier.

**Status:** CONFIRMED mechanism; exact token delta not yet measured.

**Fix options:**

| Option | Pro | Con |
|---|---|---|
| **C-1. Measure first, then decide** | Cheap; may show the delta is small enough to ignore | None — this is a prerequisite, not an alternative |
| **C-2. Raise `MAX_SEQUENCE_LENGTH` for the query/body path** (Qwen3-Embedding-0.6B supports well beyond 512) | Keeps the tuned prefix intact; fits long bodies | Longer sequences = slower embedding + more memory; must confirm model max + measure perf hit; the `(provider, model_ref, dimension)` identity invariant must stay intact (seq-len isn't part of the triple, so this is safe, but verify) |
| **C-3. Trim the instruction prefix** | Reclaims budget with zero infra change | The prefix was *deliberately* tuned over 3 commits for recall quality; trimming may give back those gains |
| **C-4. Don't prepend the instruction to long bodies** (contradiction path), only to queries | Targeted; bodies aren't "queries" anyway | Two embedding regimes to reason about; the instruction may genuinely help body-vs-body matching |

**Recommendation:** C-1 → if the delta is real, **C-2** (raise the cap for this path) is the cleanest because it preserves the tuned prompt and fixes both the query and contradiction paths at once. Fall back to C-3 only if the perf cost of a longer window is unacceptable. **Eval risk: low** (this strictly *adds* real content to the vector).

---

## Cluster B: FTS relaxed fallback — **#6, #3, #1**

Context: when the strict AND-query under-fills (`collapsed.len() < limit`), `query.rs` runs a second OR-query ("relaxed" pass) to top up results. Three defects in that pass, in the agreed order.

### #6 — relaxed pass discards short identifier anchors *(do second; near pure-win)*

**Finding.** `relaxed_fts_token` (`query.rs:2293`) drops any token whose alphanumeric count is `< 4`. That kills `PR`, `v2`, `B-7`, 2–3 char project codes, version tags. This **directly contradicts** `prompts.rs:17` (tuned the same session), which now tells the embedding model to treat "identifiers, dates … as anchors." So the vector lane is told to prize short identifiers while the BM25 fallback lane discards them.

**Scope note:** only the *relaxed fallback* lane drops them; the strict pass (via `sanitize_fts_query`) keeps short tokens. The contradiction bites specifically when the strict pass already under-filled — i.e. exactly the queries that most need the fallback.

**Status:** CONFIRMED.

**Fix options:**

| Option | Pro | Con |
|---|---|---|
| **B6-1. Identifier-aware retention** — keep short tokens that look like identifiers (contain a digit, are all-caps, or are mixed alphanumeric); drop only short all-lowercase-alpha words | Restores anchor recall without re-admitting low-signal filler | A little classifier logic to get right |
| **B6-2. Drop the length filter entirely; rely on the stopword list + strict lane** | Simplest; max recall | Re-admits short common words the stopword list doesn't cover → more OR noise |

**Recommendation:** **B6-1.** **Eval risk: low-moderate** (restores recall on identifier queries; small chance of added OR noise — A/B confirms).

### #3 — relaxed pass under-fills distinct memories *(do third; near pure-win)*

**Finding.** The FTS table is **chunk-grained**. The relaxed pass caps rows with a chunk-level SQL `LIMIT` (`relaxed_row_limit = limit.saturating_mul(8).min(256).max(limit)`, `query.rs:493`), then `collapse_bm25_memory_hits` collapses to distinct memories **in Rust, after** the LIMIT. So a few long memories whose many chunks fill the 256-row cap collapse to far fewer than `limit` distinct memories, leaving recall slots unfilled even when more matching memories exist past the cap.

**Status:** CONFIRMED.

**Fix options:**

| Option | Pro | Con |
|---|---|---|
| **B3-1. SQL-side collapse + memory-level LIMIT** (`GROUP BY memory_id`, `MIN(bm25)`/representative, then `LIMIT` on memories) | Correct and bounded; the cap means what it says | More SQL surgery; must pick the per-memory representative chunk deterministically |
| **B3-2. Bump the chunk cap / paginate until N distinct memories collapse** | Minimal change | Unbounded-ish; needs a sane hard ceiling; still probabilistic |

**Recommendation:** **B3-1** is the principled fix and aligns with how strict collapse already works; B3-2 is a stopgap if B3-1 is too invasive this pass. **Eval risk: low** (strictly fills more true matches).

### #1 — relaxed hits get top-tier RRF scores on an incomparable basis *(do fourth; needs real A/B)*

**Finding.** Relaxed (OR) hits are appended to the strict (AND) `collapsed` list (`~query.rs:502`), then the *whole* list is ranked by a single `enumerate()` → `rank = idx+1` (`~510`). That rank feeds `reciprocal_rank_score(k, rank) = 1/(k+rank)` (`fusion.rs:61`). With default `rrf_k = 60`, a relaxed-only hit at rank 4 scores `1/64 ≈ 0.0156` — **~95%** of a strict rank-1 hit's `1/61 ≈ 0.0164` — even though its BM25 was computed against a *different* (OR) query expression and is not comparable to the strict hits' scores. Weak OR-matches get near-top fused contributions.

**Status:** CONFIRMED. **This is the most likely of the eight to be load-bearing for the current eval scores** — boosting OR-matches into the top-k may be inflating recall@k on the eval set.

**Fix options:**

| Option | Pro | Con |
|---|---|---|
| **B1-1. Discount the relaxed lane's RRF contribution** (penalty factor, or a larger `k` for relaxed hits, or a score ceiling below the worst strict hit) | Relaxed becomes a tie-breaker-of-last-resort, not a top contender; tunable | One more knob to tune against the eval |
| **B1-2. Relaxed hits fill tail slots only, with a flat floor score** — never reorder relative to strict | Clean separation; removes incomparability entirely | Loses ranking signal *among* relaxed hits |
| **B1-3. Leave as-is if the eval proves it's a net win** | Zero risk of regressing the metric | Keeps a real correctness wart; brittle to corpus changes |

**Recommendation:** Try **B1-1** (discount), measure; if the eval *drops*, that's the signal that the current behavior is propping up recall@k and we discuss whether the eval set or the ranking is wrong. **Eval risk: high — A/B mandatory, do not batch.**

---

## Cluster A: Recency tie-break — **#5, #2, #4, #8** *(do last; Approach A)*

All four are facets of `sort_by_rrf_with_recency_ties` (`fusion.rs:57`, introduced by `9e60829`). Trey has chosen **Approach A: make it work properly** rather than revert. The grounding fact that makes A viable: the `memories` index table carries real timestamps — `created_at`, `updated_at`, `observed_at`, `indexed_at` (`index/schema.rs:30–32,44`), with `updated_at DESC` indexes — but the recall candidate structs (`HybridMemoryCandidate` → `FusedHybridCandidate`, `fusion.rs:9`) carry **no timestamp**, only `memory_id / text / score_breakdown / rrf_score`. That absence is *why* the tie-break scraped the date out of the `mem_YYYYMMDD` id prefix.

### The four defects

- **#5 (the no-op) — `fusion.rs:91` / `ids/sequence.rs:56,108`.** `memory_id_date` parses `mem_YYYYMMDD`; that prefix is the **insertion** date (`Utc::now()` at allocation). Imported corpora all get the import-day date → uniform recency key → the tie-break degrades to the lexicographic id tie-break. So on every first-run import (exactly what the new init wizard provisions) the feature does nothing. **CONFIRMED.**
- **#2 (relevance inversion) — `fusion.rs:82`.** Within a tie window the comparator sorts by date **first** (`memory_id_date(right).cmp(&memory_id_date(left))`) and `rrf_score` only as `.then_with(...)`. A strictly-higher-RRF dual-lane match is demoted below any fresher weaker one. **CONFIRMED.**
- **#4 (non-transitive banding) — `fusion.rs:75,77`.** `group_score = candidates[group_start].rrf_score` anchors each band to its *first/highest* member; the extend test `(group_score - candidates[group_end].rrf_score).abs() <= epsilon` always measures from the anchor, never the running neighbor. Whether two near-equal results get recency-reordered depends on absolute position, not pairwise closeness. **CONFIRMED.**
- **#8 (ε too wide at k=60) — `config.rs:8`.** `DEFAULT_VECTOR_RECALL_RECENCY_TIE_EPSILON = 0.00025`. At `rrf_k=60`, single-lane adjacent deltas `1/(60+n) − 1/(61+n)` fall below ε past ~rank 4–5, so deep in the list the "near-tie" window swallows several genuine rank positions and recency overrides real separation. **CONFIRMED (single-lane; fused two-lane scores vary).**
- **(C6, related) — `fusion.rs:5,57`.** ε is a hardcoded module const reached by direct import, while its sibling knob `rrf_k` *is* a `VectorRecallConfig` field (`config.rs:24`) threaded through `context.config` (`hybrid.rs:95`). The two co-located sort parameters are sourced inconsistently.

### What Approach A entails

1. **Plumb a real timestamp** (`updated_at`, the natural "freshness"; consider `observed_at` for imported memories) from the index query into `HybridMemoryCandidate` and `FusedHybridCandidate`. This is the load-bearing change; everything else depends on it.
2. **Key the tie-break on the real timestamp**, retiring `memory_id_date` from the ranking path.
3. **Decide the tie-break *semantics*** (the real design choice — see below).
4. **Fix banding (#4)** to be transitive / well-defined.
5. **Re-derive ε (#8)** against the real fused-score distribution; strongly consider a *relative* ε (fraction of top score, or rank-based) instead of an absolute one.
6. **Move ε into `VectorRecallConfig` (C6)** so it's tunable alongside `rrf_k`.

### The core design decision inside A — tie-break semantics

| Option | Behavior | Pro | Con |
|---|---|---|---|
| **A-1. Strict lexicographic `(rrf desc, recency desc)`** | recency only breaks *exact* rrf ties | Predictable; no banding artifacts; #2/#4/#8 vanish by construction | "tie window" becomes pointless — exact ties are rare, so recency almost never fires |
| **A-2. Banded (current intent, fixed)** | within a principled ε, sort by recency | Keeps the "near-ties prefer fresh" intent | By design recency overrides sub-ε score gaps; ε tuning is delicate; banding edge-cases must be handled carefully |
| **A-3. Continuous recency prior** — blend a small normalized recency term into the score: `final = rrf_score + λ · recency_norm` | recency *nudges*, never *dominates*; one smooth knob | **Eliminates #2, #4, and #8 entirely** (no banding, no discontinuity, recency can't invert a meaningful gap); most eval-friendly (tune one λ) | Changes the score model; λ must be tuned; "recency" needs a sensible normalization (e.g. exponential decay over age) |

**Recommendation:** **A-3 (continuous prior).** It dissolves three of the four defects structurally rather than patching each, turns a brittle discontinuous tie-break into a single tunable λ that the autoresearch loop can optimize, and degrades gracefully (λ→0 recovers pure RRF). A-1 is the conservative fallback if a continuous prior proves hard to tune. A-2 is the most faithful to the original intent but carries the most ongoing tuning burden.

**Eval risk: high.** This is the largest change of the eight and will move the numbers most; it must be the *last* item, landed alone, with a deliberate A/B and a willingness to sweep λ.

### Sub-question to resolve before implementing A

Which timestamp is "recency" for an *imported* memory? `updated_at` (last edit) will be the import time for freshly-imported memories — re-introducing a milder version of #5 unless `observed_at` (original session time) is preferred for imports. Worth deciding up front: **recency = `max(observed_at, updated_at)`?** or a source-aware choice. This is a semantics call, not a code detail.

---

## Proposed sequence (with eval gates)

| Step | Finding | Type | Eval risk | Gate |
|---|---|---|---|---|
| 1 | #7 prompt budget | pure-win | low | measure token delta → A/B confirms no regression |
| 2 | #6 short anchors | near pure-win | low–mod | A/B; watch OR-noise |
| 3 | #3 under-fill | near pure-win | low | A/B confirms fills more true matches |
| 4 | #1 relaxed rank discount | behavior change | **high** | isolated A/B; a drop is a signal about the eval set |
| 5 | A: recency cluster (#5,#2,#4,#8 + C6) | design change | **high** | isolated A/B; λ-sweep if A-3 |

Rationale: front-load the low-risk recall *gains* so the corpus is in its best honest state before we touch the eval-sensitive ranking knobs (#1, recency). That way the high-risk A/Bs are measured against a clean baseline, not a moving one.

---

## Open decisions still needing Trey

1. **Recency semantics (A-1 / A-2 / A-3).** My lean is A-3 (continuous prior). This is the one real design call left.
2. **Recency timestamp for imports** — `updated_at`, `observed_at`, or `max(...)`? (the #5 sub-question).
3. **Eval command + who runs it.** If you point me at the exact `stream-e-recall-bench` / `memorum-eval` invocation and corpus, I can run each A/B myself and bring you deltas; otherwise I hand you patches between your autoresearch iterations.
4. **#1 disposition if it's load-bearing** — if discounting relaxed hits drops recall@k, do we fix the ranking and accept the metric, or treat it as evidence the eval set rewards OR-noise?

---

## Appendix — verification provenance

These eight were surfaced by a 9-angle finder fan-out and confirmed by per-cluster adversarial verifiers that quoted the live code (not the plan prose). Each is CONFIRMED except #7 (CONFIRMED mechanism, token delta unmeasured). Findings that were *refuted* during that pass and are deliberately **not** here: the alleged dialoguer busy-spin hang (refuted — dialoguer 0.11 guards on `is_term()` before any read loop; the real, milder issue #10 shipped today), the unbounded `relaxed_row_limit as i64` cast (real mechanism, unreachable trigger), and the unbounded strict-BM25 pass (pre-existing, not introduced by 6/11). Cleanup-only items not tracked here: the `SQL`/`SQL_LIMITED` duplication and the drifted stopword lists (`query.rs` vs `memory-privacy`) — worth a separate tidy pass but not recall-correctness.
