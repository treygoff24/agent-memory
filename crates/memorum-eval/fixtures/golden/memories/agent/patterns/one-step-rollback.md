---
schema_version: 1
id: mem_20251116_1665892b9f867649_000001
type: pattern
scope: agent
summary: "Pattern: every risky change needs a one-step rollback (flag flip or previous-tag redeploy)."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-16T10:00:00Z
updated_at: 2025-11-16T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - pattern
  - rollback
  - feature-flag
---
PATTERN: a risky change must have a single-action rollback — flip a flag or redeploy the last good tag. If rollback is multi-step, harden it before shipping.
