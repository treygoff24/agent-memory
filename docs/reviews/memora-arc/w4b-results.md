# W4b — context-aware + selective enrichment re-gate: results

Protocol: `docs/plans/2026-07-12-w4b-context-enrichment-regate.md` (r2), pre-registered decision
rule applied verbatim. Executor: Claude coordinator, 2026-07-15 (authorized overnight-run protocol,
executed in-day).

## Prompt freeze (T2 step 3) — recorded before the full sweep

- **prompt_sha256:** `20fbbfc970521570af6ba6cf6360c44b7dba46c4c6240efccfd7d3c5802260a8`
- **window_policy:** `w4b-r2`
- Frozen after the 200-item dev pilot, 2026-07-15. No prompt edits after this point (rule 5);
  the single-sourced template (T1-F3) guarantees the hash matches the sent prompt.

## Dev pilot (T2 step 2)

200-item LoCoMo dev pilot: dispositions generated 104 / skipped_low_signal 83 / date_metadata 9 /
dropped_sensitive 0 → **null rate ≈ 44%** of harness-called turns, inside the pre-registered
20–60% band. 20-entry random inspection (seed 42): abstractions are entity-anchored durable facts
("Melanie has been married five years", "Caroline is actively pursuing adoption with optimism"),
cues follow [Entity]+[Aspect]; nulls sit on conversational filler. Completeness gate correctly
reported 4,117 keys pending under --limit (exit 1, resumable).

## Full dev sweep (T2 step 4)

Completed 2026-07-15 across ~5 resume passes (transient-failure retries are free): locomo
2900/2900 + longmemeval_oracle 1217/1217 processed; cumulative dispositions at final full pass —
generated 2368 / skipped_low_signal 1511 / date_metadata 222 / dropped_sensitive 0 (null rate
≈ 39% of harness-called turns — 1511/3879, in band). **Eligibility: 4312/4313 (99.98%).** One LoCoMo dev item
(key `3d788bc44ebeacca4959ac535a11ffa80472de99f4c08e5cf799bc3f9b2c7125`, ordinal 14, chit-chat
turn "Just the GoT series…") fails deterministically — 18 consecutive
`harness:validate:bad shape for abstraction` (model over-caps instead of nulling; prompt frozen,
untouched). Bar is 100% and non-relaxable by the executor → escalated to Trey 2026-07-15 with
recommendation: exclude the item from BOTH arms of every paired comparison (decision rule 2
operates on identical item sets, so exclusion preserves validity; power cost 1/4313). v2-consuming
runs are parked until the ruling. Two diagnostics-only eprintlns added to enrichment.rs to surface
failure identity (commit `1a4112d`); no semantic changes.

**Ruling (Trey, 2026-07-15): exclude from both arms.** Implemented as `--exclude-key
3d788bc44ebeacca4959ac535a11ffa80472de99f4c08e5cf799bc3f9b2c7125` on the benchmark runner —
exclusions match the canonical v2 context-item key in every arm and generation, are recorded in
each artifact's `split_config.excluded_keys`, and skip the write body at ingestion. Consequence
for the runs ledger: the banked dev-legacy-v1 reference was run WITHOUT the exclusion, so it is
superseded by a re-run with the flag (an exclusion that hits only one side would invalidate the
invariance comparison); all T3/T4 arms carry the flag.

## Runs ledger (appended as they land)

| run | arms/config | artifact | scored | judge_mean | recall@10 | mrr | notes |
|---|---|---|---|---|---|---|---|
| dev-legacy-v1 (T3 ref) | L, legacy fusion, v1 sidecars, fts-only | /tmp/memora-eval/dev-legacy-v1.json | 120 | 0.6167 | 0.6348 | 0.5082 | SUPERSEDED (no exclusion; wrong lane for the rule) |
| dev-legacy-v1-excl | L, legacy, v1, fts-only, excluded | /tmp/memora-eval/dev-legacy-v1-excl.json | 120 | 0.5958 | 0.6348 | 0.5082 | wrong lane for the rule; retained as judge-noise calibration |
| dev-legacy-v2 (fts) | L, legacy, v2, fts-only, excluded | /tmp/memora-eval/dev-legacy-v2.json | 120 | — | 0.6348 | 0.5082 | contexts identical to v1-excl; 0 quarantines both gens (no fence on fts lane) |
| dev-fourlane-v2 (fts) | F, four-lane, v2, fts-only, excluded | /tmp/memora-eval/dev-fourlane-v2.json | 120 | 0.6000 | 0.6348 | 0.5082 | VOID as verdict — fts-only disables vector lanes, four-lane degenerated to legacy; retrieved contexts identical to arm L → paired delta +0.0042 (10W/9L/101T) is pure judge noise. Useful calibration: judge noise on identical contexts ≈ ±0.02 aggregate |
| dev-legacy-api-v1 (rule-1 ref) | L, legacy, v1 sidecars, gemini-api, excluded | /tmp/memora-eval/dev-legacy-api-v1.json | 120 | 0.6303 | — | — | correct-protocol v1 reference; 1 judge_timeout (≤5 allowed) |
| dev-legacy-api-v2 (rule-1 cand) | L, legacy, v2, gemini-api, excluded | /tmp/memora-eval/dev-legacy-api-v2.json | 120 | 0.5917 | — | — | RULE 1 DIVERGENCE — see verdict |

## T3 verdict (2026-07-15): INCONCLUSIVE per pre-registered rule 1 — no treatment arms run

**Rule 1 (legacy invariance control) FAILED on the correct-protocol gemini-api arms.** Comparing
arm L on the v1 corpus vs arm L on the v2 corpus (both gemini-api + legacy fusion + exclusion):

- Dispositions diverge: promoted 2282 → 2321, quarantined 1799 → 1760 (39 writes flip
  quarantine→promoted under v2); refused identical at 231.
- Retrieved-context sets diverge on **33/120 items — all LongMemEval**; LoCoMo unaffected.
- judge_mean drift 0.6303 → 0.5917 (|Δ| 0.039 > 0.02 secondary alarm — flagged, though the two
  runs are on materially different corpora so the drift is not interpretable as judge drift).

The chain STOPPED before the four-lane treatment arm, exactly as pre-registered: "v2 changed the
effective legacy corpus; any four-lane comparison would be confounded. Diagnose, report
inconclusive; no treatment arms."

**Diagnosis.** The divergence is enrichment-content-driven and only manifests under the
production-like lane. On fts-only, write-time classification never gates (0 quarantines, both
generations, corpora byte-identical). On gemini-api, the Stream A API-lane privacy fence engages
per-write classification — abstraction/cues participate in the scanned payload — and ~1800 of
4082 benchmark writes classify above the plaintext-embeddable tier (quarantine→auto-approve in
the scaffold). There, v1's noisier structural/verbose abstractions push 39 additional writes over
the sensitivity boundary that v2's cleaner, sparser (44%-null) enrichment does not. Those 39
governance flips shift the retrievable corpus and move 33 LongMemEval retrieved-context sets.

**Two readings for the morning summary (Trey's call, not tonight's rule):** (a) strictly per
protocol, the A/B is confounded and four-lane stays dark — tonight's answerable question is
closed; (b) mechanistically, the divergence direction says v1 enrichment was *distorting the
corpus via privacy classification side-effects* and v2 reduces that distortion — which is
itself evidence for v2's central design claim (sparse, high-precision aux metadata), but routing
that into a verdict would require a new pre-registered rule on a v2-vs-v2 baseline, i.e. another
night. Per rule 5, no post-hoc rule changes tonight.

**Judge-noise calibration (bonus from the voided fts arms):** on provably identical retrieved
contexts, the pinned judge produced an aggregate paired delta of +0.0042 with 10W/9L/101T.
(A same-corpus judge_mean swing of similar order appears between the superseded v1 run and its
exclusion re-run, but those differ by the exclusion, so only the paired-identical figure is a
clean calibration.) The pre-registered ±0.01 ambiguity
band on paired deltas is of the same order as pure judge noise at n=120 — future rules should
either raise n or widen the band.

## Pre-ship review (2026-07-15)

Grok (cursor safe, cursor-30) integrated read of `91c51ab^..HEAD`: **no BLOCKER / no MAJOR**;
seams, critical invariants, debris, and verdict math all SHIP-CLEAN. Recorded follow-up: add an
ingest-level test for `--exclude-key` (a one-sided exclusion would silently invalidate rule 1 —
only `split_config.excluded_keys` provenance and the invariance check would catch it). The Fable
pre-ship gate was NOT pre-approved for this autonomous session and was skipped per protocol;
listed for Trey's review.
