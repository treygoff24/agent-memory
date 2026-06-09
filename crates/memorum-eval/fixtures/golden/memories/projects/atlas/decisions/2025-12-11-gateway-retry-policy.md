---
schema_version: 1
id: mem_20251211_1d94c80988e12d09_000001
type: decision
scope: project
summary: "Decision: the gateway adapter retries processor 5xx up to three times with jittered exponential backoff starting at 200ms."
confidence: 0.85
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-11T10:00:00Z
updated_at: 2025-12-11T10:00:00Z
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
DECISION: gateway retries on processor 5xx — exponential backoff from 200ms, 3 attempts max, jitter on. (Near-duplicate of the Dec-10 processor-retry decision; same policy, restated. Recall should collapse these.)
