---
schema_version: 1
id: mem_20251030_8b737d5eb76b2153_000003
type: postmortem
scope: agent
summary: "Postmortem: Quill's auto-retry masked a growing test race for weeks until the suite was effectively non-signal; root cause was shared test DB."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-10-30T10:00:00Z
updated_at: 2025-10-30T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - postmortem
  - ci
  - flaky
  - quill
---
POSTMORTEM: Quill's CI auto-retry hid a worsening test race for ~6 weeks; by the time anyone looked, a 'green' build meant little. Root cause: shared Postgres test DB with no isolation. Fix: per-test transaction rollback. Lesson: retries on flakes are debt, not a fix.
