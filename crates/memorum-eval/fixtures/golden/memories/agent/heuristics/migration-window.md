---
schema_version: 1
id: mem_20260118_d51d7a9faf673f3c_000001
type: heuristic
scope: agent
summary: "Heuristic: for large-table migrations, default to online dual-write + batched backfill behind a flag; reserve big-bang for small tables only."
confidence: 0.92
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-01-18T10:00:00Z
updated_at: 2026-01-18T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - heuristic
  - migration
  - dual-write
  - online
supersedes:
  - mem_20251015_e6e76b351c791fc0_000001
---
HEURISTIC (current): large-table migrations default to online dual-write + batched off-peak backfill, read-flip behind a flag, flag-flip rollback. Big-bang only for small/cold tables. Generalized from the Atlas ledger migration. Recall THIS one.
