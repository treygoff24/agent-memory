# Plan: Memora lessons → Memorum upgrades

**Status:** DRAFT — not reviewed, not scheduled. Written from a same-day read of `microsoft/Memora` (cloned at `~/Code/Memora`, ICML 2026, MIT license) against live Memorum source.

**Context:** Memora (Microsoft Research) sets SOTA on LoCoMo (86.3%) and LongMemEval (87.4%), beating Mem0/Zep/RAG/full-context, with ~half Mem0's entry count and up to 98% fewer context tokens. Its three mechanisms map cleanly onto Memorum gaps — including the import-duplication bug logged in `docs/issues.md` the same day this plan was written.

## What Memora actually does (from source, not the blog)

1. **The retrieval key is a short abstraction, not the content.** Each memory = `index` (6–8 word phrase, e.g. "Updated Project Orion timeline agreed by Dave and Sarah") + `value` (rich content). Only the index phrase is embedded: `local_memory_store.py:157` upserts `documents=[index]`. Values are never embedded.
2. **Cue anchors are tiny linked index entries.** 1–3 phrases (2–4 words, `[Main Entity] + [Key Aspect]`) generated per memory at write time (`cue_index_generator.py`), stored as their own embedded rows with `linked_memory` pointing at primary entries. Retrieval searches primary + cue lanes + BM25, merges with weighted RRF (primary 2.0, cue 1.0, hybrid 1.0; `memory.py:231`).
3. **Merge-on-write.** New extraction → exact-index dup check → vector search over existing *abstractions* (top-5, cosine ≥ 0.8 `update_score_threshold`) → LLM decides update-vs-add → update rewrites value+index, regenerates cues, keeps history (`memory_builder.py:365-579`). This is why Memora stores 344 entries where Mem0 stores 651.
4. **Policy-guided retrieval.** Iterative loop with three actions — EXPAND (pull frontier items reachable via shared cues/links), RE_QUERY (reformulate; handles "relative answer" pointers like "same college as Sarah"), STOP — driven by a prompted LLM or an RL-distilled small model (`prompted_policy_retriever.py`). Biggest wins on multi-hop.

**What we are deliberately NOT adopting:** ChromaDB/centralized store (our substrate is canonical files + git); the natural-language phrase as the *primary key* (their own code grows warts from it — index-collision rename hacks at `memory_builder.py:528`, episodic "(2)" suffixing at `memory.py:607`; our stable `mem_*` ids + abstraction-as-field is strictly better); synchronous LLM calls inside the write path (violates Memorum's daemon architecture — see T2 for where that decision moves).

## Fit with Memorum today

- Memorum embeds **body chunks** (`crates/memoryd/src/embedding/worker.rs`, per-chunk `body_hash`) — exactly the "content-fragmentation" pole Memora's paper argues against. Our `summary` frontmatter is already ~an abstraction; it just isn't the retrieval key.
- The **v4.0 trigger index** (`docs/specs/stream-e-ambient-recall-v4.0.md` §5) is convergent with cue anchors but dream-compiled and deterministic-match-only. Memora's lesson: also mint cues *at write time* from the memory value, and give them a vector lane, not just exact activation-condition matching.
- The **import duplication bug** (`docs/issues.md`, found by the 7/10 dream run) is the exact failure mode merge-on-write exists to prevent.
- The **API embedding lane** (11–17 MB live) makes abstraction-only embedding extra attractive: tiny inputs, lower cost, and a possible privacy unlock (T5 spike).

## Tasks (dependency order)

### T1 — Deterministic import dedup by source identity (the bug fix)

Fix `docs/issues.md`: key file-sourced imported memories by stable source identity (source file path + repo/profile) instead of per-import-batch identity. Re-import of an unchanged file = no-op; changed file = **supersede** the prior memory (existing Stream A supersession machinery), never a sibling.

- Owner surface: import pipeline (`crates/` import path), one migration/repair pass for the live `~/memorum` duplicates (the dream-flagged sets: 000584/000571/000464 etc.).
- No LLM needed — source identity is deterministic. This is the narrow, shippable core of "merge-on-write."
- Gate: `cargo test -p <import crate>`; live re-import twice over an unchanged profile → id count stable; dedup repair drains the review-queue duplicates.

### T2 — Near-duplicate detection routed through dreaming (merge-on-dream)

Memora merges at write time with a synchronous LLM call. Memorum's daemon must not block writes on model calls — but we already have an asynchronous consolidation organ: **dreaming**. Add a dream-pass responsibility: candidate near-duplicate sets (cosine similarity over summaries/abstractions above threshold, computed deterministically daemon-side) are handed to the dream pass, which proposes supersede/merge operations through existing Stream C governance (quarantine → review → approve).

- Reuses: pass-2 promotion machinery, governance review surface, tombstone/supersede invariants.
- Steal the constant: candidate floor ≈ 0.8 cosine on abstraction embeddings (Memora's `update_score_threshold`), tune via T6 eval.
- Gate: dream run over a corpus seeded with known near-dups proposes the right merges; nothing auto-merges without governance approval.

### T3 — Abstraction as a first-class retrieval key

Add an `abstraction` discipline: a short (≤ 8 words) retrieval phrase per memory. Two sources, no daemon LLM calls: (a) the writing agent supplies it at `memoryd remember` time — the CLI contract already assumes an LLM agent is the writer; (b) dreams backfill/repair weak ones. Embed the abstraction (and cues, T4) as additional vectors alongside — not instead of — body chunks initially; retrieval fuses lanes with weighted RRF (Memora weights: abstraction 2.0, cue 1.0, chunk/FTS 1.0).

- Spec impact: Stream A frontmatter addition (additive amendment candidate); Stream E retrieval fusion change (version bump — behavior change). **Requires Trey's explicit version-bump approval per repo convention.**
- Embedding-input change is identity-relevant: same `(provider, model_ref, dimension)` triple, new *row kind*. Store abstraction/cue vectors as distinct embedding kinds, never mixed into chunk tables — no silent re-interpretation of existing vectors (invariant #3 stays intact).
- Gate: A/B on T6 eval — chunks-only vs chunks+abstraction+RRF. Ship only on a win (eval-gated merge order, per project memory).

### T4 — Write-time cue anchors feeding the v4 trigger index

Extend the v4.0 trigger-index design: in addition to dream-compiled activation conditions, the writing agent may supply 0–3 `cues:` (2–4 word `[entity] + [aspect]` phrases; adopt Memora's `cue_index_generator.py` prompt guidelines nearly verbatim into the `using-memorum` skill / CLI contract). Cues get embedded rows in the cue lane (T3) and also register as trigger-index terms for the hermetic work-stream path.

- Keeps v4's hard invariant: the PostToolUse path stays no-network/no-subprocess — cue *matching* there is trigger-index term matching; the vector lane only serves prompt/desk cues.
- Spec impact: v4.0 spec amendment (additive) + CLI contract v1 field addition.
- Gate: cue-lane retrieval surfaces memories that chunk-semantic search misses (seeded multi-hop fixture set from T6).

### T5 — SPIKE: abstraction-only transit for the API embedding lane (privacy unlock)

Today the Gemini lane fences `confidential`/`personal` memories entirely local (28 jobs held as of 7/10) — vector recall for them is FTS-only. If the *retrieval key* is a short abstraction rather than content, a Stream-D-classified, transit-safe abstraction could be embedded via the API lane while the value never leaves the machine.

- This is a consent-fence change. **Spike only**: Stream D analysis of abstraction leakage (an abstraction of a secret is still derived from it — likely needs the abstraction itself independently classified, and plausibly a separate consent ceremony). No implementation without a ratified Stream A/D spec amendment and Trey's sign-off.
- Deliverable: 1–2 page memo with recommendation, not code.

### T6 — Adopt LoCoMo + LongMemEval in Stream H

Port the two public benchmarks into the eval harness (Memora's `app/locomo/`, `app/longmemeval/` runners are MIT-licensed reference implementations; their configs document the full method matrix). Baseline Memorum as-is, then use it as the A/B gate for T3/T4 and the tuning surface for T2's threshold.

- Also gives us standing against published numbers for Mem0, Zep, LangMem, RAG, full-context, Memora.
- Note: these benchmarks measure conversational QA recall, not Memorum's governance/privacy value — treat as a retrieval-quality gate, not a product scorecard.

### T7 (future, after T3/T4 land) — One-hop frontier expansion in recall

Memora's EXPAND action, adapted: candidates sharing cue anchors with the top-k working set form a frontier; the v4 retina judge (already a planned single fast-model call) picks among deterministic candidates *including frontier items*. RE_QUERY-style iterative reformulation stays out of scope for passive recall (latency budget); it belongs to active `memoryd recall` invoked by agents, where a second hop is cheap.

- Fold into the v4 implementation plan rather than executing standalone.

## Sequencing & gates

- T1 is a standalone bug fix — do first, independent of everything.
- T6 (benchmarks) before T3/T4 so their merge is eval-gated.
- T2 after T1 (dedup repair changes the corpus dreams see).
- T5 spike can run any time; informs whether T3's abstraction embedding uses the API lane for sensitive tiers.
- Full `scripts/check.sh` on integrated trunk only, per CPU discipline. Spec bumps only with explicit approval.

## Risks

- **Abstraction quality is now load-bearing** (T3): a vague abstraction is a lost memory. Mitigations: keep chunk lane in the fusion (never abstraction-only), dream repair pass, eval gate.
- **RRF weights are Memora's, tuned on their benchmarks** — treat 2.0/1.0/1.0 as starting points; T6 sweeps them (deterministic eval makes knob sweeps cheap, per project memory).
- **T2 threshold too aggressive** merges distinct memories → governance review is the backstop; nothing merges un-reviewed.
- **Benchmark seduction** (T6): LoCoMo optimizes conversational QA; Memorum's differentiators (privacy, governance, git durability, multi-device) score zero there. Don't let the leaderboard steer the product.
