# W4c — v2-vs-v2 four-lane re-gate: pre-registration + results

Authorized by Trey 2026-07-16 ("let's run v2 vs v2, see what happens"), resolving reading (b) of
`w4b-results.md` into a testable protocol. Executor: Claude coordinator, in-session.

## The question

W4b established that enrichment *content* shifts write-time privacy classification, so v1-corpus
vs v2-corpus comparisons are confounded. W4c removes the confound by construction: **both arms run
on the identical v2-enriched corpus** (same sidecars, same gemini-api lane, same exclusion). The
only variable is the fusion policy. This is the clean test of the W4 hypothesis ("aux input
quality was the bottleneck, not the fusion architecture") that neither prior night delivered.

## What carries over unchanged (frozen)

- v2 sidecars: `datasets/*/.*.enrichment.v2.json`, prompt sha `20fbbf c9…` (frozen 2026-07-15, untouched)
- Pinned judge: `scripts/eval/pinned-judge.sh` (luna low, frozen 2026-07-10)
- Sampling: `--locomo-qa-per-conversation 12 --longmemeval-per-split 60`, dev split only
- Exclusion: `--exclude-key 3d788bc44ebeacca4959ac535a11ffa80472de99f4c08e5cf799bc3f9b2c7125`
  on EVERY arm (Trey's 2026-07-15 ruling)
- Judge-failure rule: >5 judge errors of 120 in an arm = failed run; rerun once; twice = inconclusive
- All paired comparisons computed over the intersection of successfully judged items

## Pre-registered decision rule (written 2026-07-16, before any W4c number exists)

The `/tmp/memora-eval` artifacts from W4b did not survive; all arms are re-run fresh this session.
Artifacts now land in `~/memora-eval/` (durable) — the /tmp loss is itself the lesson.

1. **Corpus-identity control.** Arm C (legacy) and arm T (four-lane) share generation, lane, and
   exclusion, so their ingested corpora must be *identical*: dataset sha256s, selected item IDs,
   and governance/ingestion disposition counts (promoted/quarantined/refused) must match exactly
   between arms. Any mismatch → STOP (it would indicate nondeterminism in the write path, a bug
   senior to this experiment). Retrieved contexts are expected to differ — that is the treatment.
2. **Dev gate.** Best four-lane config must beat same-session arm C by **paired delta ≥ +0.03**.
   The band is widened from W4b's +0.01 per the judge-noise calibration banked that night
   (paired delta on provably identical contexts ≈ +0.004; ±0.01 is the same order as noise at
   n=120). Sweep budget **≤3 configs**: Memora defaults (1.0/1.0/2.0/1.0) first, then at most 2
   informed adjustments (W4 evidence says gentle-aux 1.0/1.0/0.5/0.5 is the strongest candidate).
   - Paired delta in (0, +0.03) → **ambiguous**: flag stays dark; numbers go to Trey.
   - Paired delta ≤ 0 → hypothesis dead on clean ground; flag stays dark; write closeout.
3. **Holdout freeze (only on an unambiguous dev win).** Freeze the winning config; enrich the
   holdout split blind under the frozen v2 prompt (it has never been v2-enriched); run holdout
   once per arm. Four-lane passes iff holdout paired delta ≥ 0 AND neither dataset regresses
   > 0.05 vs arm C on holdout (collapse guard). Holdout execution requires a fresh go from Trey
   (codex enrichment sweep, ~hours) — the dev verdict is reported first.
4. **Outcomes.** Pass → flip `four_lane_enabled` default true + pinning test + results. Fail or
   ambiguous → flag stays dark; a negative verdict is a fully acceptable outcome.
5. No fourth config, no post-hoc metric or band changes, no holdout re-rolls. Anything the rule
   does not cover goes to Trey, not into another run.

## Runs ledger (appended as they land)

| run | arms/config | artifact | scored | judge_mean | paired Δ vs C | notes |
|---|---|---|---|---|---|---|
| w4c-armC | C, legacy, v2, gemini-api, excluded | ~/memora-eval/w4c-armC-legacy-v2.json | 120 | 0.5458 | — | 0 judge errors; dispositions 2327/1754/231 (promoted/quarantined/refused) |
| w4c-armT | T, four-lane Memora defaults, v2, gemini-api, excluded | ~/memora-eval/w4c-armT-fourlane-v2.json | 120 | 0.6292 | **+0.0833** (23W/12L/85T) | 0 judge errors; dispositions 2326/1755/231; per-dataset Δ: LoCoMo +0.0417, LongMemEval +0.1250 |

## Rule-1 status (2026-07-16): 1-write disposition mismatch — verdict escalated to Trey

Corpus-identity control: dataset sha256s identical, item ID sets identical (120/120), refusals
identical (231). Initial dispositions differ by exactly **one write of 4,081**: promoted 2327 vs
2326, quarantined 1754 vs 1755. The scaffold auto-approves quarantines, so the final retrievable
corpus is the same 4,081 writes in both arms; the flip affects at most write-path timing/tier for
one memory. Ingestion records carry only run-minted `mem_*` ids (no stable context-item key), so
the flipped write cannot be pinpointed from the artifacts — recorded as a harness follow-up.
Context: arm C also drifted ~6 dispositions from the W4b v2 run (2321/1760), so there is a
low-level nondeterminism source in write-time governance under the gemini-api lane (suspect:
transient API-path effects), independent of fusion policy.

Materiality note (diagnostic, not a rule change): the treatment delta spans 35 non-tie items
across both datasets and both directions; a single write's disposition cannot produce it. Rule 1
as written says STOP on any mismatch; per rule 5 the disposition goes to Trey rather than into
another run. **Numbers above are reported as diagnostic pending Trey's ruling on whether the
1-write mismatch voids the comparison.**
