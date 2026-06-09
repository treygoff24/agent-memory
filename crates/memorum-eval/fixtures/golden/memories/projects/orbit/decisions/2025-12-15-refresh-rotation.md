---
schema_version: 1
id: mem_20251216_2e24f74dcc2006bd_000002
type: decision
scope: project
summary: "Decision: refresh tokens are single-use and rotate on every use; reuse of a consumed refresh token revokes the whole family (theft detection)."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-12-16T10:00:00Z
updated_at: 2025-12-16T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - decision
  - refresh-token
  - rotation
  - security
---
Decision: refresh tokens are single-use and rotate on every use; reuse of a consumed refresh token revokes the whole family (theft detection).
