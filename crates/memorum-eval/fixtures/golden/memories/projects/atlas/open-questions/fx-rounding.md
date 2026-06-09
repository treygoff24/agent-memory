---
schema_version: 1
id: mem_20260112_7c8e8049dc99a279_000001
type: open-question
scope: project
summary: "Open question: how should Atlas handle FX-rounding remainders on multi-currency settlement — accumulate or write off per-transaction?"
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-01-12T09:00:00Z
updated_at: 2026-01-12T09:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - open-question
  - multi-currency
  - rounding
---
OPEN QUESTION: on multi-currency settlement, sub-minor-unit FX remainders accumulate. Do we sweep them into a remainder account (auditable) or write off per transaction (simpler, tiny loss)? Finance hasn't decided. Blocks the JPY rollout.
