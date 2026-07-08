# Agent import guide: driving `memoryd import` non-interactively

This guide is written to you — the AI agent — not the user. Most users back up their prior memory by asking an agent, not by running the CLI themselves. Your job is to run one command, read one summary, and report cleanly. Every flag here is part of the `memoryd import` contract; do not invent flags outside this list.

Companion skill: `skills/using-memorum/SKILL.md` (the tight operating loop). Companion docs: `docs/importer.md` (design rationale, granularity rules, re-import semantics), `docs/troubleshooting.md` (first-run failures), `docs/agent-onboarding.md` (the full `memoryd init` install flow that wraps import).

## What import does

`memoryd import` is a non-destructive, idempotent backfill. It copies the memory a user has already accumulated in Claude Code and Codex CLI into Memorum, so a Memorum store starts up knowing what those harnesses learned. Source files are never modified. Re-runs skip unchanged sources by content hash, so running it more than once is safe and cheap.

## Paths

Resolve these before running. Use the user's exported values when present; otherwise the defaults apply.

```bash
MEMORUM_REPO="${MEMORUM_REPO:-$HOME/memorum}"
MEMORUM_SOCKET="${MEMORUM_SOCKET:-$MEMORUM_REPO/.memoryd/memoryd.sock}"
MEMORUM_RUNTIME="${MEMORUM_RUNTIME:-$MEMORUM_REPO/.memoryd}"
```

The daemon must be running before you import — import writes go through it. Confirm first:

```bash
memoryd status --socket "$MEMORUM_SOCKET"   # daemon reachable?
memoryd doctor --repo "$MEMORUM_REPO"        # substrate healthy?
```

## The one command

```bash
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET"
```

That is the default invocation, and for the overwhelming majority of users it is the only one you need. Its default behavior:

| Default              | Behavior                                                                                                                                                                                               |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Claude source        | Auto-detects and imports the **union of all** `~/.claude*/projects` roots, deduplicated. Covers multi-profile setups with no extra flag.                                                               |
| Codex source         | Imports `~/.codex/memories` (Task Group blocks → memories; ad-hoc notes → memories; rollout summaries → evidence refs).                                                                                |
| Non-git-cwd memories | Given a **project namespace derived from the cwd path** (`--non-git-cwd-default project`), no `.memory-project.yaml` written. Saved and **active/recall-visible immediately**, never silently skipped. |
| Frontmatter          | Malformed YAML is recovered leniently; the body always imports.                                                                                                                                        |
| Re-run               | Unchanged sources skipped by content hash.                                                                                                                                                             |

### Why no `--from-claude` by default

Earlier behavior resolved a single Claude root (e.g. via `CLAUDE_CONFIG_DIR`) and treated it as the whole world, silently under-covering users with more than one profile. The default now imports the **union** of every `~/.claude*/projects` root it finds. Because the encoded project-dir name is identical across profiles, the same source key is produced no matter which root reaches a file — so overlaps dedup automatically and the union is safe.

Pass `--from-claude` only to pin exact roots. When given, auto-detection is skipped entirely.

```bash
# Pin specific roots (repeatable); auto-detect is OFF
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET" \
  --from-claude ~/.claude/projects \
  --from-claude ~/.claude-work/projects
```

## Full flag table

```
memoryd import [--repo <PATH>] [--socket <PATH>]
               [--from-claude <PATH>]... [--from-codex <PATH>]
               [--harness <all|claude|codex>]
               [--non-git-cwd-default <project|me|generate|skip>]
               [--dry-run]
               [--report <FILE.json>]
               [--quiet]
```

| Flag                                                  | Default                                    | Meaning                                                                         |
| ----------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------- |
| `--repo <PATH>`                                       | `$MEMORUM_REPO` or `~/memorum`             | Canonical Memorum repo root.                                                    |
| `--socket <PATH>`                                     | `<repo>/.memoryd/memoryd.sock`             | Daemon socket; import writes go through it.                                     |
| `--from-claude <PATH>`                                | auto-detect union of `~/.claude*/projects` | Pin an exact Claude root. **Repeatable.** When present, auto-detect is skipped. |
| `--from-codex <PATH>`                                 | `~/.codex/memories`                        | Override the Codex memory root.                                                 |
| `--harness <all\|claude\|codex>`                      | `all`                                      | Restrict the run to one harness.                                                |
| `--non-git-cwd-default <project\|me\|generate\|skip>` | `project`                                  | Placement for memories whose cwd is not a git checkout. See below.              |
| `--dry-run`                                           | off                                        | Plan and report what would be written; issue zero daemon calls.                 |
| `--report <FILE.json>`                                | none                                       | Write the full reconciliation report as JSON.                                   |
| `--quiet`                                             | off                                        | Suppress per-item progress lines; still prints the final summary.               |

### `--non-git-cwd-default`

Some prior memories are tied to a directory that is not a git repo (a user's home, a scratch dir). The default `project` mints a deterministic project namespace from the cwd path so the memories are saved and land active — no filesystem side effects, nothing dropped, nothing parked in a review queue.

- `project` (default) — derive a `proj_<name>-<hash>` namespace from the cwd path. No file written. Memories land **active and recall-visible** under the default policy (not me-strict), so they're usable immediately. The privacy classifier still runs per write, so genuinely sensitive content is still encrypted at rest.
- `me` — assign to `me` scope instead. Note: me-scope runs through me-strict governance, so these land as **encrypted review-queue candidates** (not in active recall until approved). Use only when the user wants that stricter posture.
- `generate` — write a `.memory-project.yaml` into each such cwd to mint a persistent project bucket. **This is an opt-in side effect that writes into the user's directories** (it would drop a file into `~` if a memory's cwd was the home directory). Choose it only when the user wants the namespace to persist for future live sessions in that directory.
- `skip` — drop them. Use only if the user explicitly does not want non-project memories.

Governance-quarantined items (contradictions caught at write time) are routed to the review queue regardless of placement — they always require explicit review.

## The reconciliation summary

Every run ends with a summary block. Quiet or verbose, the summary always prints.

```
Memorum import complete.
  imported-active:       294
  queued-for-review:       0
  privacy-blocked:         6
  frontmatter-recovered:   3
  dropped:                 1
next: memoryd search "<topic>" --socket /Users/u/memorum/.memoryd/memoryd.sock
```

| Field                   | Meaning                                                     | Your read                                                                                                                                                                  |
| ----------------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `imported-active`       | Written and recall-visible now.                             | The success number. Report it.                                                                                                                                             |
| `queued-for-review`     | Items the governance layer flagged for a human look.        | Nonzero for contradictions caught at write time, or memories explicitly routed to `me` scope. Inspect with `review queue`; accept individually with `review approve <id>`. |
| `privacy-blocked`       | Stream D refused these (PII/contacts/donor-shaped content). | **Expected. Not an error.** Listed in `refusals[]`. Do not retry or flag as failure.                                                                                       |
| `frontmatter-recovered` | Broken YAML; body imported anyway.                          | Fine. No action.                                                                                                                                                           |
| `dropped`               | Truly unreadable files.                                     | The only real data loss. Name them to the user (listed in the report).                                                                                                     |

The `next:` line gives the exact follow-up command to confirm a memory landed.

### Machine-readable report (`--report`)

```bash
memoryd import --repo "$MEMORUM_REPO" --socket "$MEMORUM_SOCKET" --report /tmp/import-report.json
```

The JSON report carries the same counters plus per-source detail. Shape:

```json
{
  "schema_version": 1,
  "summary": {
    "parsed": 446,
    "imported_active": 294,
    "queued_for_review": 0,
    "privacy_blocked": 6,
    "frontmatter_recovered": 3,
    "dropped": 1,
    "skipped_unchanged": 198
  },
  "refusals": [
    {
      "source_key": "claude:projects/.../memory/donor_master.md",
      "harness": "claude-code",
      "reason": "privacy",
      "suggested_next_action": null
    }
  ],
  "frontmatter_recovered": [{ "source_key": "claude:projects/.../memory/feedback_x.md", "memory_id": "mem_..." }],
  "dropped": [{ "source_key": "claude:projects/.../memory/corrupt.md", "reason": "unreadable" }],
  "next_action": "memoryd search \"<topic>\" --socket <sock>"
}
```

- `refusals[].reason` is one of `privacy`, `contradiction`, `tombstone`, `grounding`, `policy`, `other`.
- `summary.skipped_unchanged` is the idempotency count — sources unchanged since a prior import. High on re-runs; that's correct.

## Exit-code contract

```
0   success — even when some writes were refused, recovered, or skipped.
    Refusals and recoveries are reported, not failures.
≠0  hard failure only:
      - cannot reach the daemon
      - lock contention: AnotherImportInProgress { pid: <N> }
      - unreadable repo
```

A privacy refusal, a frontmatter recovery, a dropped file, an all-unchanged re-run — none of these are failures. The process exits 0 and the detail is in the summary and the report. Only treat a non-zero exit as a problem to handle.

`AnotherImportInProgress { pid: <N> }` means a second import couldn't acquire the lock within the timeout. Check whether that pid is alive (`kill -0 <N>`); if it's a hung run, see `docs/troubleshooting.md` for clearing the stale lock at `<repo>/.memorum/import-state.json.lock`.

## Verifying the import landed

```bash
# Project-scope recall is immediate
memoryd search "<a topic the user worked on>" --socket "$MEMORUM_SOCKET"

# Read a hit in full
memoryd get <id> --socket "$MEMORUM_SOCKET"

# Inspect the review queue (candidates + governance quarantine)
memoryd review queue --socket "$MEMORUM_SOCKET"
```

With defaults, every non-git-cwd memory lands in a derived project scope and is searchable right after import. Only memories explicitly routed to `me` scope (or governance-quarantined items) wait in the review queue and won't appear in search until approved.

## Related commands

| Command                                         | Flag convention                             | Use                                              |
| ----------------------------------------------- | ------------------------------------------- | ------------------------------------------------ |
| `memoryd status --socket <sock>`                | `--socket`                                  | Daemon reachable?                                |
| `memoryd doctor --repo <repo> [--runtime <rt>]` | `--repo`/`--runtime` (tolerates `--socket`) | Substrate health. Exits non-zero when unhealthy. |
| `memoryd search "<q>" --socket <sock>`          | `--socket`                                  | Recall over the store.                           |
| `memoryd get <id> --socket <sock>`              | `--socket`                                  | Full body of one memory.                         |
| `memoryd review queue --socket <sock>`          | `--socket`                                  | List candidates and quarantined items.           |
| `memoryd review approve <id> --socket <sock>`   | `--socket`                                  | Approve one queued candidate by id.              |
| `memoryd forget <id> --socket <sock>`           | `--socket`                                  | Remove one memory.                               |
| `memoryd export --socket <sock>`                | `--socket`                                  | Dump the store.                                  |

Note the split: `doctor` reads the substrate directly and is keyed on `--repo`/`--runtime`; the rest go through the daemon and are keyed on `--socket`. `doctor` now also tolerates `--socket` so a health-gated import loop can pass one consistent set of flags.

**Envelope and exit codes.** The daemon-backed covered commands (`status`, `search`, `get`, `write`, `write-note`, `supersede`, `forget`, `source`, `reveal`, `observe`) emit the v1 agent envelope — `{ok,data,meta.schema_version}` on stdout for success, `{ok:false,error,meta}` on stderr for failure — and follow the published exit dictionary (0 success, 65 invalid/refused, 66 not-found, 75 daemon-unreachable/transient, 77 client gate). `import` and `doctor` keep their own dictionaries (import: 0 even on soft refusals, per above; doctor: 0/1). The full contract is `docs/api/memoryd-cli-contract-v1.md`, and `memoryd schema --json` prints it. Governance writes never report a refusal as success: a refused `write`/`supersede`/`forget` is `ok:false` exit 65, while `candidate`/`quarantined` are `ok:true` exit 0 with a `data.status` and a "not yet active" warning.

## Troubleshooting quick reference

| Symptom                              | Likely cause                                    | Action                                                                |
| ------------------------------------ | ----------------------------------------------- | --------------------------------------------------------------------- |
| Non-zero exit, "connection refused"  | Daemon not running                              | Start it; `memoryd status` to confirm.                                |
| `AnotherImportInProgress { pid: N }` | Concurrent or hung import                       | `kill -0 N`; if dead, clear the lock (see `docs/troubleshooting.md`). |
| Everything reports skipped/unchanged | Corpus already imported                         | Correct on re-run. No action.                                         |
| `privacy-blocked` count > 0          | Sensitive content refused by Stream D           | Expected. Report the count; don't retry.                              |
| `dropped` count > 0                  | Unreadable source files                         | Real loss; name the files (in the report) to the user.                |
| a memory isn't in search             | Routed to `me` scope, or governance-quarantined | `memoryd review queue`, then `memoryd review approve <id>`.           |

For anything not covered here, `docs/troubleshooting.md` maps symptoms to fixes and `docs/importer.md` carries the design rationale and re-import semantics.
