# Autonomous run record: dynamics/eval hardening backlog (2026-06-09 → 2026-06-10)

**Operator:** Claude (Fable), full autonomy granted by Trey. Orchestrator + code reviewer; implementation farmed to opus subagents under explicit file fences, coordinator-run gates.
**Plan executed:** `docs/plans/2026-06-09-dynamics-eval-hardening.md` — all 30 board tasks completed.
**Span:** 40 commits, `7c8ff81..3190303`, 296 files, +20,303/−3,594.

## What shipped, by theme

### Memory dynamics (the vision work)

- **Use-driven strength in recall ranking** (`bbcba3e`): strength = 0.45·freq + 0.35·recency(τ=14d) + 0.20·corroboration, entering ranking as a bounded additive term (alpha_points=12) so strength can never overcome a structural relevance gap ≥ alpha. OFF-state byte-identical, pinned by the version-masked STREAM_E_POLICY test (stream-e-v0.6).
- **Review-decision calibration log** (`6190f2e`): ids/metadata-only JSONL under `dreams/calibration/`, union-merged, plus a CLI report. Records regardless of `dynamics.enabled`; ranking participation is what the flag gates.
- **Fragment archival deferral** (`c425f56`): cited fragments live longer, capped by `max_fragment_lifetime_days`.
- **Spec:** memory-dynamics v0.1 (`c552bfd`).

### Embedding inference (production vector lane)

- **Qwen/Qwen3-Embedding-0.6B on Metal fp16 / CPU f32 fallback** via fastembed's candle lane (`99fa07a`). Selected empirically over EmbeddingGemma on the golden corpus: Gemma had the best raw nDCG but the worst abstention calibration ("confidently wrong"); decision recorded in `d24f713`. Asymmetric embed_query (instructed) vs embed_document (plain) is part of the provider contract. Default triple `("fastembed-candle", "Qwen/Qwen3-Embedding-0.6B", 1024)`; triple-is-identity invariant preserved (typed errors, no silent fallback).
- **Production KNN contradiction detection** (`deefa80`): sqlite-vec MATCH with over-fetch + chunk→memory collapse; write-path contradictions quarantine, never auto-supersede; degraded similarity is surfaced in the response (`similarity_degraded`), never silent.
- **Operator-tunable thresholds** (`b203feb`): contradiction `similarity_threshold`/`top_k` moved to per-policy YAML, validated fail-closed at load, defaults byte-identical to the old constants.

### Measurement instruments

- **Golden corpus** (`cd82169`): 101 memories, 50 graded queries (6 abstention), integrity lint.
- **Quality runner** (`91e07dc`): replays queries through the _real_ ranking seams (search bm25; startup candidates+rank). Regression gate arms only when a human commits `bench/quality-baseline.json`.
- **LLM-as-judge** for T13/T15 e2e (`c771690`): recorded, non-gating.
- Honest numbers as of this run: startup seam nDCG@5 = 0.1078, trap-rate@5 = 0.0227; search seam ≈ 0 (FTS5 AND-of-phrases kills natural-language queries — known, future work). These are the bm25+points baseline; recall ranking does not vector-search yet.

### Correctness fixes found along the way

- **Supersession FK aborted bulk reindex** (`0d1839e`): unguarded `memory_supersession` insert vs unsorted walkdir order. Fixed with the migration-parity EXISTS guard plus a deferred set-based resync pass in both reindex paths, so no edge is silently dropped. Found while building the quality runner (peer note `docs/2026-06-10-for-substrate-owner-supersession-fk-bulk-reindex.md`); quality metrics confirmed unchanged after removing the runner's workaround.
- Dream pass-2 corrective preamble (`d108aa8`), review-approval grounding re-verification (`0c1d7fd`), privacy enforcement downgrades surfaced (`7c8ff81`), coordination path-intersection component boundaries (`8f2816c`), web fixture fallbacks gated out of production (`8478246`).

### Structure and hygiene

Repair-cascade extraction with both divergence axes kept explicit (`5332645`); governance handler split (`23a95a3`); rendering out of protocol.rs (`7a05e69`); async recall hot path and MCP stdio bridge (`e6b86f8`, `191bf48`); incremental open-time reindex (`134e72e`); harness descriptor registry unifying four drifted lists (`dfc2e94`); dead-code deletions; dependency trims; eval-crate hygiene; TUI Reality Check parity (`1ab1a4d`); mcp_stdio de-flake (`d0f1767`); one-install-story docs + stale docs-guard inversion (`c88b744`, `3190303`).

## Verification

- Whole-crate test gates per wave at the coordinator (never single-file): substrate, governance, coordination, eval, memoryd all green; workspace clippy zero warnings; fmt clean.
- Final integrated `scripts/check.sh` on the trunk: green (after it caught two things the per-crate gates can't — oxfmt drift in 4 files and the stale `memoryd init` docs guard; both fixed in this run).
- Quality runner re-run post-FK-fix: metrics byte-identical to pre-fix (startup 0.1078), proving the corpus-staging change was behavior-preserving.

## Known blemishes and deferred work

1. **Bisect window:** `deefa80` and `dfc2e94` compile as libraries but `protocol_contract`'s test target doesn't build at those two SHAs (the old test's struct literal lacked the `similarity_degraded` field; the fix landed one commit later). Bisects across that window need `--lib`. No-amend policy means it stays.
2. **Search-seam nDCG ≈ 0:** FTS5 AND-of-phrases semantics fail natural-language queries. Candidate fixes: OR-of-terms query shaping, or routing memory_search through the vector lane. Future task.
3. **Warm query embedding latency ~240 ms on Metal** — acceptable for write-path contradiction checks, worth tuning before vector recall lands on the hot path.
4. **Corpus has no recall events**, so the dynamics strength path scores zero on golden queries; a future corpus revision should seed synthetic usage so the dynamics term is exercisable by the instrument.
5. **Quality baseline not yet armed:** `bench/quality-baseline.candidate.json` awaits Trey's review → human-authored commit as `bench/quality-baseline.json` (same convention as bench baselines).

## Orchestration lessons (for the next run)

- Three parallel subagents in one shared tree each running whole-crate cargo gates serialize on the build lock; the 600 s no-progress watchdog killed all three _after_ their edits landed but _before_ their reports. Their work was complete and correct on disk. Lesson: in a shared tree, agents implement; the coordinator runs gates sequentially.
- The agents' disjoint file fences held perfectly across both parallel waves — fence discipline in the brief is what made stall recovery cheap (review diffs, gate once, commit per scope).
- Per-crate gates lie by omission: only the integrated `scripts/check.sh` caught the oxfmt drift and the docs-guard staleness. Run it at every closeout, not just at the end of a phase.
