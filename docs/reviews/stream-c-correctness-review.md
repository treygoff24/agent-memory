### Verdict

Changes requested

### Intended outcome

Stream C appears intended to insert deterministic governance between Stream B's daemon/MCP transport and Stream A's canonical write APIs. The business outcome is: governed memory writes should select policy, verify grounding, refuse tombstoned/unsafe claims, classify duplicate/refinement/contradiction cases correctly, execute supersession chains without graph drift, and derive a review queue from canonical frontmatter rather than a hidden side store.

### Executive summary

The implementation has the right high-level shape (separate `memory-governance` crate, typed decisions, deterministic provider traits, Stream A as mutation authority), but several lifecycle invariants are not actually enforced at the daemon boundary. The biggest risks are that invalid policy files silently fall back to built-ins, tombstone rules are never loaded into the live daemon engine, unrelated active memories can force quarantines because similarity is effectively hardcoded to 1.0, explicit supersede requests execute for non-supersession decisions, and partial supersession failures can leave a replacement committed without durable repair state. Tests cover happy-path and crate-local contracts, but do not prove the end-to-end invariants that would catch these failures.

#
## Remediation status (2026-04-29)

This report is historical. Local code changes after the review have resolved or partially resolved multiple findings:

- Malformed disk policy YAML now fails closed when YAML exists.
- Live governance now loads tombstone JSONL rules, and malformed tombstone rules fail closed.
- Explicit `memory_supersede` now requires a `Supersession` decision for the requested `old_id` before mutating Stream A supersession state.
- Review queue spelling has been aligned on `pending_review`, with compatibility for `pending` and legacy `pending-review`.
- Review queue responses now have daemon default/max limits.

Remaining data-integrity and contradiction-adapter concerns should be revalidated against the latest working tree before treating this report's original P1/P2 list as active blockers.

## Findings

[P1 / High] Correctness: Invalid disk policies fail open to built-in defaults

- Evidence: `docs/specs/stream-c-governance-v0.1.md:31` requires missing, malformed, or inapplicable policy to fail closed; `crates/memoryd/src/handlers.rs:491-507` loads `repo/policies` but returns `(PolicySet::builtin(), BuiltInFallback)` when `PolicySet::load_from_dir` errors.
- Why it matters: A malformed or partially edited policy can silently downgrade enforcement to compiled defaults and still accept writes. That violates the policy trust boundary and can promote memories under a policy the operator did not intend.
- Reasoning: `load_policy_set` distinguishes only whether any YAML exists. If YAML exists but parsing fails, the error is discarded at `handlers.rs:501-504`; callers receive a normal engine using built-ins. This contradicts the fail-closed contract and makes policy corruption operationally invisible except for `policy_source` in successful responses.
- Recommendation: Return a structured governance refusal or handler error for policy load/validation failures when disk policies are present. Only use built-in fallback when no policy directory/files exist and the spec explicitly allows bootstrap fallback; include tests for malformed YAML refusing writes end-to-end.
- Confidence: High

[P1 / High] Correctness: Live governed writes never enforce tombstone rule files

- Evidence: `docs/specs/stream-c-governance-v0.1.md:126-148` requires active tombstone matches to refuse writes and malformed tombstone files to fail closed; `crates/memoryd/src/handlers.rs:510-519` constructs every live `GovernanceEngine` with `TombstoneIndex::default()`; `crates/memory-governance/src/engine.rs:184-191` only refuses when that in-memory index has a match.
- Why it matters: A user can forget/tombstone a claim, but a later `memory_write` can recreate the same claim because the daemon never reads `tombstones/*.jsonl`. This breaks the core “tombstone refusal” lifecycle guarantee.
- Reasoning: The tombstone module has loader tests, but the daemon path never calls `TombstoneIndex::load_jsonl_dir(repo/tombstones)`. The engine therefore always sees an empty index, so tombstone refusal is dead code in production handler flow. Malformed tombstone files also cannot fail closed because they are not loaded.
- Recommendation: Load the tombstone index from the canonical repo tombstone directory before constructing the engine; propagate `TombstoneLoadError` as a structured `Refused { reason: Tombstone }` / fail-closed response. Add an end-to-end test that writes a tombstone JSONL rule and proves matching `WriteMemory` is refused, plus malformed JSONL refusal.
- Confidence: High

[P1 / High] Correctness: `memory_supersede` bypasses contradiction/supersession semantics

- Evidence: The Stream C plan says `memory_supersede` must be governed and supersession should update both records only as a valid chain (`docs/plans/2026-04-29-stream-c-governance.md:635-642`, `docs/plans/2026-04-29-stream-c-governance.md:685-691`). The handler evaluates governance but only refuses `GovernanceWriteDecision::Refused` (`crates/memoryd/src/handlers.rs:213-229`), then executes `Substrate::supersede_memory` for every other decision (`crates/memoryd/src/handlers.rs:231-246`).
- Why it matters: A caller can force a supersession even when governance classified the replacement as `Duplicate`, `Refinement`, `Candidate`, `Quarantined`, or plain `Promoted`. That creates supersession edges that do not correspond to an actual contradiction decision and can turn low-confidence/review-required content into an active replacement.
- Reasoning: The governance crate already has `SupersessionPlan::from_contradiction_decision` rejecting non-supersession decisions (`crates/memory-governance/src/supersession.rs:53-66`), but the daemon handler does not use it. `governance_supersede_response` treats “not refused” as enough authorization to mutate both old and new records.
- Recommendation: Require `GovernanceWriteDecision::Supersession { next_action: SupersedeWithChain, existing_id == old_id, ... }` before executing Stream A supersession. Map `Duplicate`, `Refinement`, `Candidate`, `Quarantined`, and `Promoted` to their typed responses without lifecycle mutation. Add e2e tests for each non-supersession decision proving no old record is superseded.
- Confidence: High

[P1 / High] Data Integrity: Partial supersession failure can commit an invalid replacement without repair-required state

- Evidence: The spec requires partial supersession failure to be visible with committed side effects so the daemon can stop lifecycle writes (`docs/specs/stream-c-governance-v0.1.md:181-182`). `Substrate::supersede_memory` writes the replacement first (`crates/memory-substrate/src/api.rs:409-420`), then reads and mutates the old memory (`crates/memory-substrate/src/api.rs:422-444`). If `read_memory(old_id)` fails after the replacement committed, it returns `WriteFailure { outcome: new_outcome.clone(), kind: Validation(...) }` (`crates/memory-substrate/src/api.rs:422-425`) where `new_outcome.repair_required` is normally `None`.
- Why it matters: A bad old id or read failure can leave a new active memory with `supersedes: [missing_old_id]` on disk, but the failure does not mark repair required or record a lifecycle-stop condition. That can break the supersession graph and allow further lifecycle writes on a repo that needs repair.
- Reasoning: The `committed_lifecycle_failure` wrapper is only applied around the second `write_memory` call (`api.rs:432-444`), not around the post-commit old-memory read. The committed side effect is reported, but without durable pending repair metadata or an operator-required repair marker. The daemon maps this to a generic retryable substrate error (`crates/memoryd/src/handlers.rs:1068-1069`) and does not stop lifecycle writes.
- Recommendation: Preflight-read and validate the old memory before writing the replacement, or wrap every post-replacement failure path in a lifecycle-specific failure that sets `repair_required` / durable repair marker. Add a fault/invalid-old-id test proving no replacement is committed, or if committed, repair is durable and subsequent lifecycle writes are blocked until repair.
- Confidence: High

[P2 / Medium] Correctness: Daemon contradiction search quarantines unrelated writes once any active memory exists

- Evidence: The Stream C spec limits similarity retrieval to active memories in the same namespace/entity set (`docs/specs/stream-c-governance-v0.1.md:156-160`). The daemon adapter collects all active memories with similarity `1.0` (`crates/memoryd/src/handlers.rs:522-541`) and returns the first `limit` active memories for every candidate without filtering (`crates/memoryd/src/handlers.rs:562-564`); its tiebreaker always returns `Unclear` (`crates/memoryd/src/handlers.rs:570-573`). The engine maps `Unclear` to quarantine (`crates/memory-governance/src/engine.rs:265-270`).
- Why it matters: After the first active memory exists, unrelated non-duplicate writes can be quarantined purely because some active memory is present, not because it is actually similar or in the same entity set. That makes promotion behavior unstable as the repo grows and turns the review queue into noise.
- Reasoning: `active_memory_summaries` hardcodes `similarity: 1.0`, so `ContradictionDetector::has_above_threshold_hit` is always true for any non-empty hit list (`crates/memory-governance/src/contradiction.rs:335-353`). Because `MemorydSimilaritySearch::top_k` ignores the candidate, the fake production tiebreaker returns `Unclear`, which forces quarantine.
- Recommendation: Until real similarity is available, constrain `top_k` to same namespace and overlapping entity hash/ids, or return no hits except exact duplicate matches. Alternatively surface “provider unavailable” as an explicit policy decision rather than pretending every active memory is a 1.0-similar conflict. Add an e2e test: two unrelated grounded project writes should both be promoted/candidate per policy, not quarantine solely because the first exists.
- Confidence: High

[P2 / Medium] Tests: Review queue derivation does not verify canonical `pending` review-state spelling from the spec

- Evidence: The spec includes `status: candidate` with `review_state: pending` (`docs/specs/stream-c-governance-v0.1.md:190-194`). The projector recognizes `review_state == Some("pending-review")` (`crates/memory-governance/src/review.rs:62-71`), and the contract test uses `pending-review` (`crates/memory-governance/tests/review_queue_contract.rs:8`, `crates/memory-governance/tests/review_queue_contract.rs:71-75`).
- Why it matters: If canonical frontmatter or callers use the spec spelling `pending`, those memories will not appear in review queues unless they are also `candidate && requires_user_confirmation`. This is a visibility bug for human review and a spec/test mismatch.
- Reasoning: The implementation and tests agree with each other but not with the Stream C spec text. The queue is supposed to be canonical-frontmatter-derived; inconsistent spelling at that boundary creates silent omissions.
- Recommendation: Align the spec and implementation on one stable review-state enum/spelling. Prefer typed constants/enums instead of raw strings, and add tests for the canonical pending state plus candidate/quarantined states.
- Confidence: Medium

### Non-blocking simplifications

- `crates/memoryd/src/handlers.rs:174-391` would be safer if decision execution were split into one function per decision family (`execute_promoted`, `execute_duplicate`, `execute_supersession`, etc.). The current large match makes it easy to accidentally treat “not refused” as authorized mutation, as seen in supersede handling.
- Consider sharing the canonical text/entity hashing implementation between `contradiction.rs` and `tombstone.rs`; both currently implement similar FNV-based normalization separately (`crates/memory-governance/src/contradiction.rs:357-382`, `crates/memory-governance/src/tombstone.rs:256-288`). This matters because duplicate and tombstone matching should not drift over time.

### Test gaps

- No daemon e2e test proves malformed `policies/*.yaml` fails closed instead of falling back to built-ins.
- No daemon e2e test proves `tombstones/*.jsonl` is loaded and matching writes are refused; tombstone coverage is crate-local only.
- No supersede e2e tests prove `Duplicate`, `Refinement`, `Candidate`, `Quarantined`, or plain `Promoted` decisions do not mutate supersession state.
- No partial-write/fault-injection test covers replacement-committed / old-read-failed supersession failure semantics or lifecycle stop/repair markers.
- No e2e test proves two unrelated active memories avoid contradiction quarantine when there is no exact duplicate or relevant similarity hit.
- Review queue tests use `pending-review`, not the spec's `pending` wording, and do not test structured source/evidence fields promised by `docs/specs/stream-c-governance-v0.1.md:200-207`.

### Questions / uncertainties

- The Stream C spec says `review_state: pending`, while tests and implementation use `pending-review` and candidate/quarantined string states. I treated this as a correctness/test mismatch, but the intended canonical spelling should be decided before changing code.
- I did not run the full Rust gate because this lane is correctness review-focused and the tree contains broad dirty user/work-in-progress changes. I only wrote this report and verified its existence with the requested command.

### Positives

- The crate boundary is directionally sound: `memory-governance` returns typed decisions and does not directly mutate files.
- The contradiction detector has deterministic traits and tests that prove exact duplicates avoid tiebreaker calls.
- Review queue data is derived from frontmatter/envelopes rather than a hidden side database, matching the intended architecture.

## P0/P1/P2 summary

- P0: 0
- P1: 4 — policy fail-open fallback; tombstone rules not loaded by live daemon; `memory_supersede` bypasses supersession decision semantics; partial supersession failure lacks durable repair/stop semantics.
- P2: 2 — daemon contradiction adapter quarantines unrelated writes; review queue pending-state spelling/test mismatch.
