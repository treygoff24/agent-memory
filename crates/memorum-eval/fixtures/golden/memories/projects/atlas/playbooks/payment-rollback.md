---
schema_version: 1
id: mem_20251104_8160843165a0f533_000001
type: playbook
scope: project
summary: "Playbook for rolling back a bad payments deploy: flip the kill switch, drain in-flight, redeploy previous tag, reconcile the ledger."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-04T10:00:00Z
updated_at: 2025-11-04T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - playbook
  - rollback
  - payments
  - incident
---
PLAYBOOK — payments rollback:
1. Flip the `payments_kill_switch` flag (stops new charges).
2. Let in-flight charges drain (max 90s).
3. Redeploy the previous known-good tag.
4. Run the ledger reconciliation job.
5. Page Priya, post in #payments-incidents.
