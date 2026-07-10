# Round-2 triage — plan r3 → r4 (2026-07-10)

Reviewers: native Opus plan-reviewer (persistent, re-attack), Cursor/Grok-4.5 (`cursor-28`), Muse Spark 1.1 (`opencode-1`, comparative slot). All three independently confirmed round-1 fixes landed; all three independently found the same structural trio the round-1 fixes created. Convergence table:

| Defect | Opus | Cursor | Muse |
| --- | --- | --- | --- |
| Abstraction/cue *generation* job owned by no wave; eval lanes empty → W4 gate placebo | NB1 | B1 | B2 + M(W2-job) |
| Multi-source merge "atomic" unbacked by substrate (supersede is 1→1; no fs+git+index txn) | NB2 | B2 | M(merge) |
| API-lane 750ms embed inside 800ms hook → residue ~50ms, 4-lane fusion infeasible at tail | M1 | B3 | M(latency) |
| W0 "fixture ClassificationOutcome" not a real write-DTO knob | — | M1 | B1 |

Coordinator verification: supersede 1→1 confirmed (`supersession.rs` SupersessionPlan old_id/new_id); 750/800 constants confirmed (`recall/config.rs:22`, `recall_hook.rs:37`); `WriteRequest` carries meta hints only — classification is derived in the governance handler (Muse/Cursor correct).

## Dispositions — blockers & structural

| Finding | Disposition |
| --- | --- |
| Generation-job orphan (all three) | ACCEPT — new W2 deliverable 6: `abstraction_compile` dream job (harness-CLI dream pass, machine-verified constraints, governed writes, structural fallback from `summary` when no harness available). W4 gains an explicit **eval-corpus backfill prep step** using the same job (parity with production writes); baseline recorded pre- and post-backfill. Live W5 explicitly NOT a prerequisite for W3/W4 gates. Wave graph redrawn. |
| Merge "atomic" unbacked (all three) | ACCEPT — W3 spec task rewritten: the word "atomic" dropped; approval apply = journaled, idempotent, resumable; pre-flight re-validation of ALL source statuses at approval time; defined ordering (stage replacement → supersede sources → activate last); crash recovery wired into startup/dream reconcile; event kinds, tombstone/claim-lock interaction, provenance union, strictest-classification carry, two-clone idempotent replay all specified. A true multi-supersede substrate primitive (one git commit + one index txn) is listed as the alternative the spec task must decide on — budgeted as Stream A work if chosen. |
| API-lane latency (all three) | ACCEPT — pre-decided in r4: API-lane hook path (prompt/desk passive recall) keeps today's lanes (FTS + chunk-vector under existing budgets); the new abstraction/cue lanes serve the local-lane hook path (shared-deadline formula: embed_budget = min(lane timeout, deadline − measured fusion reserve)) and the `memoryd search` pull path (own, longer timeout). Work-stream cue removed from the gate (owned by v4 P2, unwired). |
| W0 classification mechanics (Cursor M1, Muse B1) | ACCEPT — ingestion contract rewritten: sensitivity is an *expected* value asserted against the classifier's outcome (mismatch = counted, reported); provenance via `SourceKind::User` + `explicit_user_context=true` for dialogue turns, `AgentPrimary` + pinned `file:` artifact otherwise; auto-`review approve` loop for the eval tree; quarantine floor evaluated per source kind. No "fixture ClassificationOutcome injection" language. |

## Dispositions — majors

| Finding | Disposition |
| --- | --- |
| W2 spec-list gaps: multi-table KNN query APIs + worker drains all row kinds (Muse) | ACCEPT — W2 task 2 extended (query path is part of the lifecycle). |
| Privacy composition: enumerate ALL write entrypoints incl. `review approve` (Trusted today), dream writes, upgrade path revoking API vectors (Muse) | ACCEPT — W2 task 3 rewritten with entrypoint enumeration + sensitivity-upgrade → revoke API vectors + re-enqueue held-local + drop-abstraction-keep-body fail-closed rule. |
| Schema-6 mechanics: additive `CREATE TABLE IF NOT EXISTS`, table-exists guards, doctor cross-checks, explicit renumber-before-code (Muse; Cursor numbering) | ACCEPT — W2 task 4 extended; decision-point numbering fixed (body now cites DP2). |
| W3 fence gaps: exclude candidate/quarantined/encrypted/pending-review/backfill-manifest; sensitivity-compat = no-downgrade; live NN-distance histogram (Muse) | ACCEPT — fence list expanded. |
| W3-dark-until-W5 + W1→W3 edge is data-not-build dependency (Opus M2/NIT) | ACCEPT — stated; diagram relabeled. |
| W3/W4 tuning edges need abstraction-populated eval tree (Cursor) | ACCEPT — explicit edges in diagram. |
| FTS deliberately not extended to abstractions; rendering unchanged; row kinds derived/rebuildable, never synced (Opus minors, Cursor M2) | ACCEPT — stated as decisions in W2/W4. |
| Cue merge rule: union → case-insensitive dedup → deterministic sort → truncate to 3, ours-first (Muse) | ACCEPT — replaces bare "set-union". |
| Findings artifacts gitignored = history loss (Muse) | ACCEPT — tracked review artifacts live in `docs/reviews/memora-arc/`; `thoughts/` stays scratch-only. |
| Cargo.lock rebase procedure for W0∥W1 (Cursor, Muse) | ACCEPT — one-line contract added (second lane `cargo build --locked`; coordinator resolves with targeted `cargo update -p`). |
| Devin refuses empty accepted-findings lists (Cursor) | ACCEPT — added to review-cycle protocol. |
| W0 grounding open follow-up interaction (Muse minor) | ACCEPT — noted; the CLAUDE.md grounding catch-22 follow-up is cross-referenced, not solved here. |

## Rejections / choices between offered fixes

- Muse's "make W0 depend on W2" restructure — NOT taken; chose the lighter W4-prep eval-backfill step (keeps W0∥W1 parallelism; W0's raw baseline is still valuable as the pre-abstraction control).
- Cursor's "raise hook deadline for fusion-bearing cues" — NOT taken as default; chose lane-split (API hook path unchanged) as the lower-risk pre-decision. Deadline raise remains an option inside the W4 spec if local-lane measurement demands it.
- Opus NB2's "add a genuine multi-supersede transaction primitive" — neither accepted nor rejected: it is the explicit alternative the W3 ratified-spec task must decide, with Stream A budget consequences stated.

Verdict carried into r4: Cursor/Muse "another structural pass" honored via round 3 (Sol xhigh convergence re-read of r4) before readiness is declared.
