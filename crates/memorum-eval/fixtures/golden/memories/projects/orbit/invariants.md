---
schema_version: 1
id: mem_20251113_c19c1cce237197e3_000001
type: invariant
scope: project
summary: "Orbit invariant: signing keys never leave KMS; the service signs via the KMS API and never holds private key material in memory."
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-13T10:00:00Z
updated_at: 2025-11-13T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: orbit/identity
canonical_namespace_id: proj_e06dae2d38b4
tags:
  - invariant
  - kms
  - security
  - keys
entities:
  - id: ent_kms
    label: KMS signing key
---
INVARIANT: JWT signing keys never leave KMS. Orbit calls the KMS sign API; private key material is never loaded into application memory. Marco audits this quarterly. Violation is a security incident.
