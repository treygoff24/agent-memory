# Run record — recall-ranking fixes (Claude orchestrator × Cursor Composer)

**Date:** 2026-06-12. **Plan:** `docs/reviews/2026-06-12-recall-ranking-findings.md` (all eight findings, three clusters). **Process:** Claude orchestrated and code-reviewed; every implementation was delegated to Cursor Composer 2.5 via `delegate --isolation worktree cursor work`, one fix per eval cycle, each gated by an isolated A/B on the deterministic recall-quality eval (`memorum-eval-quality --embedding real`, golden fictional corpus). Workspace gate (`scripts/check.sh`) green on the integrated trunk at the end.

## Commits (in order)

| Commit | Finding | A/B result (nDCG@5 / recall@5 / MRR / trap@5) |
|---|---|---|
| baseline @ `9c067a7` | — | 0.7757 / 0.790 / 0.860 / 0.200 (reproduces 6/11 run record exactly) |
| `d8a8c7f` | #7 sequence cap 512→640 | unchanged — win is on the governance contradiction path, which this eval doesn't measure |
| `a878291` | #6 identifier anchors in relaxed FTS | 0.7754 / 0.790 / 0.860 / 0.200 (noise-level tail reorder) |
| `d260aac` | #3 memory-level LIMIT in relaxed fallback | unchanged — corpus never tripped chunk-cap starvation |
| `631d2ff` + `06c6521` | #1 relaxed-lane rank discount | offset sweep below; landed at 15 → **0.7776 / 0.787 / 0.857 / 0.200** |
| `a7c8cfe` | recency cluster (#2 #4 #5 #8, C6) → A-3 continuous prior | 0.7773 / 0.787 / 0.850 / 0.200 at λ=0.0005 |

Final state vs baseline: nDCG@5 +0.0016, recall@5 −0.003, MRR −0.010, trap@5 flat — all eight correctness defects fixed at net-zero metric cost.

## Sweeps (deterministic, local uncommitted patches)

**RELAXED_RANK_OFFSET** (#1): 0 → 0.7754 nDCG; **15 → 0.7776 (kept)**; 30 → 0.7736; 60 → 0.7622. The naive offset-60 discount cost real recall (0.790→0.767) with zero trap improvement — the OR fallback was surfacing true answers, not noise. Offset 15 beat the undiscounted behavior outright, dissolving the findings doc's open decision #4 (no metric-vs-correctness tradeoff needed).

**recency λ**: 0 → 0.7769; **0.0005 → 0.7773 (kept, the shipped default)**; 0.001 → 0.7704; 0.002 → 0.7592 with trap *rising* to 0.22. Stronger recency causes inversions without suppressing traps.

## Findings for the next loop

1. **The remaining trap@5 0.20 is not recency-separable** at any λ that doesn't also wreck nDCG. The 6/11 run record's pending "trap re-suppression via recency window re-tune" directive is unlikely to pay off as stated; per-case analysis (the `memorum-eval --dump-cases` backlog item) is the prerequisite.
2. **The instruction prefix measures exactly 67 Qwen3 tokens** (332 chars). Recorded here so future prompt tuning can budget against the 640 cap without re-measuring.
3. Measured token math, sweep reports, and per-fix fused JSON reports are in the session job dir (ephemeral); the numbers above are the durable record.
