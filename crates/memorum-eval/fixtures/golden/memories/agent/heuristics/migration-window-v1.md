---
schema_version: 1
id: mem_20251015_e6e76b351c791fc0_000001
type: heuristic
scope: agent
summary: "Heuristic: prefer big-bang migrations during maintenance windows for simplicity."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: superseded
created_at: 2025-10-15T10:00:00Z
updated_at: 2026-01-18T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - heuristic
  - migration
  - superseded
superseded_by:
  - mem_20260118_d51d7a9faf673f3c_000001
---
HEURISTIC (superseded): big-bang migrations in a maintenance window are simplest. Superseded after the Atlas ledger saga proved lock duration makes big-bang risky at scale. Do not apply to large tables.
