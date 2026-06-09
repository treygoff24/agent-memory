---
schema_version: 1
id: mem_20260205_faca5b17f9886509_000001
type: heuristic
scope: agent
summary: "Heuristic: dual-write migrations are worth their complexity once a table is large enough that lock time exceeds an acceptable maintenance window."
confidence: 0.8
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-02-05T10:00:00Z
updated_at: 2026-02-05T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - heuristic
  - migration
  - dual-write
---
Heuristic: dual-write migrations are worth their complexity once a table is large enough that lock time exceeds an acceptable maintenance window.
