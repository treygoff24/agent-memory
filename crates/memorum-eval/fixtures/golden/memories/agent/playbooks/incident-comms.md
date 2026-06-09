---
schema_version: 1
id: mem_20251101_ff7f6930a5d9b269_000003
type: playbook
scope: agent
summary: "Playbook: incident comms — declare severity, post a single source-of-truth thread, update every 15 min, write the postmortem within 48h."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-01T10:00:00Z
updated_at: 2025-11-01T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
tags:
  - playbook
  - incident
  - comms
---
PLAYBOOK — incident comms:
1. Declare severity explicitly.
2. One source-of-truth thread; all updates land there.
3. Update every 15 minutes even if 'no change'.
4. Blameless postmortem within 48 hours.
