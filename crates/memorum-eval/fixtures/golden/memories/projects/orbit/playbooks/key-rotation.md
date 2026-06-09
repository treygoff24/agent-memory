---
schema_version: 1
id: mem_20251114_f2190dc0b948411e_000002
type: playbook
scope: project
summary: "Playbook: rotate the JWT signing key by adding the new key to the JWKS, waiting one max-TTL, then retiring the old key."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-14T10:00:00Z
updated_at: 2025-11-14T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - playbook
  - key-rotation
  - jwt
  - jwks
---
PLAYBOOK — signing-key rotation:
1. Generate new key in KMS, add its public half to the JWKS endpoint.
2. Start signing new tokens with the new key.
3. Wait one max access-token TTL (15 min) so old tokens drain.
4. Remove the old key from JWKS.
Never skip the drain window or you'll reject valid tokens.
