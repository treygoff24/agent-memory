# Findings triage — W3 merge-proposal spec, round 1

Reviewer: Luna high (codex-78, main scope, group memora-w3spec). Spec at commit `aa6864f`. Verdict NOT-RATIFIABLE; **11/11 accepted**. Revisions applied by the coordinator as spec r2 (specs are coordinator-owned).

| # | Sev | Finding | Disposition |
| --- | --- | --- | --- |
| 1 | BLOCKER | Staged replacements visible to FTS fallback + delta recall (`query_chunks` filters neither status nor the staging marker) | ACCEPTED — central non-servability predicate required, applied to every read lane (hybrid, FTS-only degraded, delta recall, startup); forced-FTS + delta tests in the gate |
| 2 | BLOCKER | Specified transitions rejected by shipped `validate.rs` (candidate→active, pinned→superseded); rollback loses original status/trust | ACCEPTED — explicit transition-matrix amendment; journal captures each source's original (status, trust_level) tuple and rollback restores it; replacement trust set explicitly on activation |
| 3 | BLOCKER | No concurrency fence — two proposals can share a source; rollback can steal ownership | ACCEPTED — single merge-apply mutex (one apply at a time, dream-scheduler serialized) + every supersede/rollback is CAS on (base_hash, status, superseded_by == this proposal); claim-locked sources rejected at validation |
| 4 | BLOCKER | Spec references per-source supersession events that don't exist (`Superseded` deferred; no `MergeApplied`) | ACCEPTED — spec now *defines* the typed events it needs (MergeProposalApplied terminal + per-source entries carried in it) instead of citing phantom ones; event-mirror + replay semantics stated |
| 5 | MAJOR | Rollback can overwrite a post-supersede edit (no hash guard on the superseded state) | ACCEPTED — rollback CAS against proposal-owned superseded state; on mismatch: stop, quarantine the proposal, operator repair — never overwrite |
| 6 | MAJOR | Review fences don't match shipped review semantics (three pending spellings; no pinned-approval field in the CLI DTO) | ACCEPTED — reuse the shared review-membership predicate; `review merges approve` gains an explicit `--approve-pinned <id>` acknowledgment |
| 7 | MAJOR | Aux-vector behavior for staged candidates undefined (W2 pipeline would enqueue rows the lifecycle matrix deletes) | ACCEPTED — staging creates NO aux rows/jobs/vectors; aux enqueue happens at activation; rollback tombstoning purges any derived aux state |
| 8 | MAJOR | Journal not fail-closed (no framing, checksums, torn-line rule, startup reconcile hook) | ACCEPTED — framed JSONL with per-record sha256, torn/corrupt tail ⇒ fail-closed (proposal quarantined, no partial apply), reconcile runs in daemon startup before any read serving |
| 9 | MAJOR | Mid-apply availability gap (sources superseded before replacement active) never ratified | ACCEPTED — explicit decision recorded: gap accepted (single device, no awaits between phases, bounded to fs-ops duration), crash inside the gap covered by forward-resume; test asserts post-crash availability |
| 10 | MAJOR | Plan/spec drift on two-clone + N=1 ("rewrite-in-place" would violate self-reference validation) | ACCEPTED — ratified: N=1 uses new-ID supersession like N>1 (no in-place rewrite); two-clone contract = proposals device-local, only results sync, idempotent replay keyed by proposal id |
| 11 | MINOR | Direct `memoryd get` by ID exposes staged replacement | ACCEPTED — documented as intentional: direct get is the inspection surface the review CLI itself needs; staged candidates carry the marker in the returned envelope |

Round 2: Sol high convergence re-read over W2 package r3 + W3 spec r2 together (the planned round-3 bookend from findings-w2spec-r1.md), then both go to Trey for ratification.
