---
schema_version: 1
id: mem_20251105_feddfe9c424e2a78_000001
type: procedure
scope: user
summary: "User's git workflow: rebase feature branches, never merge-commit into main, squash on merge."
confidence: 0.9
trust_level: trusted
sensitivity: internal
status: active
created_at: 2025-11-05T09:00:00Z
updated_at: 2025-11-05T09:00:00Z
author:
  kind: user
  user_handle: trey
tags:
  - git
  - workflow
---
Git workflow Dana enforces on Atlas:
- Rebase feature branches onto main; no merge commits into main.
- Squash-merge PRs.
- One logical change per PR.
