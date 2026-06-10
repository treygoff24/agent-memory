# Peer note: supersession FK aborts bulk reindex (2026-06-10)

**From:** Claude, while implementing Task 4.2 (quality-metrics runner over the golden corpus).
**To:** whoever owns `memory-substrate` next.
**Severity:** latent correctness bug in bulk reindex. Not hit by the daemon's normal write path.

## What I found

Loading the golden corpus (`crates/memorum-eval/fixtures/golden/`, 101 real memory files) into a fresh `Substrate` via `Substrate::init` aborts with:

```
OperatorRepairRequired("index consistency: FOREIGN KEY constraint failed")
```

## Root cause

`memory-substrate::index::query::sync_supersession` writes each memory's `memory_supersession` edges with an **unguarded** insert:

```rust
// crates/memory-substrate/src/index/query.rs  (sync_supersession)
txn.execute(
    "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id) VALUES (?1, ?2)",
    params![memory_id, supersedes_id.as_str()],
)?;
```

The table has `FOREIGN KEY(supersedes_id) REFERENCES memories(id)` and `PRAGMA foreign_keys = ON`. During a **bulk reindex** (`Substrate::open` → reconcile `phase_6_index_consistency` → `reindex_stale_memories`), the tree is walked in **unsorted `walkdir` order**. When a supersessor is upserted before its supersedes-target's `memories` row exists, the FK trips and the whole reconcile aborts.

The daemon's _incremental_ write path never hits this — the supersede target is already indexed by the time the replacement is written — which is why no existing test caught it.

Notably, the v4 migration's own supersession bootstrap (`migrations.rs`) **already guards** the exact same insert with `WHERE EXISTS (SELECT 1 FROM memories WHERE id = ...)`. The per-write `sync_supersession` simply never got the same guard. That parity gap is the fix.

## Suggested fix (one statement, behavior-preserving)

```rust
txn.execute(
    "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id)
     SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM memories WHERE id = ?2)",
    params![memory_id, supersedes_id.as_str()],
)?;
```

This matches the migration's guard. It is behavior-preserving for the incremental path (target always present → guard never trips). For bulk reindex it drops only the edges whose target genuinely isn't indexed yet; a subsequent pass / the next reindex re-adds them once the target lands. (A fully complete fix would do a deferred second pass for supersession after all `memories` rows are inserted, but the guard alone unbreaks bulk import.)

I verified this fix resolves the corpus load in a throwaway worktree.

## Why I did not land it

`memory-substrate/src/index/query.rs` was dirty in the working tree with your in-flight Task 3.2 (contradiction similarity) work at the time. Rather than edit a file you were mid-flight on, I made the quality runner self-contained: it strips the `supersedes:` frontmatter edge list from its _staged_ copies of the corpus before indexing. That is behavior-preserving for the runner because the recall candidate-selection path excludes superseded tails by `status: superseded` (handled in `collect_recall_candidates`), never by the `memory_supersession` edge table. See `crates/memorum-eval/src/quality.rs::strip_supersedes_block` for the rationale in context.

If/when you apply the substrate guard, the runner's strip becomes belt-and-suspenders and could be removed — but it's harmless to leave.
