# Import dogfood log — 2026-06-18

Running log of a live dogfooding test: importing all of Trey's real Claude Code and Codex CLI memory into the live Memorum store via `memoryd import`. Captures errors, ergonomics friction, and concrete improvement ideas as they surface. Author: Claude, acting as Trey's agent.

**Environment**
- Daemon: `com.memorum.daemon` under launchd, pid 9930, repo `/Users/treygoff/memorum`, socket `/Users/treygoff/memorum/.memoryd/memoryd.sock`.
- Binary: `memoryd` (`~/.cargo/bin/memoryd`), version 0.1.0.
- Store health at start: `memoryd doctor` → `healthy: true`, no findings. ~208 `mem_*.md` already in substrate (dream output + 3 prior Prospera imports + hand-written).
- Prior import state: `/Users/treygoff/memorum/.memorum/import-state.json` tracks only **3** memories (Prospera-Policy, imported 2026-06-12). The 6/12 bulk import aborted on a chunk-index corruption bug (`UNIQUE constraint failed: memory_chunks.chunk_id`); see `docs/2026-06-12-for-codex-import-repair-supersede-index-corruption.md`. Today's commits (`43b9270` chunk-id fix) target that class of bug — this run also validates the fix at real scale.

---

## Pre-import corpus analysis

### Claude memory lives in a shared backing store reached via per-profile symlinks
Trey runs **three** Claude profiles, each with its own `projects/` dir whose `<encoded>/memory` entries are **symlinks** into one shared backing store `~/.claude-shared/memory/` (40 project dirs, **388** real memory `.md` files excluding `MEMORY.md` indexes).

Per-root reachability (what each `projects/` root can see through its symlinks):

| Root | Shared projects reached | Files reached |
|------|------------------------|---------------|
| `~/.claude/projects` | 32 / 40 | 318 |
| `~/.claude-personal/projects` | 13 / 40 | 160 |
| `~/.claude-work/projects` | 24 / 40 | 313 |
| **Union of all three** | **40 / 40** | **388** |

The importer's `claude.rs` walker is recursive with `follow_links(true)` and keys each memory by its path **relative to the root** (`claude:<encoded>/memory/<topic>.md`). Because the encoded project dir name is identical across profiles, the **same source_key is produced no matter which root reaches it** — so the import-state file dedups overlaps automatically. That makes "run once per profile root" safe and idempotent.

### FINDING 1 (ergonomics / correctness, HIGH) — default discovery silently under-covers on multi-profile setups
Discovery precedence (`discovery.rs`): `--from-claude` flag → `CLAUDE_CONFIG_DIR/projects` → `~/.claude/settings.json autoMemoryDirectory` → default `~/.claude/projects`.

On this machine `CLAUDE_CONFIG_DIR=~/.claude-personal`, so a bare `memoryd import` discovers only **13/40 projects (160/388 files)** — it silently misses **more than half** the corpus, with no warning that other profile roots exist. A user who runs the obvious `memoryd import` and sees "160 imported" has no signal that 228 memories were never considered. The importer treats the single resolved root as the whole world.

**Improvement ideas:**
- Detect sibling `~/.claude*/projects` dirs (and `~/.claude-shared/memory`) and either import their union by default or warn: "found 3 Claude profile roots; only importing `<X>`; pass `--from-claude` or `--all-profiles` to include the rest."
- Accept `--from-claude` multiple times, or add `--all-claude-profiles`, so one invocation covers the union.
- Consider discovering the shared backing store (`~/.claude-shared/memory`) directly as a first-class source, since it is the actual ground truth and is profile-independent. (Caveat: its layout is flat `<encoded>/*.md` with no `/memory/` infix, so source_keys would differ from the per-profile scheme and would not dedup against profile-rooted imports — needs a normalization rule.)

### Codex corpus
- `~/.codex/memories/MEMORY.md`: **13** `# Task Group:` blocks → 13 memories.
- `~/.codex/memories/extensions/ad_hoc/notes/`: **1** note.
- `rollout_summaries/`: 130 files — attached as evidence refs, not imported as memories (per design).
- `CODEX_HOME` unset → default `~/.codex/memories/` resolves correctly.

### Dream-scratch noise (minor)
Both `~/.claude/projects` (410 dirs) and `~/.claude-personal/projects` (348 dirs) are dominated by `memoryd-dream-scratch-run-*` project dirs. Their `memory/` subdirs are **empty**, so they contribute no candidates — but the recursive walker still descends all of them. Not a correctness problem; a walk-time/noise consideration on a machine that dreams a lot.

---

## Dry-run findings (`~/.claude/projects`, zero side effects)

Plan summary: **parsed 446, would-write 427, skipped-by-prompt 19, parse errors 3, unresolved back-edges 93.** (318 source files → 446 candidates after multi-section dossier decomposition.)

### FINDING 2 (correctness, HIGH) — strict YAML frontmatter silently drops real memories
3 Claude-authored memory files fail `serde_yaml` parsing and are dropped (logged, not aborted). All three are the same class: **unquoted/partially-quoted scalar values** that Claude's own auto-memory system writes and reads fine, but strict YAML rejects:

- `feedback-agency-disagree-and-build.md` — `name:` value contains `: ` (`...agency: disagree...`) → YAML reads it as a nested mapping. *"mapping values are not allowed in this context."*
- `feedback-agent-browser-react-click.md` — `description:` value begins with a backtick (`` `agent-browser... ``). Backtick is a **reserved indicator** in YAML and can't start a plain scalar. *"found character that cannot start any token."*
- `feedback_shape_of_policy_altitude.md` — `name:` value is partially quoted (`"Shape of policy" doc altitude...`) — opens a quote, then trails unquoted text. *"did not find expected key."*

These are valuable feedback memories (one is literally Trey's "agency is a feature" directive) and they vanish with only a stderr line. The body — the valuable part — is fully recoverable regardless of frontmatter validity.

**Improvement ideas:**
- On strict-YAML failure, fall back to a **lenient line-scan** that pulls `name`/`description`/`type` as raw strings and imports the body, rather than dropping the whole memory. Worst case: import with empty frontmatter hint + the body.
- Surface a parse-error **count in the run summary** (right now it's only in the JSON report's `parse_errors[]` and inline stderr; easy to miss in a 446-line run).
- Upstream note: this also exposes a bug in the **Claude-side memory writer** (these files carry `originSessionId`, so Claude wrote them) — it emits unquoted YAML values containing `:`, leading `` ` ``, and embedded `"`. Memorum's importer is just the first thing strict enough to notice.

### FINDING 3 (ergonomics, MEDIUM) — non-interactive runs silently skip non-git-cwd memories
19 memories (across 6 distinct non-git cwds) get `prompted_skip` because the run is non-interactive and `--non-git-cwd-default` was omitted. The 6 cwds:
`/Users/treygoff`, `/Users/treygoff/Code`, `/Users/treygoff//config/opencode`, `~/Code/b4a-plan-site`, `~/Code/claude-space`, `~/Code/sjb-site`.

Two of these (`~` and `~/Code`) are genuinely user-global. For this import I'll pass `--non-git-cwd-default me` — it captures all 19 with **no filesystem side effects**. Note `generate` would be actively harmful here: it would write a `.memory-project.yaml` into `~/` and `~/Code` themselves.

**Improvement idea:** when running non-interactively with the default (skip), the importer should at minimum print a one-line warning to stderr ("19 memories skipped: non-git cwd; re-run with --non-git-cwd-default me|generate to include them") so a scripted caller notices. Today you only learn this by reading the JSON report's counters.

### FINDING 4 (cosmetic / data quality, LOW) — hyphen-decode artifact for dotfile dirs
`~/.config/opencode`'s Claude-encoded dir `-Users-treygoff--config-opencode` decodes to `/Users/treygoff//config/opencode` (double slash, lost dot) because the `.` was encoded as a second `-` and the decoder maps every `-` to `/`. Cosmetic here (the cwd is non-git and goes to `me` anyway), but the decoder doesn't recover leading-dot path segments. The `resolve_existing_path` fallback can't fix it because `//config` never `stat`s.

---

## Run log

Ran the import across all three Claude profile roots + Codex, sequentially, each gated by a `memoryd doctor` health check (halt-on-unhealthy). Flags on every run: `--non-git-cwd-default me --repo /Users/treygoff/memorum --quiet --report <file>`. **No index corruption** — the 6/12 chunk-id class of failure did not recur; store stayed `healthy: true` after every step.

### FINDING 5 (CLI consistency, LOW) — `doctor` and `status`/`search`/`review` disagree on `--socket`
My first orchestration run aborted on what looked like an unhealthy store — but the import had actually succeeded (exit 0). The false alarm was `memoryd doctor --socket <path>` erroring with *"unexpected argument '--socket'"*. `doctor` takes `--repo`/`--runtime` (it inspects the substrate directly), while `status`, `search`, and `review queue` take `--socket` (they go through the daemon). Same binary, neighboring subcommands, different connection model and flags, no shared `--socket` that's simply ignored where irrelevant. Easy footgun when scripting a health-gated loop. *(This was my bug, not Memorum's — but the inconsistency is the reason the bug was easy to write.)*

### Per-step results

| Step (root) | parsed | new | me-cand | quar | dedup | idem-skip | privacy-refused |
|---|---|---|---|---|---|---|---|
| `~/.claude/projects` | 446 | 221 | 19 | 3 | 198 | 0 | 5 |
| `~/.claude-personal/projects` | 249 | 1 | 2 | 0 | 0 | 245 | 1* |
| `~/.claude-work/projects` | 398 | 68 | 14 | 1 | 0 | 311 | 4* |
| `~/.codex/memories` | 14 | 4 | 9 | 0 | 0 | 0 | 1 |

\* The Claude privacy refusals in personal/work runs are **re-encounters** of the same 5 files the main run already refused (overlapping roots re-attempt refused writes since refusals aren't recorded in the state file). The `skipped_idempotent` counts confirm cross-root dedup works: once the main run wrote a memory, the other roots skipped it by source_key.

### Net result (store ground truth, de-duplicated)

- **342 memories written** total: 294 new (project scope) + 44 me-scope candidates + 4 governance-quarantined.
- **198 deduped** against the surviving 6/12 import (daemon content-hash detection re-established idempotency even though the state file only carried 3 entries — the documented resilience path, confirmed live).
- **543 entries** now in `import-state.json` (530 claude-code + 13 codex).
- **6 unique privacy refusals** (5 Claude files + 1 Codex Task Group) — Stream D blocked them. Sources: `llm-council/project_chamber_v1_shipped` (#what-landed), `AZC/project_coalition_atlas_refresh_pipeline`, `AZC/project_national_donor_master_v2`, `SEZ-TPRI/project_jesse_von_stein_cos`, `prospera-radar-build/project_monitor_restructure_gemini_plan`, Codex `Granola notes export` task group. These hold donor/contact/PII-shaped content; the refusals look correct.
- **3 Claude files dropped** (malformed YAML, FINDING 2) — the only true data loss.
- Final `memoryd doctor`: `healthy: true`, one benign finding (`embedding_backlog`: 6 jobs draining — expected post-bulk-import).

### Recall verification
- `memoryd search "delegate droid alias reasoning"` → top hit is the imported Codex Task Group (`mem_…_000528`). ✓
- `memoryd search "AZC state pager persuasion craft"` → 27 hits across imported AZC project memories. ✓ Full-body `memory_get` available per snippet guidance.
- Project-scope recall works immediately. **Me-scope did not** (see FINDING 6).

### FINDING 6 (ergonomics / expectations, MEDIUM) — me-scope imports land as encrypted review-queue candidates, invisible to recall until approved
The 44 `--non-git-cwd-default me` memories were written to the **encrypted-at-rest tier** (`encrypted/me/knowledge/mem_*.md`) as **candidates** under `me-strict@v1`, and enqueued for review (`memoryd review queue` shows 45 "candidate requires confirmation" + 4 "governance quarantine" = 49 items). A content search for one of them (`fusion council…`) returns **0 hits** — candidates aren't in active recall until promoted via `memoryd review approve` / Reality Check.

This is defensible governance (personal/global facts get stricter handling + encryption), but the import UX doesn't telegraph it: a user who runs `--non-git-cwd-default me` to "include everything" gets 44 memories that are imported-but-dormant, with no summary line saying "44 memories queued for review; approve them to activate." The only way to discover it is to notice me-scope searches come back empty and then think to check the review queue. The final import summary should call out candidate/quarantine counts and the next action to activate them.

### FINDING 7 (reporting completeness, LOW) — report enumerates refusals/dedups/parse-errors but not quarantined or me-scope writes
`parse_errors[]`, `refusals[]`, and `dedups[]` are listed by source_key in the JSON report, but `written_candidate` (me-scope) and `quarantined` memories are only counters — there's no list of *which* sources they were. So you can't map the 44 me-scope or 4 quarantined back to their origin files from the report alone; you have to cross-reference the review queue by id. Adding `candidates[]` and `quarantined[]` arrays (source_key → memory_id) would close the loop.

---

## Summary of findings (for the importer backlog)

| # | Severity | Finding | Suggested fix |
|---|----------|---------|---------------|
| 1 | HIGH | Default discovery under-covers on multi-profile setups (here: 13/40 projects via `CLAUDE_CONFIG_DIR`; full corpus needs all 3 roots) | Auto-detect sibling `~/.claude*/projects`; warn or `--all-claude-profiles`; or treat `~/.claude-shared/memory` as a first-class source |
| 2 | HIGH | Strict YAML frontmatter silently drops real memories (3 lost) | Lenient line-scan fallback for `name`/`description`/`type`; import body regardless; count parse errors in summary |
| 3 | MED | Non-interactive runs silently skip non-git-cwd memories | stderr warning naming the skip count + the flag to include them |
| 4 | LOW | Hyphen-decode artifact for dotfile dirs (`//config/opencode`) | Recover leading-dot segments in the encoded→path decoder |
| 5 | LOW | `doctor` rejects `--socket` while sibling subcommands require it | Accept-and-ignore `--socket` on `doctor`, or share one connection-flag convention |
| 6 | MED | me-scope imports are encrypted review-queue candidates, invisible to recall until approved, with no UX signal | Final summary should report candidate/quarantine counts + the activation step |
| 7 | LOW | Report doesn't enumerate which sources became me-scope candidates or were quarantined | Add `candidates[]` / `quarantined[]` (source_key → memory_id) to the report |

## What's NOT imported (and what to do about it)

1. **3 malformed-YAML files** (FINDING 2) — recoverable by quoting the offending frontmatter value in each source file, then re-running import (which would pick up the new content hash). The fix is one line per file:
   - `pactact-site/.../feedback-agency-disagree-and-build.md` — quote the `name:` value (it contains `: `).
   - `pactact-site/.../feedback-agent-browser-react-click.md` — quote the `description:` value (starts with `` ` ``).
   - `SEZ-TPRI/.../feedback_shape_of_policy_altitude.md` — quote the `name:` value (embedded `"`).
2. **6 privacy-refused** memories — correctly blocked; no action unless Trey wants to hand-review and force specific ones.
3. **44 me-scope + 4 quarantined candidates** — imported but dormant in the review queue; run `memoryd review queue` / `approve` (or the Reality Check ritual) to activate the ones worth keeping.
