---
schema_version: 1
id: mem_20251025_581143ebc1964900_000004
type: anti-pattern
scope: agent
summary: "Anti-pattern: relying on sticky sessions for correctness; they break on rolling deploys and autoscaling."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-10-25T10:00:00Z
updated_at: 2025-10-25T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - anti-pattern
  - sessions
  - sticky
  - deploys
---
ANTI-PATTERN: sticky sessions as a correctness mechanism. They break under rolling deploys and autoscale events. Prefer stateless tokens. (Orbit learned this.)
