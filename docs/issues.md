# Known issues

## RESOLVED (shipped W1, verified in tree 2026-07-18): Import re-materializes file-sourced memories as duplicates instead of superseding

**Resolution:** the ordinal-stripping identity fix shipped in the memora-lessons arc's W1. `crates/memoryd/src/import/candidate.rs` now strips the mutable `task-group-N-` prefix via `ordinal_free_section`, disambiguates surviving slug collisions with a content-hash suffix (`disambiguate_collisions` → `section_disambiguation`), and `import_identity` prefers the recovered canonical `mem_*` id before falling back to the ordinal-free portable tuple. Covered by `ordinal_free_section_strips_task_group_prefix`, `disambiguate_collisions_*`, and the pipeline-level idempotency test `record_identity_matches_after_suffix_toggle_and_renumbered_edit`. Original analysis retained below for history.

**Found:** 2026-07-10, by the first project-scope dream run (pass-1 reflection flagged it).

**Symptom:** the same file-sourced memory accretes a new id on each re-import instead of updating/superseding the prior one. Live examples in `~/memorum` (project scope `proj_a17c5597…`):

- `memorum-dogfooding-live-setup` → `mem_20260619_…_000464`, `mem_20260708_…_000571`, `mem_20260710_…_000584`
- `cli-first-pivot-and-next-arcs` → `mem_20260708_…_000650`, `mem_20260710_…_000663`
- `memorum-import-flow-hardened…` → `000570`/`000465` pair
- `memorum-launchd-needs-absolute-binary-path` → `000015`/`000012` pair

The dated ids track the three imports (6/19, 7/8, 7/10 re-import after the CLI-first rebuild). Passive recall now surfaces near-identical duplicates side by side (visible in session-start recall blocks), and the review queue carries duplicate pending items (31 pending as of 2026-07-10).

**Root cause (re-verified 2026-07-10 with live state-file evidence; supersedes BOTH prior hypotheses):** the `ImportState::default()` amnesia theory was wrong — `run_import_session` loads persisted state at `pipeline/mod.rs:69`, overwriting the CLI's default, and the live state file exists (`~/memorum/.memorum/import-state.json`, 798 records). Two real mechanisms:

1. **Codex section keys embed a mutable ordinal.** Codex renumbers MEMORY.md task groups between runs, so the same logical section gets a new `source_key` each import: live state holds `codex:memories/MEMORY.md#task-group-4-local-factory-droid-hook-inspection-and-disable` → `mem_20260619_…_000457` AND `…#task-group-45-local-factory-droid-hook-inspection-and-disable` → `mem_20260710_…_000565`. No match → `WriteNew` → duplicate, every re-import. Fix: identity must strip the `task-group-\d+-` ordinal prefix from the section slug (with collision disambiguation).
2. **The 7/8 run's state went to the wrong repo** via the `import --repo` cwd-default bug (fixed `a2b06a3` on 7/9) — Claude-file keys have exactly one state record each, pointing at the newest id, so intermediate runs planned everything `WriteNew` against state they couldn't see. Fix already shipped; residue is the live duplicates themselves.

Open sub-question for the live repair pass: at least one changed Claude file (`memorum-dogfooding-live-setup`) shows the new id NOT superseding the old — check whether a daemon-side supersede was refused inside a Success payload (known envelope gotcha). The daemon-side `governance::contradiction` layer is semantic/embedding-based and not a correctness deduper (state identity is load-bearing; governance is a backstop). Identity contract for the fix: recovered canonical `mem_*` frontmatter id when present, portable tuple fallback with **ordinal-free section key**; ambiguous historical matches report-only. See W1 of `docs/plans/2026-07-10-memora-lessons-memorum-upgrades.md`.

**Impact:** recall-budget waste, duplicate review-queue noise, confidence dilution across copies. Not data loss.

**Repro:** run `memoryd import` twice over an unchanged Claude profile; count ids per summary.

## RESOLVED (2026-07-18): `summary` merge rule diverges on equal `updated_at` (pre-existing, found 2026-07-10)

**Resolution:** ported the ratified `abstraction` value-hash tie-break to `summary`. At equal `updated_at`, `field_rules.rs` `apply_scalar_rules` now selects the side with the lexicographically greater `sha256(NFC(summary))` (`summary_tie_key`) instead of falling through to Git-side-dependent ours-wins, and the diagnostic is emitted with side-independent `winner_value`/`loser_value` (previously `winner`/`loser_side` side labels). The `three_way.rs` deterministic `merge_id`/`created_at` derivation, previously gated to `conflicting_fields == ["abstraction"]`, now fires for any conflict set that is a subset of the convergent fields `{abstraction, summary}` so the whole merged file is byte-identical across opposite merge directions (invariant #6, §13.6.1). Pinning test: `summary_conflict_uses_updated_at_then_side_independent_hash_and_preserves_loser`. Original analysis retained below.

Spec §14.4 says same-field `summary` conflicts select "the side with later `updated_at`" — undefined at equality; the shipped `field_rules.rs` implementation falls through to ours-wins, which is Git-side-dependent and violates two-clone convergence (invariant #6) in the equal-timestamp case. Same fallthrough resolves divergent `_extras` add/add values silently ours-wins (relevant to mixed-version fields). Low practical frequency (requires identical-microsecond independent edits, or `_extras` divergence), but it's a real convergence hole. Found by the W2 spec-package review (Luna, codex-77). Fix direction: deterministic value-hash tie-break, as ratified for the new `abstraction` field in the W2 package — port it to `summary` (+ audit other newer-wins rules: `confidence`, `entities` label, `author`) in a follow-up.

## OPEN — NEEDS SPEC DECISION: shipped `_extras` merge diverges from spec §14.4 (pre-existing, found 2026-07-10)

Spec §14.4 says unknown `_extras` add/add same-key conflicts "quarantine unless values equal"; the shipped `field_rules.rs` `three_way_value` fallthrough resolves them silently ours-wins instead. Spec/code drift — one of them is wrong. Found by the W2 spec-package round-2 review (Cursor, cursor-29).

**Not fixed with the `summary` tie-break (2026-07-18), deliberately.** The two fields do *not* share a resolution: the spec's written rule for `_extras` add/add is **quarantine**, not a value-hash tie-break, so applying the `abstraction`/`summary` tie-break here would contradict the spec rather than converge with it. Quarantining instead is a larger change (`three_way_value` returns `Option<Value>`; propagating a `QuarantineReason` up through `merge_extras` is real plumbing) and is a behavior/contract decision that needs Trey's ratification per the "don't change the spec without an explicit ask" rule. Options to pick between: (a) amend §14.4 to specify the value-hash tie-break for `_extras` (matches the convergence intent, minimal code), or (b) implement the spec-as-written quarantine (bigger change, matches current wording). Decision pending.

## RESOLVED (shipped W0, verified in tree 2026-07-18): `memoryd search` FTS-only degraded path is strict-AND — zero hits for natural-language queries (found 2026-07-10 by W0)

**Resolution:** the degraded-path fallback was rerouted in W0. `crates/memoryd/src/handlers/memory_ops.rs` `fts_search_hits` now calls `substrate.query_hybrid_chunks`, which runs the same two-stage strict-AND → relaxed-OR helper (`query_hybrid_bm25_memories` in `crates/memory-substrate/src/index/query.rs`) as the fused lane, so natural-language queries return hits with no vector recall. The two-stage OR fallback is covered by `relaxed_bm25_fallback_limits_distinct_memories_not_chunks` and `relaxed_bm25_fallback_applies_rank_offset_to_or_hits`. Original analysis retained below.

When vector recall is unavailable, `memory_ops.rs` `fts_search_hits` falls back to `substrate.query_chunks` (strict-AND phrase tokens, `query.rs:520`). Natural-language questions ("What does Melanie do to destress?") require every token in a single chunk → 0 hits over a 540-memory corpus, measured by the W0 benchmark e2e. The hybrid BM25 lane already has the two-stage strict→relaxed-OR fallback (`query.rs:649-680`, RELAXED_RANK_OFFSET sweep-tuned) — the degraded path just doesn't use it. Fix: route the FTS-only fallback through the same two-stage helper, with a pinning test (natural-language query over a fixture corpus returns >0 hits in degraded mode). Scoped into W0's fix round.
