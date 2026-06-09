---
schema_version: 1
id: mem_20251110_e7ed1dcdf272e4d9_000005
type: decision
scope: project
summary: "Initial auth decision: use server-side session cookies stored in Redis with sticky sessions."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: superseded
created_at: 2025-11-10T10:00:00Z
updated_at: 2025-11-20T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - auth
  - sessions
  - cookies
  - redis
  - superseded
entities:
  - id: ent_auth
    label: auth flow
    aliases:
      - authentication
superseded_by:
  - mem_20251120_a43df63066cac661_000001
---
DECISION (superseded): server-side session cookies in Redis, sticky sessions at the LB. Superseded after sticky sessions broke during rolling deploys and Marco flagged the Redis blast radius. Do not follow this plan.
