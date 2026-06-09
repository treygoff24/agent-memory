---
schema_version: 1
id: mem_20251201_b6550e42d1d32eeb_000002
type: regression
scope: project
summary: "Regression: tokens issued by a node with skewed clock were rejected as 'not yet valid'; fixed by adding 30s leeway on the nbf claim."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-01T10:00:00Z
updated_at: 2025-12-01T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - regression
  - jwt
  - clock-skew
  - nbf
---
REGRESSION: a node with ~20s clock skew issued JWTs with a future `nbf`, and other nodes rejected them as not-yet-valid. Fix: 30s leeway when validating `nbf`/`exp`, plus chrony enforced on all nodes. Symptom looked like random auth failures.
