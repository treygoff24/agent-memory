---
schema_version: 1
id: mem_20251025_1b7b619ed4faa9b8_000002
type: anti-pattern
scope: agent
summary: "Anti-pattern: using floating-point for currency amounts; rounding errors accumulate into real financial discrepancies."
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
  - money
  - float
---
ANTI-PATTERN: floats for money. IEEE rounding silently corrupts balances. Use integer minor units. (Atlas banned this outright.)
