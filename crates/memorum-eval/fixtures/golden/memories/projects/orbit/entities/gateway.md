---
schema_version: 1
id: mem_20251112_ac4d7ac60ae71bc7_000002
type: artifact
scope: project
summary: The Orbit 'gateway' is the public API gateway that terminates TLS, validates JWTs, and routes to internal services.
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-12T10:00:00Z
updated_at: 2025-11-12T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - entity
  - gateway
  - api
  - jwt
  - routing
entities:
  - id: ent_orbit_gateway
    label: gateway
    aliases:
      - API gateway
---
In Orbit, 'gateway' = the public API gateway. Terminates TLS, validates the JWT access token, routes to internal services. NOT the Atlas payment gateway — different service, same word. Owned by Noor's team.
