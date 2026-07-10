# Findings triage — W1 import-dedup fix, round 1

Diff under review: Terra's uncommitted worktree diff (codex-76, `delegate/codex-20260710T220306Z_48771d`). Reviewers: coordinator read, Cursor safe (cursor-1), Luna high safe (pending merge below). Coordinator gate on the worktree: `cargo test -p memoryd -- --test-threads=2` → **3 failures** (Terra fixed fixture compile errors but never re-ran): `execute_candidate_with_supersede_next_action_issues_followup_supersede`, `execute_promoted_with_existing_id_counts_as_dedup_not_new`, `second_run_with_unchanged_content_but_wrong_project_bucket_repairs_bucket`.

Verdict: **NEEDS-REWORK** (unanimous). The identity scaffolding is sound; the live root cause is not fixed and the new keying model breaks four adjacent paths.

## Accepted findings (fix round 1 scope)

| # | Sev | Finding | Source | Fix contract |
| --- | --- | --- | --- | --- |
| F1 | BLOCKER | Codex ordinal survives in identity tuple — live bug unfixed. `import_identity` embeds `section` verbatim (`task-group-45-…`) | coordinator + cursor | Strip the `task-group-\d+-` prefix from the section component at identity computation (keep `source_key` verbatim for display). Collision policy: two sections in the same file whose ordinal-free slugs collide → disambiguate by content-hash suffix at parse time and surface both as `ReportAmbiguous` when historical records match ambiguously. Fixture: renumber `task-group-N` between two runs → stable memory_id count |
| F2 | BLOCKER | v1-record migration never happens on `SkipUnchanged` (comment claims it does); ordinal orphans never pruned for anchor-less records | coordinator + cursor | On `SkipUnchanged` with a matched record, rewrite it under `source_identity` and remove the old key (state save is already per-record atomic). Prune superseded/stale keys by identity, not only by `source_memory_id` |
| F3 | BLOCKER | Re-import over the already-duplicated live corpus never reaches `ReportAmbiguous` (legacy records have empty identity fields; only exact source_key compat matches) | cursor | After F1+F2: also match legacy records by ordinal-free identity recomputed from their stored `source_key`; multiple hits → `ReportAmbiguous`. This is what makes report-only real for the live repair pass |
| F4 | MAJOR | Supersession-chain lookup uses `state.imports.get(&source_key)` — misses rekeyed records; W3 lineage silently lost | cursor | Resolve the prior record via the same plan-time match (identity/anchor/compat), thread it through `PlannedWrite` instead of re-looking-up by key |
| F5 | MAJOR | `retain` can leave two map entries for one `memory_id` (daemon-dedup path) and keeps stale ordinal keys forever | cursor | Retain/replace by `source_identity` + matched-prior-key + `memory_id`; explicit prune of the matched record's old key |
| F6 | MAJOR | `alias_to_id` seeded from map keys, which are now identities (`tuple:…`) not source keys — wiki-link resolution degrades post-migration | cursor | Seed from `record.source_key` (and stored aliases), never the map key |
| F7 | MAJOR | Identity's profile component is weak on two axes: anchor matches ignore harness/profile entirely (copied frontmatter ids collapse distinct sources), and the tuple's profile component is a bare directory basename (`/a/.claude/…` vs `/b/.claude/…` collide; symlinked profiles diverge) | cursor + luna | Constrain anchor matches to same harness; derive the tuple's profile component from the canonicalized (symlink-resolved) profile root path, not its basename; tests for multiple explicit roots + symlinked roots |
| F8 | MAJOR | Plan §W1 edge-case + gate tests missing: rename-only pipeline, content+rename, profile-symlink same-file, identical relative paths across profiles, path reuse, double re-import stable-id count, ordinal renumber | coordinator + cursor | Implement all as pipeline-level tests (unit identity tests alone don't satisfy the gate) |
| F9 | MAJOR | 3 failing tests in the worktree (see header) — fixture changes never validated | coordinator | Make the full `cargo test -p memoryd` suite green |
| F10 | MAJOR | Supersede retry can mint multiple replacements: daemon commits the replacement, crash/failure before state save → next run re-supersedes the old id and the substrate creates a fresh replacement per request | luna | Bounded fix (no substrate change): before issuing a Supersede, read the prior memory's `superseded_by` chain from the daemon; if it is already superseded by a memory whose content hash matches the candidate, adopt that id into state instead of writing again. Simulated-crash test (state save suppressed between runs), not true crash injection |
| F11 | MINOR | Stale doc comment: state.rs still says map keyed by portable `source_key` | cursor | Update comments to the identity-keyed model |
| F12 | NIT | ReportAmbiguous absent from harness counters; tuple uses `:` delimiter; `.codex` profile detection is exact-match while `.claude*` is prefix | cursor | Counter + `.codex*` prefix; delimiter left as-is (paths with `:` are already excluded by tree constraints) |

## Rejected / deferred

- Generic "crash between Promoted and save_atomic → stale state" (cursor MAJOR, luna overlap) — the *general* crash-journaling problem is pre-existing and out of W1 scope; the bounded supersede-specific consequence is accepted as F10. DEFERRED beyond F10's mitigation.

Luna round-1 (codex-1, 1 BLOCKER + 4 MAJOR + 2 MINOR, NEEDS-REWORK) merged above: convergent on F1/F2/F3/F4; unique adds folded into F7 (profile-root identity) and F10 (supersede retry duplication).
