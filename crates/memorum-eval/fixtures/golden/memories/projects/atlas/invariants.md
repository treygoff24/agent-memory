---
schema_version: 1
id: mem_20251102_bef37069407dccc9_000001
type: invariant
scope: project
summary: "Atlas invariant: money amounts are integer minor units (cents); floats are never used for currency anywhere in the codebase."
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-02T10:00:00Z
updated_at: 2025-11-02T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - invariant
  - money
  - currency
entities:
  - id: ent_money
    label: money representation
---
INVARIANT: all monetary amounts are stored and computed as integer minor units (cents) with an explicit currency code. Floating point is banned for currency. Violating this is an automatic PR block. This is the rule the Rust type-system preference traces back to.
