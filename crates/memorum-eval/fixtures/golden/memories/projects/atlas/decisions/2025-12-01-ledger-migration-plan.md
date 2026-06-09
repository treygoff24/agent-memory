---
schema_version: 1
id: mem_20251201_ad04f10fe704d9c9_000001
type: decision
scope: project
summary: "Initial plan: migrate the Atlas ledger table to a partitioned scheme with a single big-bang cutover during a maintenance window."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: superseded
created_at: 2025-12-01T13:00:00Z
updated_at: 2025-12-08T13:00:00Z
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
entities:
  - id: ent_ledger
    label: ledger table
    aliases:
      - ledger
superseded_by:
  - mem_20251208_eefe78aaa2ca1834_000001
---
DECISION (superseded): big-bang cutover of the ledger table to monthly partitions during a Sunday maintenance window. Superseded after Lena flagged the lock duration would exceed the window. Do not follow this plan.
