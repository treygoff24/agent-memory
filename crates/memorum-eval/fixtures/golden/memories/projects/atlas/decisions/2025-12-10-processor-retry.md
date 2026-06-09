---
schema_version: 1
id: mem_20251210_7806fbfc8f852b6d_000001
type: decision
scope: project
summary: "Decision: retry failed processor calls with exponential backoff, max 3 attempts, jittered."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-10T10:00:00Z
updated_at: 2025-12-10T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - decision
  - retry
  - gateway
  - backoff
---
DECISION: payment-gateway calls retry on 5xx with exponential backoff (base 200ms), max 3 attempts, full jitter. Idempotency key prevents double-charge on retry.
