---
schema_version: 1
id: mem_20251106_3d7b4e1955df9118_000002
type: artifact
scope: project
summary: The Atlas 'pipeline' is the billing aggregation pipeline that rolls usage events up into invoice line items.
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-06T10:00:00Z
updated_at: 2025-11-06T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - entity
  - pipeline
  - billing
  - aggregation
entities:
  - id: ent_pipeline
    label: Pipeline
    aliases:
      - billing pipeline
      - data pipeline
---
In Atlas, 'pipeline' = the billing aggregation data pipeline (usage events -> invoice line items, 5-minute windows). NOT the Quill CI 'Pipeline' — same word, different project and meaning.
