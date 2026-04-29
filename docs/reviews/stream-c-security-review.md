# Stream C Security / Poisoning Review

Review lane: Task 12 security and poisoning review.  
Scope: Stream C governance changes in `crates/memory-governance/**`, `crates/memoryd/src/{handlers,mcp,protocol,cli}.rs`, Stream C tests/docs, and Stream C-adjacent substrate API changes.  
Mode: read-only review except this report. No production code was patched.

## Executive summary

Stream C has the right architectural intention: typed governance decisions before Stream A writes. The dangerous gap is that the daemon boundary treats caller-supplied governance metadata and admin protocol messages as trusted. That creates several durable poisoning paths: refused decisions can still be written as quarantined records and later approved without revalidation, tombstones are not loaded by the live daemon, invalid disk policies silently fall back to built-ins, and privacy classification is hardcoded to `Trusted` for governed writes unless the caller voluntarily labels sensitivity.

## P0/P1/P2 summary

- P0: 1 — refused governance decisions can be persisted with `force_quarantine` and later promoted through unauthenticated review approval.
- P1: 5 — invalid policy files fail open; live daemon never enforces tombstone rules; grounding is caller-self-attested/file-existence-only; Stream D privacy gaps are marked `Trusted`; admin review protocol is exposed on the same unauthenticated daemon socket despite not appearing in the MCP manifest.
- P2: 1 — malformed governance metadata is accepted/defaulted instead of rejected, amplifying privacy/grounding bypasses.


## Remediation status (2026-04-29)

This report is historical. Local code changes after the review have addressed several original blockers:

- Caller-controlled `force_quarantine` is no longer accepted by `GovernanceMeta`; unknown metadata fields fail `invalid_request`. Refused decisions are terminal and are not laundered into quarantined writes.
- Disk policy YAML load failures now fail closed instead of falling back to built-ins when YAML exists.
- The live daemon now loads tombstone JSONL rules through `TombstoneIndex::load_jsonl_dir`; malformed tombstone files fail closed.
- Governance metadata now uses typed enums, `deny_unknown_fields`, and finite `0.0..=1.0` confidence validation.
- Missing or sensitive/unclassified privacy metadata now refuses with `privacy` pending Stream D classification.

Remaining concerns such as local daemon/admin authorization and the strength of grounding proof should be re-reviewed against the current tree before being promoted or cleared.

## Findings

### P0 — Refused writes can be durably stored and later promoted by chaining `force_quarantine` with review approval

**Evidence**

- The Stream C spec says `Refused` means the request “must not create or mutate a memory” (`docs/specs/stream-c-governance-v0.1.md:90`).
- The daemon accepts caller-supplied `force_quarantine` inside arbitrary `meta` (`crates/memoryd/src/handlers.rs:630-643`) and parses that metadata directly from the request body (`crates/memoryd/src/handlers.rs:689-700`).
- `execute_write_decision` checks `input.meta.force_quarantine` before it matches on the governance decision, then writes a quarantined memory unconditionally (`crates/memoryd/src/handlers.rs:274-296`). That means even `GovernanceWriteDecision::Refused` for grounding/policy/tombstone can become a durable quarantined file.
- Review approval then changes the memory to `status = Active` and `trust_level = Trusted` without rerunning grounding, tombstone, policy, contradiction, or privacy checks (`crates/memoryd/src/handlers.rs:955-966`).
- The daemon dispatch exposes `ReviewApprove` and `ReviewReject` requests directly (`crates/memoryd/src/handlers.rs:56-60`; `crates/memoryd/src/protocol.rs:41-43`).

**Exploitability**

A local daemon client sends `WriteMemory` with a poisoned body, e.g. `source_kind: "subagent"`, no valid `session-spawn:` proof, and `force_quarantine: true`. Governance would refuse for grounding, but the handler persists the record as quarantined. The same local client then sends `ReviewApprove { id }`, making the poisoned record active/trusted with no second governance pass.

**Impact**

This defeats the core safety invariant for durable memory poisoning. Any refusal reason that is computed after parse but before execution can be laundered into a durable review item and then an active trusted memory. If tombstone loading is fixed later, this bug would still let `force_quarantine` write tombstone hits unless decision execution is changed.

**Minimal remediation**

- Move `force_quarantine` handling after the decision match and allow it only for otherwise write-eligible non-refused decisions, or delete caller-controlled `force_quarantine` entirely.
- Treat `GovernanceWriteDecision::Refused` as terminal: no write, no quarantine, no candidate.
- Make `review approve` rerun governance against the current memory and reject if grounding/tombstone/privacy/policy checks do not still pass.
- Add an e2e regression: ungrounded/tombstoned write with `force_quarantine: true` must return `Refused` and must not create a file; approving a quarantined item with missing source proof must fail.

### P1 — Invalid disk policies fail open to built-in fallback policies

**Evidence**

- The contract says missing, malformed, or inapplicable policy fails closed with a `policy` refusal (`docs/specs/stream-c-governance-v0.1.md:31`).
- `load_policy_set` detects whether `repo/policies` has any YAML, but if `PolicySet::load_from_dir` fails it discards the error and returns `PolicySet::builtin()` with `BuiltInFallback` (`crates/memoryd/src/handlers.rs:491-507`).
- Built-in `project-standard` uses `TombstoneEnforcementMode::Review` (`crates/memory-governance/src/policy.rs:158-165`), while the Stream C spec says project-standard uses strict tombstones (`docs/specs/stream-c-governance-v0.1.md:79`).

**Exploitability**

An attacker or buggy automation that can edit policy files can insert malformed YAML or an unknown key. Instead of failing closed, the daemon silently downgrades to compiled defaults and continues processing writes. The response may say `policy_source = built_in_fallback`, but the write has already been authorized.

**Impact**

Policy files are the operator’s security boundary. Silent fallback makes policy corruption an enforcement bypass and can downgrade stricter local rules to weaker built-ins.

**Minimal remediation**

- If any disk policy YAML exists and loading fails, return `GovernanceWriteResponse { status: Refused, reason: Policy }` or a typed daemon error.
- Use built-in policies only for explicit bootstrap mode when no disk policies exist and the spec allows it.
- Preserve the load error in the response/log so operators can repair the policy file.
- Add daemon e2e coverage for malformed policy YAML and missing required scope.

### P1 — Live governed writes never enforce tombstone rules, so forgotten claims can be reintroduced

**Evidence**

- The spec says a tombstone hit always refuses the write and malformed tombstone files fail closed (`docs/specs/stream-c-governance-v0.1.md:126-148`).
- The governance engine only refuses when its in-memory tombstone index has a match (`crates/memory-governance/src/engine.rs:184-191`).
- The live daemon constructs every engine with `TombstoneIndex::default()` and never loads `repo/tombstones/*.jsonl` (`crates/memoryd/src/handlers.rs:510-519`).
- `memory_forget` calls Stream A `tombstone_memory` (`crates/memoryd/src/handlers.rs:259-271`), and Stream A only changes the target memory’s frontmatter and appends a tombstone event (`crates/memory-substrate/src/api.rs:665-692`); it does not create a Stream C tombstone rule under `tombstones/`.

**Exploitability**

After a user forgets a memory, a later governed write can submit the same claim with a new id. Since the daemon’s tombstone index is empty and active-memory duplicate search skips tombstoned records, the forgotten content can become active again.

**Impact**

This breaks the “forget means do not resurrect” guarantee and makes tombstones ineffective against durable poisoning or relationship-drift cleanup. It also means malformed tombstone files cannot fail closed because production never reads them.

**Minimal remediation**

- Load `TombstoneIndex::load_jsonl_dir(repo.join("tombstones"))` when building the live engine.
- Propagate `TombstoneLoadError` as a fail-closed `Refused { reason: Tombstone }` or daemon error.
- Make `memory_forget` write or derive a durable tombstone rule that matches future equivalent claims, not only the old memory id.
- Add e2e tests for re-adding a forgotten claim and for malformed tombstone JSONL.

### P1 — Grounding can be self-attested by request metadata and file refs prove existence, not claim support

**Evidence**

- The spec requires user writes to be self-grounded only when caller context identifies the local user/session, and agent/subagent writes require resolvable refs/session-spawn proof (`docs/specs/stream-c-governance-v0.1.md:115-122`).
- `GovernanceMeta` lets the request body supply `source_kind`, `source_ref`, and `explicit_user_context` (`crates/memoryd/src/handlers.rs:630-643`).
- `GovernanceWriteInput::candidate` trusts that metadata to set explicit user context (`crates/memoryd/src/handlers.rs:720-728`).
- `governance_sources` maps unknown source kinds to `AgentPrimary`, not an invalid-request refusal (`crates/memoryd/src/handlers.rs:831-838`).
- `FileSourceResolver` accepts any absolute `file:` path as grounded when `path.is_file()` is true, with no repo/session containment and no content/hash check tying the cited file to the claim (`crates/memory-governance/src/grounding.rs:71-85`; `crates/memory-governance/src/grounding.rs:122-134`).

**Exploitability**

A daemon client can claim `source_kind: "user"` and `explicit_user_context: true` without an authenticated user/session proof. Or it can claim an arbitrary absolute `file:` path that merely exists, even if the file does not support the proposed memory. A subagent can also use `force_quarantine` from the P0 finding to bypass the currently hardcoded session resolver.

**Impact**

Grounding becomes an attacker-controlled assertion rather than a verification boundary. This allows poisoned facts to look grounded in frontmatter and policy decisions, making later review harder because the record carries plausible-looking source metadata.

**Minimal remediation**

- Derive `source_kind`, user context, session id, and subagent id from the authenticated/registered caller, not from caller-supplied JSON.
- Reject unknown source kinds instead of defaulting to `AgentPrimary`.
- Restrict `file:` refs to approved repo/runtime/session roots, canonicalize paths, reject symlink escapes, and require a content hash or line-fragment digest that is checked at governance time.
- Add tests for self-attested user context, arbitrary absolute files, unknown source kinds, and symlinked dream journal refs.

### P1 — Stream D privacy is treated as trusted-by-default for governed writes

**Evidence**

- The spec says if Stream C needs privacy classification that Stream D has not supplied, it must fail closed and must not silently assume trusted classification (`docs/specs/stream-c-governance-v0.1.md:219`).
- `privacy_refusal` only fires when the caller voluntarily supplies `meta.sensitivity` equal to `confidential`, `personal`, `sensitive`, or `secret` (`crates/memoryd/src/handlers.rs:703-718`). Missing or misspelled sensitivity proceeds.
- `to_memory` writes governed memories with `Sensitivity::Internal`, `mask_personal_for_synthesis: false`, and indexing enabled for non-quarantined writes (`crates/memoryd/src/handlers.rs:731-778`).
- `write_governed_memory` always calls Stream A with `classification: ClassificationOutcome::Trusted` (`crates/memoryd/src/handlers.rs:393-407`), and `memory_supersede` does the same (`crates/memoryd/src/handlers.rs:237-244`).

**Exploitability**

A caller omits `sensitivity` or sets a benign/misspelled value while the body contains secrets, personal information, or confidential facts. The daemon writes plaintext, indexes it, and marks the write trusted.

**Impact**

This creates a pre-Stream-D privacy leak path and stores sensitive content in passive recall/search surfaces. It also makes later privacy repair harder because the event log records the write as trusted.

**Minimal remediation**

- Until Stream D exists, fail closed or quarantine all structured durable writes that lack a trusted privacy classification from a non-caller-controlled classifier.
- At minimum, reject unknown sensitivity values and keep unclassified writes out of passive recall/indexes.
- Do not pass `ClassificationOutcome::Trusted` by default; require an explicit trusted classification source.
- Add e2e tests for missing sensitivity, misspelled sensitivity, and secret-like bodies.

### P1 — Admin review protocol is hidden from the MCP manifest but still exposed on the unauthenticated daemon socket

**Evidence**

- The spec says MCP agent-facing tools must not expose admin review actions (`docs/specs/stream-c-governance-v0.1.md:209`).
- The MCP manifest itself lists only agent-facing tools (`crates/memoryd/src/mcp.rs:184-199`), so the manifest side is mostly clean.
- The same daemon protocol includes `ReviewQueue`, `ReviewApprove`, and `ReviewReject` variants (`crates/memoryd/src/protocol.rs:41-43`) and dispatches them without an authorization check (`crates/memoryd/src/handlers.rs:56-60`).
- The default socket path is `/tmp/memoryd.sock` for server and client commands (`crates/memoryd/src/cli.rs:37-48`, `crates/memoryd/src/cli.rs:169-197`), and the server binds/removes that path without setting restrictive permissions or validating peer credentials (`crates/memoryd/src/server.rs:74-83`).

**Exploitability**

Even though an MCP client cannot discover `memory_review_approve` from the manifest, any local process that can open the daemon socket can send the daemon JSON protocol directly and approve/reject queue items. In a multi-agent local environment, this is effectively an admin-tool leak around the MCP tool registry.

**Impact**

Review approval is the gate that turns quarantined/candidate memories into active/trusted records. Exposing it over the same unauthenticated socket collapses the boundary between agent-facing memory tools and operator/admin actions.

**Minimal remediation**

- Split admin review actions onto a separate admin socket or require an explicit local capability token/peer credential check.
- Place the default socket under a user-private runtime directory and chmod it to owner-only where supported.
- Keep MCP manifest tests, but add direct-protocol authorization tests proving ReviewApprove/Reject fail without admin credentials.
- Consider making the daemon reject admin requests unless launched in an operator/admin mode.

### P2 — Malformed governance metadata is defaulted instead of rejected

**Evidence**

- `GovernanceMeta` uses `#[serde(default)]` but not `deny_unknown_fields`, so typos and extra fields are ignored (`crates/memoryd/src/handlers.rs:630-643`).
- `GovernanceMeta::default` supplies project/user-like defaults (`namespace = Project`, `source_kind = "user"`, `sensitivity = None`) (`crates/memoryd/src/handlers.rs:652-665`).
- `memory_type` defaults unknown values to `MemoryType::Project` (`crates/memoryd/src/handlers.rs:893-902`), and unknown `source_kind` defaults to `AgentPrimary` for governance (`crates/memoryd/src/handlers.rs:831-838`).

**Exploitability**

A caller can misspell `sensitivity`, `source_kind`, or `explicit_user_context` and receive a different security posture than intended. An attacker can also add confusing extra fields that look reviewed in logs/client code but are ignored by the daemon.

**Impact**

This is a secondary amplifier for the P1 privacy and grounding bugs. Security-relevant metadata should fail closed on unknown fields/values; defaulting makes malformed inputs ambiguous and hard to audit.

**Minimal remediation**

- Add `#[serde(deny_unknown_fields)]` to `GovernanceMeta` and use typed enums for `memory_type`, `source_kind`, and `sensitivity`.
- Validate confidence is finite and within `0.0..=1.0` at the daemon boundary.
- Reject unknown source kinds, memory types, and sensitivity labels with `invalid_request`.
- Add protocol contract tests for unknown fields and invalid enum values.

## Residual risk and confidence

Residual risk remains high until the daemon boundary is hardened because Stream C currently assumes local daemon callers are honest. I did not run broad Rust gates because this was a read-only adversarial review lane over a dirty WIP tree; I verified only the requested report existence command after writing this file.

Confidence: high for the P0 and policy/tombstone/privacy findings because the cited control flow is direct. Confidence: medium-high for socket/admin exposure because exploitability depends on the intended local trust model and socket permissions at runtime, but the code currently has no explicit authorization check.
