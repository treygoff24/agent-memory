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
≈ 42% of harness-called turns, in band). **Eligibility: 4312/4313 (99.98%).** One LoCoMo dev item
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
| dev-legacy-v1 (T3 ref) | L, legacy fusion, v1 sidecars | /tmp/memora-eval/dev-legacy-v1.json | 120 | 0.6167 | 0.6348 | 0.5082 | same-night v1 reference (plan T3.1; /tmp was wiped); invariance baseline for arm-L-on-v2 |
