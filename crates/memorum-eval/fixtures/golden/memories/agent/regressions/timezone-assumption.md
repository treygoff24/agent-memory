---
schema_version: 1
id: mem_20251102_9210e51763122a66_000003
type: regression
scope: agent
summary: "Cross-cutting regression class: assuming a fixed timezone for scheduling produces wrong-time actions; always resolve the user's TZ explicitly."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-02T10:00:00Z
updated_at: 2025-11-02T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - regression
  - timezone
  - scheduling
---
REGRESSION CLASS: agents repeatedly assume a default timezone (often Pacific) for scheduling and produce wrong-time actions. Always resolve the user's actual timezone from memory before scheduling. (See the me/ timezone correction.)
