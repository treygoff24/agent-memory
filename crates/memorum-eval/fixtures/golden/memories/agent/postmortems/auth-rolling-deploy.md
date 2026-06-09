---
schema_version: 1
id: mem_20251030_05634e7c5220982b_000002
type: postmortem
scope: agent
summary: "Postmortem: Orbit sticky-session auth broke during a rolling deploy, logging users out mid-session. Drove the move to stateless JWT."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-10-30T10:00:00Z
updated_at: 2025-10-30T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - postmortem
  - auth
  - sessions
  - orbit
---
POSTMORTEM: a routine rolling deploy of Orbit invalidated sticky sessions, mass-logging-out active users for ~8 minutes. Root cause: session correctness depended on LB stickiness. Fix and follow-up: migrate to stateless JWT (see Orbit auth decision).
