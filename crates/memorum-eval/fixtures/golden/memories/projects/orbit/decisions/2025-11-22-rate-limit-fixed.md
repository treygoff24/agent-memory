---
schema_version: 1
id: mem_20251122_f6531fd88a5ea343_000001
type: decision
scope: project
summary: "Initial rate-limit policy: a flat 100 req/min per IP on the auth endpoints."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: superseded
created_at: 2025-11-22T10:00:00Z
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
  - superseded
superseded_by:
  - mem_20251205_318c41ee89b47965_000002
---
DECISION (superseded): flat 100 req/min per IP on auth endpoints. Superseded — NAT'd corporate clients all share an IP and got throttled. Do not follow.
