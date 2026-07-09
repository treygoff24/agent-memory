# API embedding lane — execution log

Per the blocked->30min rule. Running state notes for the 2026-07-09 build arc
(`docs/plans/2026-07-09-api-embedding-lane.md` v0.2).

## 2026-07-09 ~13:45 CDT — BLOCKER: all codex lanes down (auth)

- **What happened:** Trey's personal Codex subscription hit its usage limit mid-run, killing the T2.2 Sol
  lane (codex-66). Per Trey's directive, all `delegate codex` launches are rerouted to his WORK auth until
  6:30pm CDT (`codex.authProfile: work` in `~/.delegate/config.personal.json`, `fallbackProfile: work` in
  base config; backups at `~/.delegate/config.*.pre-worklimit-20260709`; one-shot revert scheduled in-session).
- **Blocker:** the work Codex OAuth token is REVOKED (stale since 6/30; server returns `token_revoked`).
  Needs Trey: `CODEX_HOME=~/.ai-profiles/runtime/codex/work codex login` (browser OAuth). NOTE: that
  home's `auth.json` is a symlink to `~/.ai-profiles/work/codex-auth.json` — verify it survives login.
- **What was tried:** rerouted delegate config (verified via smoke call — reaches work home, 401s),
  inspected both auth homes.
- **What unblocks:** Trey's work login. Then relaunch remaining codex tasks (T3.1 terra, T3.2 luna).
- **Salvage:** the "dead" T2.2 run had actually completed its implementation before dying (killed during
  its own gate). Orchestrator audited the diff (complete, all deliverables, fence untouched) and is
  running the crate gate directly. If green, T2.2 commits and Wave 2 closes without a relaunch.

## Status snapshot at blocker time

- Wave 1: committed (`13c20a1`), Grok-reviewed.
- Wave 2: T2.0/T2.1a/T2.1b committed (`ba4eb38`, `b649c43`, earlier); T2.3 committed (`24a9a10`); sacred
  fence gate PASSED (grok cross-family + orchestrator verdict); T2.2 gate in flight on salvaged diff.
- Wave 3: briefs staged (`/tmp/memorum-briefs/T3.1-*.md`, `T3.2-*.md`); execution blocked on codex auth.
- Wave 4: T4.1/T4.2 staging in progress by orchestrator (not blocked).
