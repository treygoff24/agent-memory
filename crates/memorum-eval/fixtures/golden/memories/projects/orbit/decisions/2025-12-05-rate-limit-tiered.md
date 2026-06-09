---
schema_version: 1
id: mem_20251205_318c41ee89b47965_000002
type: decision
scope: project
summary: "Current rate-limit policy: per-account token-bucket on auth endpoints (not per-IP), with a separate stricter per-IP failed-login limit."
confidence: 0.92
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-05T10:00:00Z
updated_at: 2025-12-05T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - decision
  - rate-limit
  - auth
  - token-bucket
supersedes:
  - mem_20251122_f6531fd88a5ea343_000001
---
DECISION (current): rate-limit auth endpoints per-account via a token-bucket (handles shared-NAT corporate clients), PLUS a strict per-IP limit on FAILED logins only (credential-stuffing defense). Recall THIS for Orbit rate-limiting.
