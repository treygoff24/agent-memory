---
schema_version: 1
id: mem_20251020_fa6263bc3288eab6_000005
type: pattern
scope: agent
summary: "Pattern: assess blast radius and rollback path before correctness when reviewing infra/data changes."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-10-20T10:00:00Z
updated_at: 2025-10-20T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - pattern
  - review
  - blast-radius
---
PATTERN: for infra and data-shape changes, the first review questions are blast radius and rollback path, not code style.
