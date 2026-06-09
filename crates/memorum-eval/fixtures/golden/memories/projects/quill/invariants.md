---
schema_version: 1
id: mem_20260104_2e92b9fee57807c2_000001
type: invariant
scope: project
summary: "Quill invariant: docs builds must be reproducible — no network access during the build, all inputs vendored."
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-01-04T10:00:00Z
updated_at: 2026-01-04T10:00:00Z
author:
  kind: agent
  harness: claude-code
  session_id: sess_g0001
namespace: quill/docs
canonical_namespace_id: proj_cbde0e5dce53
tags:
  - invariant
  - build
  - reproducible
  - hermetic
---
INVARIANT: the Quill docs build is hermetic — zero network access during build, every input vendored or content-addressed. This is what made the flaky-CI root cause findable: once the build was hermetic, the only remaining nondeterminism was the shared test DB.
