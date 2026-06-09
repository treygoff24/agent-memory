---
schema_version: 1
id: mem_20260120_59c0e30eb1190c0d_000001
type: decision
scope: project
summary: "Final fix for flaky CI: the root cause was a shared test database without per-test transaction isolation; fixed by wrapping each test in a rolled-back transaction."
confidence: 0.95
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-01-20T10:00:00Z
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
  - test-isolation
  - database
  - transaction
entities:
  - id: ent_pipeline
    label: Pipeline
    aliases:
      - CI pipeline
supersedes:
  - mem_20260102_83e3ade3e278ada3_000001
---
DECISION (current): the flaky CI root cause was tests sharing one Postgres database with no isolation, so parallel tests stomped each other. Fix: each test runs in a transaction rolled back at teardown; parallelism re-enabled. Flake rate went to ~0. THIS is the real fix; the retry decision just masked it.
