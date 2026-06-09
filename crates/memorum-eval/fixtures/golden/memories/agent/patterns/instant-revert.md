---
schema_version: 1
id: mem_20251117_a29790195966a358_000001
type: pattern
scope: agent
summary: "Pattern: ship only changes you can instantly revert in one action — a flag toggle or a redeploy of the prior release."
confidence: 0.85
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-17T10:00:00Z
updated_at: 2025-11-17T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - pattern
  - rollback
  - revert
---
PATTERN: don't ship what you can't instantly revert. One action — flag off, or redeploy prior tag. (Near-duplicate of the one-step-rollback pattern; recall should collapse these.)
