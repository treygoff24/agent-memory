---
schema_version: 1
id: mem_20251030_8453647158aa04d4_000001
type: postmortem
scope: agent
summary: "Postmortem: Atlas backfill double-count near-miss — dual-write overlap wasn't deduped; caught in staging. Lesson: backfills must dedupe against concurrent writes."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-10-30T10:00:00Z
updated_at: 2025-10-30T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - postmortem
  - migration
  - backfill
  - atlas
---
POSTMORTEM (near-miss): during the Atlas ledger backfill, rows written by the dual-write during the backfill window were counted twice. Caught in staging by the reconciliation check. Lesson: any backfill running alongside live writes must dedupe on a stable key. Timeline, contributing factors, and the reconciliation guard are documented here.
