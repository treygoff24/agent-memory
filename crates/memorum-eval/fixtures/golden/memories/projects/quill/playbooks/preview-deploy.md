---
schema_version: 1
id: mem_20260105_f24704b2b01c0ac1_000005
type: playbook
scope: project
summary: "Playbook: a broken PR preview deploy is almost always a stale vendored dependency — clear the build cache and re-run before deeper debugging."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-01-05T10:00:00Z
updated_at: 2026-01-05T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: quill/docs
canonical_namespace_id: proj_cbde0e5dce53
tags:
  - playbook
  - preview
  - ci
  - cache
---
PLAYBOOK — broken Quill preview deploy:
1. 90% of the time it's a stale vendored dep or build cache. Clear cache, re-run.
2. If still broken, check the GitHub Actions runner image version.
3. Only then dig into the build logs.
