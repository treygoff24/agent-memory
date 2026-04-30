# Stream D Correctness/API Review

Date: 2026-04-29

## Review loop

Fresh-context correctness review initially requested changes for encrypted
supersede/forget behavior and MCP admin-name coverage.

## Findings and fixes

### Encrypted forget was plaintext-only

Fix: Stream A `read_memory_envelope` fallback now considers encrypted markdown,
and `tombstone_memory` reads envelopes instead of plaintext-only records.
`memory_forget` writes tombstone rules from plaintext body when available, or
from safe metadata for encrypted records.

### Encrypted supersession helper bypassed Stream A semantics

Fix: removed the daemon-side encrypted supersession mutation helper. v0.1 fails
closed for encrypted supersede replacements and encrypted old-memory
supersession until Stream A has an atomic encrypted lifecycle API.

### MCP manifest did not pin Stream D admin names

Fix: MCP manifest tests now explicitly reject Stream D privacy/filter/device
admin tool names in both manifest and `ToolName` conversion paths.

## Status

Core API invariants are covered by `memory-privacy` contract tests,
`memoryd/tests/privacy_e2e.rs`, `memoryd/tests/governance_e2e.rs`, and
`memoryd/tests/mcp_manifest.rs`. Stream E remains `not_implemented`.
