---
schema_version: 1
id: mem_20251220_b87253ae26efe467_000001
type: regression
scope: project
summary: Backfill double-counted rows whose ledger entry was written during the dual-write window; fixed by deduping on (entry_id) before insert.
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-20T11:00:00Z
updated_at: 2025-12-20T11:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - migration
  - ledger
  - regression
  - backfill
  - double-count
entities:
  - id: ent_ledger
    label: ledger table
    aliases:
      - ledger
related:
  - mem_20251215_276a0acbede92b39_000001
---
REGRESSION: rows written during the dual-write overlap got counted twice in the backfill, inflating one customer's balance. Root cause: backfill didn't dedupe against rows the dual-write already copied. Fix: dedupe on entry_id before insert; add a reconciliation check. Caught in staging, never hit prod.
