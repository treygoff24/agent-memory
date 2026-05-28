# `memoryd import`: backfilling prior harness memory into Memorum

The importer is a non-destructive, idempotent backfill tool that copies your existing Claude Code and OpenAI Codex CLI memories into Memorum. Run it once on a new machine and Memorum starts up with everything you've already taught those tools. Run it again whenever you want — it skips sources whose content hasn't changed.

## Quickstart

```bash
# The init wizard offers the import as part of first-run setup:
memoryd init

# Or run the import directly against an already-initialized repo:
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET"
```

## What gets imported

| Source | What lands in Memorum |
|--------|----------------------|
| Claude Code `~/.claude/projects/<encoded>/memory/<topic>.md` | One memory per single-fact file, or one memory per substantive `##` section in a multi-section dossier |
| Claude Code `user_profile.md` | One memory |
| Codex `~/.codex/memories/MEMORY.md` | One memory per `# Task Group:` block |
| Codex `~/.codex/memories/extensions/ad_hoc/notes/*.md` | One memory per note |

## What's skipped (and why)

- **`CLAUDE.md`, `AGENTS.md`** — user-authored instructions, not learned memory. Memorum doesn't touch them.
- **Claude `MEMORY.md`** — index file; the importer reads the topic files it points to instead.
- **Codex `raw_memories.md`, `memory_summary.md`, `skills/`** — intermediate or orthogonal to the durable memory layer.
- **Codex `rollout_summaries/`** — these are attached as `evidence` refs on the parent Task Group memory, not imported as separate memories.
- **Subagent memory (`.claude/agent-memory/`), Claude `rules/*.md`** — fringe surfaces; can be added in v2 if there's demand.

## Imported memories vs. hand-written

Imported memories carry a `confidence` of **0.7**. Hand-written memories default to **0.85**. This means:

- Imports stay above the Reality Check review threshold (0.6), so they don't flood the review queue on first import.
- Hand-written memories that you've edited later still rank higher in entity-overlap recall.

The lower confidence acknowledges that imported content was vetted by your prior harness's standards, not Memorum's stricter governance checks.

## Re-run semantics asymmetry

This catches everyone the first time:

| Where you edit | Re-import behavior |
|----------------|-------------------|
| The **source file** (e.g. `~/.claude/projects/x/memory/y.md`) | Re-import sees the new content hash and supersedes the existing Memorum memory. |
| The **Memorum copy** (e.g. `$MEMORUM_REPO/projects/proj_xxx/decisions/mem_yyy.md`) | Re-import preserves your edit. The content hash check is against the source, not the Memorum copy. |

Translation: edit the source if you want the change to propagate via re-import; edit the Memorum copy if you want to keep your local refinement.

## Rollback options

There's no `memoryd import --undo` in v1. To remove imported memories:

- **Individual**: `memoryd forget <id>` for each id from the import report.
- **Bulk find**: `memoryd search "source.harness=claude-code"` or `"source.harness=codex"` to enumerate imports for bulk action.
- **Nuclear**: delete `$MEMORUM_REPO/.memorum/import-state.json` and the affected memory files, then re-init.

Bulk-undo as a first-class command is a v2 feature.

## Dashboard limitations

The v1 dashboard (`memoryd web enable`) doesn't have a "filter by import source" view. Imported memories show up in the global list with `source.kind = import` and `source.harness = claude-code | codex`. v2 will add filtering.

## Post-import dream cycle

After a large import, the first 1-2 Stream F dream cycles may produce noisier candidate output as the system absorbs the new entity space. This self-corrects after a couple of runs. If `memoryd doctor` reports a flood of low-confidence dream candidates right after import, that's expected; don't act on the noise yet.

## Locked decisions (one-line summaries)

| Question | Answer |
|----------|--------|
| Granularity | Claude: adaptive (single-fact vs. 3+ substantive `##` sections → decompose); Codex: 1 Task Group = 1 memory |
| Wiki-links | Two-pass topological sort; cyclic back-edges stay as inert `[[name]]` text |
| Write primitive | `memory_write` for everything (including Codex ad-hoc notes — `memory_note` was overridden to preserve provenance) |
| Codex rollout summaries | Raw `file://` refs in `evidence[]` on the parent Task Group |
| Non-git cwds | Prompt per unique cwd: generate `.memory-project.yaml`, drop to `me` scope, or skip |
| Entity extraction | Source-provided only (Codex `### keywords`, Claude frontmatter `name`) |
| State file | `$MEMORUM_REPO/.memorum/import-state.json` |
| Conflict UX | Skip and log to import report |
| Throughput | Sequential, accept seconds-per-write |
| Re-import | Auto-supersede on content-hash change |
| First-run UX | Interactive `memoryd init` wizard offers the import |

## State file format

`$MEMORUM_REPO/.memorum/import-state.json`:

```json
{
  "schema_version": 1,
  "imports": {
    "claude:projects/.../memory/topic.md": {
      "memory_id": "mem_20260527_...",
      "content_hash": "sha256:...",
      "imported_at": "2026-05-27T22:33:00Z",
      "harness": "claude-code",
      "source_path_at_import": "/Users/u/.claude/projects/.../memory/topic.md",
      "supersession_chain": []
    }
  }
}
```

The state file is a **performance optimization**, not the load-bearing correctness mechanism. The daemon's duplicate-detection re-establishes idempotency on any re-run that finds the state file truncated.

A sibling lock file at `<state-file>.lock` plus a `import.pid` file prevents two concurrent imports from racing each other. A second invocation that can't acquire the lock within 5s fails with `AnotherImportInProgress { pid: <holding-pid> }`.

## Conflict / refusal report

The importer never aborts on a refused write. Each refusal appends to the report with:

- `source_key` — the harness-relative key (e.g. `claude:projects/.../memory/topic.md`).
- `harness` — `claude-code` or `codex`.
- `reason` — `privacy`, `contradiction`, `tombstone`, `grounding`, `policy`, or `other`.
- `suggested_next_action` — when available.

In verbose mode (default), refused writes also appear inline in the progress stream:

```
[47/500] REFUSED (privacy): claude:projects/.../memory/feedback_X.md
```

## CLI flags

```
memoryd import [--harness all|claude|codex] [--dry-run]
               [--from-claude <path>] [--from-codex <path>]
               [--report <file.json>] [--quiet]
               [--socket <path>] [--repo <path>]
```

- `--dry-run` — plan and report what would be written; issue zero daemon calls.
- `--report <file.json>` — write structured JSON for diff-friendly inspection.
- `--harness all|claude|codex` — restrict the run.
- `--from-claude <path>` / `--from-codex <path>` — independent overrides; either can be supplied without the other.

## Troubleshooting

See `docs/troubleshooting.md` for: `AnotherImportInProgress`, all-`SkipUnchanged`, "harness not detected", and other first-run failures.
