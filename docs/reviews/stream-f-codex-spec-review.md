# Stream F Dreaming Spec Review - Codex

**Reviewed spec:** `docs/specs/stream-f-dreaming-v0.1.md`  
**Reviewer:** Codex  
**Date:** 2026-04-30  
**Verdict:** revise before implementation

## Summary

The Stream F architecture is directionally strong. The harness-CLI delegation is the right product cut: Stream F should own the cognitive pipeline, not API-key management or model routing. The spec also has the right high-level safety posture: dream prose is not grounding, Pass 2 never auto-promotes, masked synthesis is mandatory, and grounding rehydration happens at promotion time.

I would not implement directly from v0.1 yet. The remaining issues are not philosophical; they are contract seams that will create churn during implementation: config keys are missing, Pass 2 and Pass 3 prompt I/O are underspecified, the dream Markdown files need explicit exclusion from canonical memory parsing/indexing, current Stream D masking API names do not match the spec, and the subprocess privacy model needs a harder line on prompt transport.

## Strong parts worth preserving

- **Harness-CLI delegation is the right boundary.** Stream F should not own generic provider plumbing, bring-your-own API keys, token accounting, or model selection. Piggybacking on `claude`, `codex`, etc. is a good v0.1 simplification.
- **Safety invariants are mostly correct.** Dream prose never being a grounding source, Pass 2 candidates always entering a queue, and no dream-pass `memory_reveal` are the right defaults.
- **Git-synced, per-device substrate fits the product.** If dreaming is supposed to find cross-device and cross-harness patterns, device-local-only substrate weakens the point.
- **Acceptance signals are unusually actionable.** The named test files and cases are close to an implementation plan.
- **The user-facing provider disclosure should stay prominent.** Dream prompts are masked, but still leave the daemon through whichever upstream harness CLI is selected. Users need to see that clearly.

## Blocking or high-priority findings

### 1. Configuration contract is incomplete

The visible `dreams:` config block defines CLI priority, pass timeout, fragment lifetime, caps, and cleanup hour, but later sections rely on additional keys:

- `dreams.lease_window_seconds`
- `dreams.pass_1_window_days`
- `dreams.candidate_stale_days`
- the grounding rehydration drift threshold
- `events.compaction_days`

These need defaults, validation ranges, and docs in the configuration section before implementation starts. Otherwise implementers will invent incompatible defaults in different modules.

### 2. Pass 3 `<pending-attention>` surfacing may silently underfire

The spec says Pass 3 output remains masked, and Stream E surfaces questions only when entity/alias text intersects the active recall seed set. If entity names are masked, literal entity matching can fail even for relevant questions.

Recommended fix: make Pass 3 output structured enough to preserve entity hooks without unmasking. For example:

```text
ent_auth_flow,ent_jwt	What assumption about Person_A's auth bug reports are we overfitting to?
```

or JSONL:

```jsonl
{
  "entities": [
    "ent_auth_flow",
    "ent_jwt"
  ],
  "question": "What assumption about Person_A's auth bug reports are we overfitting to?"
}
```

Then Stream E matches on explicit entity ids and emits only the masked question text.

### 3. Pass 2 needs a deterministic evidence catalog

Pass 2 validation requires refs to come from the prompt input verbatim. That is the right safety rule, but the current Pass 2 inputs are described too loosely. Pass 2 should receive an explicit evidence catalog containing every valid `sub_*` and `mem_*` ref it is allowed to cite.

Recommended prompt input shape:

```json
{
  "pass_1_markdown": "...masked...",
  "evidence_catalog": [
    { "kind": "substrate_fragment", "ref": "sub_01J...", "entities": ["ent_auth_flow"], "excerpt": "...masked..." },
    { "kind": "memory", "ref": "mem_20260430_...", "entities": ["ent_auth_flow"], "summary": "...masked..." }
  ],
  "candidate_schema": { "...": "..." }
}
```

Validation should reject any candidate evidence ref not present in this catalog.

### 4. Harness CLI prompt transport is a privacy seam

The spec currently describes invocations like `claude -p <prompt>` and `codex exec <prompt>`, while also requiring prompts never be logged or written. Even masked prompts in argv can be visible through local process inspection.

Preferred rule: adapters must pass prompts over stdin when the harness supports it. If a supported v0.1 harness cannot accept stdin, the spec should explicitly state that argv exposure is accepted for that adapter and surface it in the privacy disclosure/status output. Do not leave this implicit.

### 5. Stream D `MaskingSession` API mismatch

The spec names `MaskingSession::unmask` and `MaskingSession::end`, but the shipped API appears to use `mask(...)` and `restore(session_id, ...)`, with teardown represented by dropping the in-memory session rather than an `end` method.

Either amend Stream D's API as part of Stream F or rewrite the spec against the shipped names. Do not leave implementers to discover the mismatch mid-rollout.

### 6. Dream Markdown must not be parsed as canonical memory Markdown

The spec says `dreams/journal/**.md` and `dreams/questions/**.md` are not canonical memories and have no frontmatter. The Stream A tree/indexing path needs an explicit amendment: these files must be skipped by canonical memory validators and indexers, then checked by dream-specific validators.

This should be an explicit acceptance test: a frontmatter-free dream Markdown file under `dreams/journal/<scope>/<date>.md` is valid, and it is not indexed as a canonical memory.

### 7. `DreamRunReport` response shape contradicts Pass 2 refusal reporting

`PassOutcome` currently has `candidate_ids`, but Pass 2 prose says refusals are recorded with `accepted: false, reason: <code>`. Add a concrete type, for example:

```rust
struct CandidateWriteResult {
    id: Option<String>,
    accepted: bool,
    reason: Option<String>,
    source_ref_count: usize,
}
```

Then `PassOutcome` can carry `candidate_results: Vec<CandidateWriteResult>` instead of only `candidate_ids`.

### 8. CLI naming should be normalized

The spec alternates between `memory dream ...` and `memoryd dream ...`. The shipped repo currently exposes `memoryd` commands. Unless a separate `memory` wrapper is in scope, v0.1 should standardize on `memoryd dream ...`.

## Answers to Claude's explicit design questions

### 1. Should substrate-fragment writes fold into `memory_note(kind=...)`, or become a separate `memory_observe` tool?

**Recommendation: use a separate `memory_observe` tool.**

I understand the API-parsimony argument for `memory_note` plus a `kind` enum, but the two operations encode different intent:

- `memory_note`: "write something note-like that may become canonical memory."
- `memory_observe`: "record raw substrate for later synthesis."

Those differences matter to agents. Tool names are behavioral nudges. If the same tool handles canonical notes and substrate observations, agents will eventually misuse `kind`, especially when deciding whether a thought is durable belief or raw signal.

Recommended public MCP surface:

```json
{
  "tool": "memory_observe",
  "arguments": {
    "text": "Third time investigating JWT validation in this repo - pattern emerging around key rotation.",
    "kind": "pattern",
    "entities": ["ent_auth_flow", "ent_jwt"]
  }
}
```

Allowed `kind` values can remain `observation | pattern | signal`. Internally, `memory_note` and `memory_observe` can share privacy classification, caller context, append helpers, and response structs. The separation only needs to exist at the agent-facing and daemon-protocol intent layer.

If you keep `memory_note(kind=...)`, I would at least rename the enum values so `NoteKind::Note` does not sit beside substrate-only kinds. But my stronger call is: separate tool.

### 2. Should substrate fragments be git-synced or device-local-only?

**Recommendation: git-sync substrate fragments, with strict guardrails.**

Synced substrate is closer to the product's DNA. Dreaming is supposed to identify patterns across sessions, devices, and harnesses. Device-local-only substrate produces partial dreams: the MacBook dreams one reality, the work machine dreams another, and neither sees the whole behavioral pattern.

The sync-surface concern is real, but the spec already has the right mitigation shape:

- per-device file prefixes to avoid merge conflicts;
- append-only JSONL;
- short default plaintext lifetime;
- encrypted substrate for PII tiers;
- encrypted substrate contributes only safe descriptors to dream input;
- substrate is not in canonical memory search/indexes;
- archived substrate is not treated as durable memory.

I would add one more explicit sentence to the public docs: syncing substrate increases the private git repo's raw-observation surface, even though it is not canonical memory.

So: sync it, but treat it as low-level durable telemetry rather than searchable knowledge.

### 3. Are git-fetch-based leases the right semantics?

**Recommendation: git-based lease election is acceptable for v0.1, but scheduled runs need a retry window.**

Using Stream A's git transport is the right v0.1 simplification. It avoids a new distributed coordination system. The weak point is the current failure mode: a short network blip at 03:00 can silently skip the day.

I would distinguish scheduled runs from manual runs:

- `memoryd dream now`: fail fast on `lease_unavailable` and report it.
- scheduled daily dream: retry within a bounded window, e.g. 03:00-06:00 UTC or `cleanup_run_hour_utc + dream_retry_window_minutes`.

Suggested scheduled semantics:

1. At scheduled time, attempt `fetch -> inspect lease -> append lease -> push`.
2. On fetch/push failure, record `lease_unavailable` in status and retry with bounded exponential backoff.
3. Keep retrying until the configured retry window closes.
4. If still unavailable, record a missed-run summary visible in `memoryd dream status` and `memoryd dream review`.

This preserves git as the lease backend without letting one transient sync failure erase the day's dream.

### 4. Is Pass 2 evidence-ref validation too tight?

**Recommendation: keep it tight, but add an explicit evidence catalog.**

Requiring evidence refs to come from the prompt input verbatim is the right safety invariant. It prevents dream-authored candidates from laundering hallucinated support into the candidate queue.

The concern about legitimate multi-fragment synthesis is solved by allowing multiple citations, not by weakening validation. A candidate can summarize across two or five fragments if it cites all relevant refs from the catalog.

Recommended validation rules:

- every candidate must cite at least one valid prompt-provided ref;
- candidates may cite multiple refs;
- every cited ref must exist in the Pass 2 evidence catalog;
- candidate claims may synthesize across cited refs, but uncited support does not count;
- hallucinated or out-of-window refs are deterministic rejects.

This is strict, but not too strict. It is exactly the kind of deterministic seam dreaming needs.

### 5. Are the `<pending-attention>` caps right?

**Recommendation: start slightly tighter: 2 per scope, 6 total, keep 240 bytes/question.**

The current 3/scope and 8 total are defensible guesses, but `<pending-attention>` is startup cognitive load. Too many adversarial questions will feel like nagging and train users/agents to ignore the section.

My preferred v0.1 defaults:

- 2 questions per scope;
- 6 questions total;
- 240 UTF-8 bytes per question remains fine.

Add deterministic ordering so the cap is predictable:

1. strongest entity overlap with the active recall seed set;
2. most recent question file;
3. novelty against recently surfaced question hashes;
4. stable lexical/hash tie-break.

Also add counters for omitted dream questions by reason: cap, no entity match, unsafe fragment, malformed line. That gives real data for tuning the caps later.

## Other recommended revisions

- Define `ScopeRunSummary` and `HarnessCliStatus` in the daemon protocol section.
- Define the encrypted substrate JSONL shape and descriptor projection explicitly.
- Specify git commit author/message conventions for lease writes and cleanup writes.
- Specify dirty-tree behavior when the daemon needs to commit lease or cleanup mutations.
- Clarify whether `dreams/cleanup/<device_id>/<date>.json` is a new top-level path contract; it appears in cleanup but not in the initial owned path list.
- Fix the small wording error: the spec says "Three new top-level directories" and then lists four.
- Consider encoding scope path segments instead of using literal `project:proj_abc` in paths. Colons are awkward and can fight slug/path assumptions. `project/proj_abc` or a URL-safe encoded scope segment is safer.

## Bottom line

I would greenlight the product architecture and revise the spec before implementation. The most important edits are:

1. split `memory_observe` from `memory_note`;
2. keep substrate git-synced;
3. retain git leases but add scheduled retry/missed-run semantics;
4. keep strict Pass 2 evidence validation with an explicit evidence catalog;
5. make Pass 3 question files structured enough for entity matching while masked;
6. tighten `<pending-attention>` caps to 2/scope and 6 total for v0.1;
7. align config/API names with shipped Streams C/D/E code before coding begins.
