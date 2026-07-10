# Known issues

## OPEN: Import re-materializes file-sourced memories as duplicates instead of superseding

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

## OPEN: `summary` merge rule diverges on equal `updated_at` (pre-existing, found 2026-07-10)

Spec §14.4 says same-field `summary` conflicts select "the side with later `updated_at`" — undefined at equality; the shipped `field_rules.rs` implementation falls through to ours-wins, which is Git-side-dependent and violates two-clone convergence (invariant #6) in the equal-timestamp case. Same fallthrough resolves divergent `_extras` add/add values silently ours-wins (relevant to mixed-version fields). Low practical frequency (requires identical-microsecond independent edits, or `_extras` divergence), but it's a real convergence hole. Found by the W2 spec-package review (Luna, codex-77). Fix direction: deterministic value-hash tie-break, as ratified for the new `abstraction` field in the W2 package — port it to `summary` (+ audit other newer-wins rules: `confidence`, `entities` label, `author`) in a follow-up.

## OPEN: shipped `_extras` merge diverges from spec §14.4 (pre-existing, found 2026-07-10)

Spec §14.4 says unknown `_extras` add/add same-key conflicts "quarantine unless values equal"; the shipped `field_rules.rs` `three_way_value` fallthrough resolves them silently ours-wins instead. Spec/code drift — one of them is wrong. Found by the W2 spec-package round-2 review (Cursor, cursor-29). Fix direction TBD with the `summary` tie-break audit above (same fallthrough).
