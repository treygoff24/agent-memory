---
schema_version: 1
id: mem_20251102_e77dc3c51249a9ef_000002
type: invariant
scope: project
summary: "Atlas invariant: every payment-mutation endpoint must accept and honor an idempotency key; retries must never double-charge."
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-02T10:10:00Z
updated_at: 2025-11-02T10:10:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - invariant
  - idempotency
  - payments
---
INVARIANT: every endpoint that moves money accepts an Idempotency-Key header and dedupes on it for 24h. A retried request must never double-charge. Enforced by a contract test in CI.
