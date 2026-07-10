# Known issues

## OPEN: Import re-materializes file-sourced memories as duplicates instead of superseding

**Found:** 2026-07-10, by the first project-scope dream run (pass-1 reflection flagged it).

**Symptom:** the same file-sourced memory accretes a new id on each re-import instead of updating/superseding the prior one. Live examples in `~/memorum` (project scope `proj_a17c5597…`):

- `memorum-dogfooding-live-setup` → `mem_20260619_…_000464`, `mem_20260708_…_000571`, `mem_20260710_…_000584`
- `cli-first-pivot-and-next-arcs` → `mem_20260708_…_000650`, `mem_20260710_…_000663`
- `memorum-import-flow-hardened…` → `000570`/`000465` pair
- `memorum-launchd-needs-absolute-binary-path` → `000015`/`000012` pair

The dated ids track the three imports (6/19, 7/8, 7/10 re-import after the CLI-first rebuild). Passive recall now surfaces near-identical duplicates side by side (visible in session-start recall blocks), and the review queue carries duplicate pending items (31 pending as of 2026-07-10).

**Root cause (verified 2026-07-10, supersedes the original hypothesis):** the dedup machinery exists and is correct — `plan_action_for_record` (`crates/memoryd/src/import/pipeline/execute.rs:369-380`) plans `SkipUnchanged` on content-hash match and `Supersede` on change, keyed by stable `source_key` + `content_hash`. But the CLI import path constructs `ImportState::default()` (`crates/memoryd/src/cli/import.rs:30`) instead of loading persisted state, so every run starts amnesiac and plans everything as `WriteNew`. Secondary question for the fix: why the daemon-side `governance::contradiction` layer (documented as the load-bearing dedup per `import/state.rs:3-7`) also failed to catch the duplicates. Two review findings sharpened the fix contract: key on the recovered canonical `mem_*` frontmatter id when present (survives renames), portable tuple as fallback; ambiguous historical repairs are report-only. See W1 of `docs/plans/2026-07-10-memora-lessons-memorum-upgrades.md`.

**Impact:** recall-budget waste, duplicate review-queue noise, confidence dilution across copies. Not data loss.

**Repro:** run `memoryd import` twice over an unchanged Claude profile; count ids per summary.
