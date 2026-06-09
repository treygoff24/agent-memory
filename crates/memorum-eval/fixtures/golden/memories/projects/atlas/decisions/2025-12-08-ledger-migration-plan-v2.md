---
schema_version: 1
id: mem_20251208_eefe78aaa2ca1834_000001
type: decision
scope: project
summary: "Revised plan: dual-write to old and new ledger tables, backfill in batches, then flip reads — no maintenance window."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: superseded
created_at: 2025-12-08T13:00:00Z
updated_at: 2025-12-15T13:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - migration
  - ledger
  - database
  - superseded
  - dual-write
entities:
  - id: ent_ledger
    label: ledger table
    aliases:
      - ledger
supersedes:
  - mem_20251201_ad04f10fe704d9c9_000001
superseded_by:
  - mem_20251215_276a0acbede92b39_000001
---
DECISION (superseded): dual-write + batched backfill + read-flip, no downtime. Superseded by v3 after we found the dual-write doubled write latency under peak load; v3 adds a write-shadow buffer. Do not follow this plan.
