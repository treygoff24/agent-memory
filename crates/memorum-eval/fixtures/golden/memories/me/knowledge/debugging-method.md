---
schema_version: 1
id: mem_20251105_168458837760c39b_000003
type: procedure
scope: user
summary: "User's debugging method: reproduce first, bisect second, read the code third; never guess-and-patch."
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
  - debugging
  - method
---
Debugging discipline: 1) get a reliable repro, 2) git bisect if it's a regression, 3) read the code path. No speculative patches without a repro.
