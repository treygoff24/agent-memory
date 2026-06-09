---
schema_version: 1
id: mem_20251204_91610cfca2fa4b5e_000001
type: playbook
scope: project
summary: "How-to: pass an Idempotency-Key on every money-moving request; the gateway dedupes on it for 24 hours so retries never double-charge."
confidence: 0.85
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-04T10:00:00Z
updated_at: 2025-12-04T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - playbook
  - idempotency
  - payments
  - retry
---
HOW-TO: every money-moving request carries an Idempotency-Key header; the gateway caches the result keyed on it for 24h, so a retried request returns the original result instead of charging again. (Restates the idempotency invariant as an operational note — near-duplicate of invariants-idempotency.md.)
