---
schema_version: 1
id: mem_20251020_60a6259974540814_000002
type: pattern
scope: agent
summary: "Pattern: gate risky changes behind a feature flag with a flag-flip rollback path; never ship a change you can't disable in one step."
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
  - feature-flag
  - rollout
  - rollback
---
PATTERN: risky changes go behind a flag whose default-off is a one-step rollback. If you can't disable it instantly, it's not ready.
