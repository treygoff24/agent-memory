---
schema_version: 1
id: mem_20251020_1981257f2f642c59_000001
type: pattern
scope: agent
summary: "Pattern: never patch a reported bug without a reliable reproduction first; the repro is the spec for the fix."
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
  - debugging
  - repro
---
PATTERN: a bug without a repro is a rumor. Get a deterministic repro before writing any fix — it becomes the regression test.
