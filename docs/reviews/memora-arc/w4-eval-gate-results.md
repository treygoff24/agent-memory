# W4 eval-gate results — four-lane fusion does NOT clear the honest bar (2026-07-11)

## Protocol

Per plan §W4 gate + pre-registered splits/judge (BUILD-STATE 2026-07-10): enriched corpus (8,596
bodies, sidecars from the W4-prep codex sweep), pinned judge (`scripts/eval/pinned-judge.sh`,
luna low, frozen 2026-07-10), sampling `--locomo-qa-per-conversation 12 --longmemeval-per-split 60`,
dev split only (holdout untouched — see verdict), one flag/weight set apart per arm. Runner gained
`--w-*` weight overrides for the pre-authorized dev sweep (worktree commit `47b945e`); weights are
recorded in each artifact's `split_config.fusion_weights`.

Artifacts (6MB each, /tmp/memora-eval/, summarized here): `baseline1.json`, `dev-legacy-api.json`,
`dev-fourlane-api.json`, `dev-fourlane-s1.json`, `dev-fourlane-s2.json`.

## Dev-split results (judge_mean; n=120 per arm, 60 per dataset)

| arm | weights (chunk/bm25/abs/cue) | LoCoMo | LongMemEval | ALL |
|---|---|---|---|---|
| baseline₁ — FTS-only, legacy | — | 0.3559 | 0.8667 | 0.6134 |
| arm L — gemini vectors, legacy 2-lane | — | 0.5417 | 0.8583 | **0.7000** |
| arm F — four-lane, Memora defaults | 1.0/1.0/2.0/1.0 | 0.4569 | 0.8362 | 0.6466 |
| S1 — four-lane, gentle aux | 1.0/1.0/0.5/0.5 | 0.4750 | 0.8509 | 0.6581 |
| S2 — four-lane, cue-only aux | 1.0/1.0/0.0/1.0 | 0.4746 | 0.8390 | 0.6568 |

Paired per-item (vs arm L): F-defaults 7W/18L/91T · S1 5W/13L/99T · S2 6W/16L/96T.
Retrieved-relevant churn (S1 vs arm L): +183 gained / −186 lost — the aux lanes surface real
complementary hits but displace an equal number of more-judge-critical ones at top-K.

## Verdict

- **Literal plan gate (beat baseline₁): PASSED** by every four-lane config.
- **Honest gate (beat what the same vectors do under the existing legacy fusion): FAILED** by every
  four-lane config, at the 3-config sweep cap. Enabling four-lane as shipped would make explicit
  search *worse* than leaving the fusion flag alone.
- Holdout NOT scored (preserved for a future freeze); main NOT fast-forwarded. Merge decision
  escalated to Trey.

## Interpretation (hypotheses, not conclusions)

1. **Aux-lane quality, not architecture:** enrichment abstractions/cues are single-shot one-liners
   from a codex sweep over benchmark turns; Memora's were produced by its full dream pipeline.
   Dream-generated abstractions over the live corpus (W5) may be materially better. The fusion code
   is sound (11 findings closed; RRF math verified by two reviewers) — the *inputs* underperform.
2. **RRF dilution is real:** any weight on a noisy lane trades top-K slots away from the chunk lane;
   the +183/−186 churn shows the ceiling is set by aux precision, not by weighting.
3. **The big win was already banked:** vectors themselves (arm L) are +0.087 judge over FTS-only.
   That path ships today on the API lane.

## Options for the merge decision

- **(a) Merge W4 dark:** flip `four_lane_enabled` default to `false` (+ pinning test), merge the
  reviewed code + observability + A/B tooling; re-run the gate after W5 produces dream-quality
  abstractions on the live corpus. Preserves the work; ships nothing regressive.
- **(b) Merge with cue-only weights on:** beats baseline₁ but knowingly worse than legacy on the
  same vectors — not defensible.
- **(c) Don't merge:** W4 dies on its branch; the sweep tooling and latency counters die with it.

Coordinator recommendation: **(a)**.
