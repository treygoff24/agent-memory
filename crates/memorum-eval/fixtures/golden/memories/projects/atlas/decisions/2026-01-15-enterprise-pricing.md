---
schema_version: 1
id: mem_20260115_3ef5219024821d9a_000001
type: decision
scope: project
summary: "Decision: enterprise tier floor price set to a confidential negotiated figure; do not surface in synthesis."
confidence: 0.9
trust_level: trusted
sensitivity: confidential
status: active
created_at: 2026-01-15T10:00:00Z
updated_at: 2026-01-15T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: atlas/billing
canonical_namespace_id: proj_2170411deb73
tags:
  - decision
  - pricing
  - confidential
retrieval_policy:
  passive_recall: true
  max_scope: project
  mask_personal_for_synthesis: true
  index_body: false
  index_embeddings: false
---
DECISION (confidential): enterprise tier floor pricing was set in a closed Finance/Sales meeting. Confidential-sensitivity: not body/embedding indexed, masked for synthesis. Exists in corpus to exercise the privacy path.
