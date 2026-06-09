---
schema_version: 1
id: mem_20251103_c4cad2fb7372a353_000001
type: artifact
scope: project
summary: The Atlas 'gateway' is the payment gateway adapter that talks to Stripe and Adyen; rate-limited to 50 req/s per processor.
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-03T10:00:00Z
updated_at: 2025-11-03T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - entity
  - gateway
  - payments
  - stripe
  - adyen
entities:
  - id: ent_atlas_gateway
    label: gateway
    aliases:
      - payment gateway
---
In Atlas, 'gateway' = the payment gateway adapter (handlers for Stripe, Adyen). Rate-limited to 50 req/s per processor. Owns retry/backoff for processor 5xx. NOT to be confused with the Orbit API gateway — different service, same word.
