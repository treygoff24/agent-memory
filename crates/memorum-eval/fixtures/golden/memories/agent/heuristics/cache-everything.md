---
schema_version: 1
id: mem_20251005_45ee8456f7ac57fb_000001
type: heuristic
scope: agent
summary: "Heuristic: cache aggressively at every layer to improve latency."
confidence: 0.9
trust_level: untrusted
sensitivity: internal
status: tombstoned
created_at: 2025-10-05T10:00:00Z
updated_at: 2026-02-01T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - heuristic
  - cache
  - stale
tombstone_events:
  - id: tomb_01J9AGCACHE
    applied_at: 2026-02-01T10:00:00Z
    actor:
      kind: agent
      ref: claude-code
    reason: wrong
    reason_text: Caused stale-data incidents; replaced by a measure-first caching heuristic.
    prior_status: active
---
(Tombstoned) Old heuristic: 'cache aggressively everywhere'. Caused multiple stale-data bugs. Retracted in favor of 'measure, then cache the proven hot path with explicit invalidation'. Should not be recalled as guidance.
