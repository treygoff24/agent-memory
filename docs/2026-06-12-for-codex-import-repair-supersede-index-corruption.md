# For Codex: import repair-supersede corrupts the chunk index (live repro preserved)

Context: today's onboarding fix wave (see `docs/reviews/2026-06-12-agent-onboarding-roleplay-audit.md`, findings 18–19) made two import changes that surfaced a substrate bug at real scale: (1) discovery now follows symlinked memory dirs, growing the live Claude corpus from 3 candidates to 228; (2) import reruns now repair mis-bucketed memories via `memory_supersede` (RepairBucket path in `crates/memoryd/src/import/pipeline.rs`).

## What happened

Bulk import of 228 candidates on Trey's machine: 202 written cleanly, then a deterministic abort on `claude:-Users-treygoff-Library-CloudStorage-Dropbox-Prospera-Policy/memory/trey-career-facts.md` — an existing memory whose bucket changed, so it took the RepairBucket → `memory_supersede` path. The supersede failed with `write failed: index failed after commit (retryable=true)`.

It is not retryable in practice: the commit half succeeded, so the events log now carries duplicate chunk writes. Every subsequent attempt — including `memoryd doctor --reindex` — fails with `UNIQUE constraint failed: memory_chunks.chunk_id` while replaying the log. The derived index cannot be rebuilt from canonical events, and recall returns zero hits on the store.

## Why this looks like your 2026-06-10 note

Same shape as `docs/2026-06-10-for-substrate-owner-supersession-fk-bulk-reindex.md`: supersession plus index maintenance diverging from the events log under load. The new data point is a clean, deterministic, single-memory repro and a store you can poke at.

## Repro state (preserved, do not assume it stays — Trey may wipe)

- Store: `~/memorum` (post-purge fresh repo from today, ~205 memories, corrupted index at `~/memorum/.memoryd/index.sqlite`).
- Trigger command: `memoryd import --harness claude --socket ~/memorum/.memoryd/memoryd.sock` — aborts on the same memory every run, "after 0 memories had already been written" on re-runs (idempotent skips work; only the repair-supersede fails).
- `memoryd doctor` → `operator repair required: UNIQUE constraint failed: memory_chunks.chunk_id`; `doctor --reindex` → same constraint from replay.

## What would close it

Supersede (and any write) must be atomic across commit + index, or replay must tolerate/dedupe the duplicate chunk events it can now produce. The reindex path failing on its own canonical log is the part that turns a transient write bug into an unrecoverable store.

— Claude, 2026-06-12
