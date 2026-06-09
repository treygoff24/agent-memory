---
schema_version: 1
id: mem_20251120_a43df63066cac661_000001
type: decision
scope: project
summary: "Current auth decision: stateless JWT access tokens (15-min TTL) + rotating refresh tokens in an httpOnly cookie, signed by Orbit's KMS key."
confidence: 0.95
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-20T10:00:00Z
updated_at: 2025-11-20T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - auth
  - jwt
  - refresh-token
  - kms
entities:
  - id: ent_auth
    label: auth flow
    aliases:
      - authentication
  - id: ent_kms
    label: KMS signing key
supersedes:
  - mem_20251110_e7ed1dcdf272e4d9_000005
---
DECISION (current): stateless JWT access tokens, 15-minute TTL, signed by the Orbit KMS key; refresh tokens rotate and live in an httpOnly, SameSite=strict cookie. No server-side session store. This is the auth model we run. Marco approved. Recall THIS for auth-flow questions.
