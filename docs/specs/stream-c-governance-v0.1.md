# Stream C Governance Spec v0.1

**Status:** implementation contract for Stream C governance.
**Sources:** `docs/specs/system-v0.1.md` §§11, 14, 19 and the shipped Stream A substrate contract.
**Non-source:** Stream A specs are dependency contracts, not editing targets for this stream.

Stream C adds deterministic governance between Stream B's daemon/MCP transport and Stream A's canonical write APIs. Stream A remains the only layer that mutates Markdown memories, event logs, and derived indexes.

## 1. Scope and dependency boundaries

Stream C owns:

- loading and validating versioned governance policies;
- evaluating `memory_write`, `memory_supersede`, and `memory_forget` requests before canonical writes;
- grounding verification for locally resolvable source references;
- deterministic tombstone matching;
- contradiction classification orchestration through provider traits;
- supersession planning and lifecycle invariants;
- review queue projection for quarantined or review-required memories.

Stream C does not own:

- direct file mutation, event-log appends, or index writes; those remain Stream A substrate responsibilities;
- Unix-socket transport, CLI parsing, or MCP manifest generation except where Stream B handlers call governance;
- privacy classification internals, encrypted storage, passive recall ranking, or UI surfaces beyond review queue data needed by CLI/daemon tests.

All governance APIs must return typed decisions and typed refusal reasons. Callers must not infer behavior by parsing free-form strings.

## 2. Policy schema v1 and built-in policies

Policy files are YAML documents loaded from `policies/*.yaml`. Unknown keys are invalid. A missing, malformed, or inapplicable policy fails closed with a `policy` refusal.

Implemented schema v1:

```yaml
name: project-standard
version: 2
scope: project
confidence_floor: 0.7
requires_grounding: true
tombstone_enforcement: review
contradiction_policy: supersede
review_gates:
  - low_confidence
```

Required fields:

- `name`: one of the built-in policy names below.
- `version`: positive integer included in `policy_applied` as `<name>@v<version>`.
- `scope`: `me`, `project`, `agent`, or `dreaming`. `scope` is the runtime policy selection key.
- `confidence_floor`: inclusive `0.0..=1.0`; invalid or non-finite values fail policy loading.
- `requires_grounding`: whether the selected policy requires resolvable grounding.
- `tombstone_enforcement`: `refuse` or `review`. `refuse` returns a tombstone refusal; `review` writes a candidate review item with `next_actions: ["tombstone"]`.
- `contradiction_policy`: `supersede` or `quarantine`. `supersede` authorizes a bidirectional supersession chain for typed contradictions; `quarantine` routes contradictions to review.
- `review_gates`: list of trigger names; empty is allowed. The implemented built-ins use `low_confidence`, `missing_grounding`, and `dream_source`.

The policy loader uses `deny_unknown_fields`. Older design-only keys such as
`schema_version`, `grounding_required`, `grounding_rehydration_required`,
`review_gate`, `sensitivity_defaults`, `privacy_filter`, and `subagent_writes`
are not accepted by the implemented YAML schema.

Built-in policies:

- `me-strict`: personal/user-scoped memory policy with high confidence floor, required grounding, refuse-mode tombstones, contradiction quarantine, and low-confidence/missing-grounding gates.
- `project-standard`: project-scoped memories with grounded evidence, `tombstone_enforcement: review`, contradiction supersession, and `low_confidence` review gating.
- `agent-strict`: cross-project agent memory with a higher confidence floor, required grounding, refuse-mode tombstones, contradiction quarantine, and low-confidence/missing-grounding gates.
- `dreaming-strict`: dream-derived proposals only; requires grounding, refuse-mode tombstones, contradiction quarantine, and includes the `dream_source` review gate. Dream prose is still rejected as a direct source by grounding rules.

## 3. Governance decision state machine

The governance engine returns one of these stable outcomes:

- `Promoted`: request may be written as an active memory. Includes `id`, `namespace`, `policy_applied`, and optional `supersedes`.
- `Candidate`: request may be written as `status: candidate`. Used for low-risk pending review, review-gate blocking, or caller-visible candidate queues.
- `Quarantined`: request may be written as `status: quarantined`; it is excluded from passive recall and appears in review queue.
- `Refused`: request must not create or mutate a memory. Includes reason code `grounding`, `contradiction`, `tombstone`, `privacy`, or `policy`, plus structured details.
- `Duplicate`: request matches an existing active memory and must not create a second active memory. Response includes existing memory id.
- `Refinement`: request should update or merge evidence into an existing memory instead of creating a peer active memory. Governance returns a typed merge plan; Stream A performs the mutation.
- `Superseded`: an old memory has been replaced through a valid supersession chain and must remain walkable.
- `Tombstoned`: a memory has been forgotten by adding an active tombstone rule and changing canonical status to `tombstoned`.

Allowed write-path transitions:

```text
incoming request
  -> Refused | Duplicate | Refinement | Promoted | Candidate | Quarantined
Candidate -> Promoted | Quarantined | Tombstoned
Quarantined -> Promoted | Candidate | Tombstoned
Promoted -> Superseded | Tombstoned
Superseded -> Tombstoned
```

No transition deletes canonical history. Human review state is represented with frontmatter `review_state`, not as an additional v0.1 governance outcome.

## 4. Grounding ref resolution rules for v0.1

Grounding proves that a write is traceable to a resolvable local source at decision time.

Rules:

- User writes may be self-grounded when the request source kind is `user` and the caller context identifies the local user/session.
- Agent-primary writes require a `source.ref` resolvable by a local resolver: file ref, tool transcript handle, cached URL/artifact handle, or session turn handle.
- Subagent writes require both a resolvable `source.ref` and a session-spawn registry entry proving the subagent existed under the parent session.
- Tool/file writes require refs that still resolve locally at governance time.
- Dream prose is never a source. Paths under `dreams/journal/` and source kind `synthesis` from dream prose are refused as grounding even if the file exists.
- Dream-derived proposals may cite substrate fragments or live source files; `dreaming-strict` requires rehydrating those refs before promotion.

A missing, stale, or disallowed source returns `Refused { reason: grounding }` unless policy explicitly allows quarantine for suspicious but potentially recoverable source data.

## 5. Tombstone rule schema and matching canonicalization

Tombstones are active deletion/refusal rules. A tombstone hit always refuses the write; it does not quarantine silently.

Rule schema v0.1, stored as JSONL under `tombstones/`:

```json
{
  "id": "tomb_20260429_0001",
  "target_memory_id": "mem_20260423_014",
  "content_hash": "sha256:...",
  "entity_hash": "sha256:...",
  "reason": "user_forget",
  "reason_text": "user asked to forget this claim",
  "active": true,
  "created_at": "2026-04-29T00:00:00Z"
}
```

Canonicalization:

- content hash input is Unicode-normalized text with Markdown formatting removed where possible, lowercased, and collapsed whitespace;
- entity hash input is the sorted set of canonical entity ids, lowercased and joined with `\n`;
- a match requires an active rule and either matching `target_memory_id` or matching both content hash and entity hash;
- malformed tombstone files are typed load errors and fail closed for governance decisions that require tombstone enforcement.

## 6. Contradiction detection stages and provider trait boundary

The contradiction pipeline runs after policy selection, grounding verification, and tombstone matching.

Stages:

1. Normalize the candidate claim, namespace, entity set, and target memory type.
2. Detect exact duplicates by canonical claim hash in the same namespace/entity set; return `Duplicate` without provider calls.
3. Retrieve top-K active memories in the same namespace and entity set through a `SimilaritySearch` trait.
4. If max similarity is below the policy threshold, continue to final promotion/candidate/quarantine decision.
5. If max similarity meets or exceeds the threshold, call a `ContradictionTiebreaker` trait with the candidate and top-K summaries.
6. Map tiebreak outcomes:
   - `Same` -> `Duplicate`;
   - `Refinement` -> `Refinement` with merge/evidence plan;
   - `Contradiction` -> supersession plan or `Quarantined`, according to policy;
   - `Unclear` -> `Quarantined` with reason `contradiction_unclear`.

Provider traits must be deterministic in tests. Production LLM or embedding providers are not wired in v0.1; no governance unit test may make a network call.

## 7. Supersession chain write invariants

Supersession replaces a claim without erasing the old record.

Invariants:

- the replacement memory records `supersedes: [old_id]`;
- the old memory records `superseded_by: [new_id]`;
- the old memory status becomes `superseded`, not deleted;
- `valid_until` on the old memory is capped when a replacement validity boundary is known;
- `valid_from` on the new memory is set when known or left null explicitly;
- the event log records the lifecycle change;
- the chain is walkable in both directions and passes Stream A tree validation;
- partial write failure must be visible as a typed failure that includes any committed side effects so the daemon can stop lifecycle writes until repair.

The governance crate may create a `SupersessionPlan`; only Stream A substrate APIs execute canonical mutations.

## 8. Review queue visibility requirements

The review queue is derived from canonical memory frontmatter, never from a hidden side database.

Queue includes memories with:

- `status: quarantined`;
- `status: candidate` with review state `candidate`, `pending`, `pending_review`, or legacy `pending-review`;
- policy decision details requiring user or parent-agent attention.

Queue excludes memories with:

- `status: active`, `pinned`, `superseded`, `archived`, or `tombstoned`, unless a future audit command explicitly asks for historical records.

Each queue item exposes:

- memory id, namespace, type, summary, and entity ids;
- status and review state;
- `policy_applied`;
- reason code and structured details;
- source descriptor and evidence refs when available;
- stable next actions: `approve`, `reject`, `forget`, or `quarantine` where valid.

CLI/daemon review commands may list and resolve queue items. MCP agent-facing tools must not expose admin review actions unless a future spec explicitly changes the tool boundary.

## 9. Stream D/E/G non-goals

Stream C must not implement:

- Stream D privacy internals: regex detector evolution, local Privacy Filter inference, span labeling, age encryption, secret leak history rewriting, or masked synthesis storage.
- Stream E startup recall: final recall block assembly, passive recall ranking, memory_startup implementation, or startup cache policy.
- Stream G UI: TUI/web dashboard screens, browser interactions, visual review workflows, or notification UX.

If a Stream C decision requires privacy classification that Stream D has not supplied, the decision fails closed with `Refused { reason: privacy }` or a policy refusal. It must not silently assume trusted classification.
