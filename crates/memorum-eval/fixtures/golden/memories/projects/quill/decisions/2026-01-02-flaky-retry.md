---
schema_version: 1
id: mem_20260102_83e3ade3e278ada3_000001
type: decision
scope: project
summary: "Initial response to flaky CI: auto-retry failed test jobs up to twice to keep the pipeline green."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: superseded
created_at: 2026-01-02T10:00:00Z
updated_at: 2026-01-20T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: quill/docs
canonical_namespace_id: proj_cbde0e5dce53
tags:
  - ci
  - flaky
  - retry
  - superseded
entities:
  - id: ent_pipeline
    label: Pipeline
    aliases:
      - CI pipeline
superseded_by:
  - mem_20260120_59c0e30eb1190c0d_000001
---
DECISION (superseded): auto-retry failed CI jobs twice to mask flakes. Superseded once we realized retries hid a real race in the test harness and the flake rate kept climbing. Do not follow this — it treats the symptom.
