---
schema_version: 1
id: mem_20251025_8cbf0ba5bf7e5ff1_000001
type: anti-pattern
scope: agent
summary: "Anti-pattern: auto-retrying failing tests to keep CI green; it masks real races and the flake rate compounds."
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
  - ci
  - flaky
  - retry
---
ANTI-PATTERN: retrying flaky tests to force green. It hides real concurrency bugs and the underlying flake rate grows until the suite is worthless. (See the Quill CI saga.)
