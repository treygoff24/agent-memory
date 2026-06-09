---
schema_version: 1
id: mem_20251215_276a0acbede92b39_000001
type: decision
scope: project
summary: "Final executed plan: shadow-buffered dual-write to partitioned ledger, batched backfill off-peak, read-flip behind a flag, rollback by flipping the flag back."
confidence: 0.95
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-15T13:00:00Z
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
  - dual-write
  - rollback
entities:
  - id: ent_ledger
    label: ledger table
    aliases:
      - ledger
supersedes:
  - mem_20251208_eefe78aaa2ca1834_000001
---
DECISION (current, executed Dec 2025): partition the ledger table; dual-write through a shadow buffer to absorb the latency hit; backfill in 50k-row batches during off-peak; flip reads behind the `ledger_partitioned` flag; rollback = flip the flag back. This is the plan we actually ran. Recall THIS one for ledger migration questions.
