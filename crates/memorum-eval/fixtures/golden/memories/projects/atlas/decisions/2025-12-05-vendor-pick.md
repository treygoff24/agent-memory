---
schema_version: 1
id: mem_20251205_5d658074dcb2ea52_000001
type: decision
scope: project
summary: Decision to adopt 'PayFast' as a third payment processor.
confidence: 0.9
trust_level: untrusted
sensitivity: internal
status: tombstoned
created_at: 2025-12-05T10:00:00Z
updated_at: 2026-01-20T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - decision
  - vendor
  - stale
entities:
  - id: ent_payfast
    label: PayFast
tombstone_events:
  - id: tomb_01J7ATLVND
    applied_at: 2026-01-20T10:00:00Z
    actor:
      kind: user
      ref: ravi
    reason: wrong
    reason_text: PayFast integration was cancelled by Finance; decision reversed.
    prior_status: active
---
(Tombstoned) We had decided to add PayFast as a third processor. Finance cancelled the contract in Jan 2026; the decision was reversed. Should not surface as a current decision.
