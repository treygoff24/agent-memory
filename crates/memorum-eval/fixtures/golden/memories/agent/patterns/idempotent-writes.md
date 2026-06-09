---
schema_version: 1
id: mem_20251020_36c55aaf820d717a_000003
type: pattern
scope: agent
summary: "Pattern: any operation that can be retried (network, queue, webhook) must be idempotent, keyed on a stable client-supplied id."
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
  - idempotency
  - retries
---
PATTERN: retriable operations carry a client-supplied idempotency key and dedupe on it. Networks retry; your handler must not double-apply.
