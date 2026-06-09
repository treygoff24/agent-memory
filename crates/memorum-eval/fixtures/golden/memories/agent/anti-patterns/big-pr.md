---
schema_version: 1
id: mem_20251025_2b738aaaea4fa210_000003
type: anti-pattern
scope: agent
summary: "Anti-pattern: shipping one giant PR that bundles refactor + feature + migration; it's unreviewable and unrevertable."
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
  - pr-size
  - review
---
ANTI-PATTERN: the mega-PR mixing refactor, feature, and migration. Nobody can review it and you can't revert one piece. Split by reversibility boundary.
