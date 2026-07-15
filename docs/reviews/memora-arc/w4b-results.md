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

## Runs ledger (appended as they land)

(pending)
