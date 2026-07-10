# Known issues

## OPEN: Import re-materializes file-sourced memories as duplicates instead of superseding

**Found:** 2026-07-10, by the first project-scope dream run (pass-1 reflection flagged it).

**Symptom:** the same file-sourced memory accretes a new id on each re-import instead of updating/superseding the prior one. Live examples in `~/memorum` (project scope `proj_a17c5597…`):

- `memorum-dogfooding-live-setup` → `mem_20260619_…_000464`, `mem_20260708_…_000571`, `mem_20260710_…_000584`
- `cli-first-pivot-and-next-arcs` → `mem_20260708_…_000650`, `mem_20260710_…_000663`
- `memorum-import-flow-hardened…` → `000570`/`000465` pair
- `memorum-launchd-needs-absolute-binary-path` → `000015`/`000012` pair

The dated ids track the three imports (6/19, 7/8, 7/10 re-import after the CLI-first rebuild). Passive recall now surfaces near-identical duplicates side by side (visible in session-start recall blocks), and the review queue carries duplicate pending items (31 pending as of 2026-07-10).

**Hypothesis:** `memoryd import` keys file-sourced memories by something that changes across runs (import batch / timestamp) rather than by stable source identity (file path + content hash), so re-import creates rather than supersedes. Fix likely lives in the import dedup/supersede path — see `docs/2026-06-12-for-codex-import-repair-supersede-index-corruption.md` for prior art in this area.

**Impact:** recall-budget waste, duplicate review-queue noise, confidence dilution across copies. Not data loss.

**Repro:** run `memoryd import` twice over an unchanged Claude profile; count ids per summary.
