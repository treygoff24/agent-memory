---
schema_version: 1
id: mem_20251020_bddedd358232317b_000004
type: pattern
scope: agent
summary: "Pattern: make builds hermetic (no network, vendored inputs) before chasing flaky tests — it isolates the real nondeterminism."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-10-20T10:00:00Z
updated_at: 2025-10-20T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - pattern
  - build
  - hermetic
  - flaky
---
PATTERN: before hunting flakes, make the build hermetic. Removing network/clock/filesystem nondeterminism shrinks the search space to the actual culprit.
