# B3 spec amendment — review round 1 triage (2026-07-15)

Reviewers: Grok 4.5 (cursor-1, safe, on the Terra worktree) + native Opus subagent. Author: Terra
(codex-88). Coordinator triage below; fix round assigned to Terra in the same worktree.

## Accepted — BLOCKER

- **B3S-1 (Grok #1): contradicts live §8.7 generation-context drop/rebind.** §8.7 mandates
  drop-fields-keep-body + outcome rebind for generation writes (naming dream compile/backfill);
  the amendment mandates typed tier-increase refusal at the same entrypoint. Fix: explicit carve-out
  in the B3 block naming §8.7 — drop/rebind applies only to create/supersede generation writes;
  in-place `metadata_amend` refuses instead — and amend §8.7's entrypoint sentence accordingly.
- **B3S-2 (Grok #2): Stream F drift.** `stream-f-dreaming-v0.3.md` Amendment 2026-07-11 still
  defines apply = governed supersede + dual-classify drop/rebind. Fix: matching dated Stream F
  amendment — apply path becomes B3 `metadata_amend`; supersede/drop-rebind language replaced.
- **B3S-3 (Grok #3 + Opus M1, convergent on clause 3): "ordinary CAS write path" is wrong twice.**
  It would emit `WriteCommitted` (not one `MetadataAmended`) and unconditionally bumps
  `updated_at` while clause 1 declares timestamps immutable. Fix: specify a dedicated thin amend
  write (CAS write + index + exactly one `MetadataAmended`); timestamps: `created_at` immutable,
  `updated_at` bumps — semantically correct and consistent with the W2 abstraction-merge
  updated_at+sha256 tie-break (an amended abstraction IS fresher); design note updated to match.
- **B3S-4 (Grok #4 + Opus M2, convergent): encrypted-row combined scan unimplementable.**
  Ciphertext isn't scannable and no plaintext body exists without reveal. Encrypted rows STAY in
  scope (the backfill targets 118/915 encrypted rows; excluding them recreates the B1 class). Fix
  (Opus's shape): mandated scan = proposed abstraction/cues always (that's where a leak surfaces —
  they land as plaintext frontmatter) + body/summary/tags only where plaintext is available;
  explicit no-decrypt invariant — encrypted body out of scan scope by construction.

## Accepted — MAJOR

- **B3S-5 (Grok #5): idempotent short-circuit must not precede CAS.** Always compare
  `expected_base_hash` first; mismatch → `MetadataAmendmentStaleBase` even when values look
  identical.
- **B3S-6 (Grok #6; adjudicated against Opus axis-6): honest framing.** Swapping the shipped
  `abstraction_compile` apply from governed supersede to in-place amend is a behavior + report-shape
  change to an existing surface, not additive. Fix: relabel as a **Trey-authorized behavior-change
  amendment (2026-07-15, in-version per explicit authorization — see decision record)** with a
  Touches list (§8.7, §12, Stream F amendment, CLI contract §8 report).
- **B3S-7 (Grok #7): don't redefine ratified Amendment 19.** `update_encrypted_memory_metadata`
  keeps its `(id, actor, mutate)` shape. Worker-hash comparison happens INSIDE the mutate closure
  (closure-revalidation pattern, per the B1 F1 TOCTOU fix precedent) so the primitive's own
  fresh-read CAS covers it; handler appends `MetadataAmended` after success.
- **B3S-8 (Grok #8 + Opus m2): pin the report vocabulary.** CLI §8 gains the outcome table:
  `(outcome, new_id, reason, applied/skipped counter)` rows for `amended` / `unchanged` / each
  typed refusal / validation skip; snake_case wire reasons pinned to the typed variant names.
- **B3S-9 (Grok #9 + Opus m1, convergent): cite the right classifier.** Spec names
  `classify_plaintext_memory` (derives `PrivacyNamespace` from stored scope, User→Me — the floor
  discipline) as the composition, extended to include proposed abstraction/cues in the scanned
  payload; drop the `GovernanceWriteInput`/`classify_input_privacy` citation and the nonexistent
  `title` field. Code-wave note: it returns generic invalid_request on encryption-required today —
  must be mapped to `MetadataAmendmentTierIncreaseRefused`.
- **B3S-10 (Grok #10): closed refusal set.** Enumerate: stale base, tier increase, validation/cap
  failure, missing id, actor mismatch, secret refusal (relation to `WriteFailureKind::SecretRefused`
  stated), lifecycle-not-active. CLI reasons = snake_case of each.

## Accepted — MINOR / NIT

- **B3S-11 (Grok #11):** drop the vacuous "names any other field" clause (fixed request shape).
- **B3S-12 (Grok #12):** remove caller-supplied `actor` from the request; hardcoded at the
  compile→handler boundary per review/reality-check precedent.
- **B3S-13 (Grok #13):** move the block under the spec's `## Amendments` discipline with
  Touches/rationale format.
- **B3S-14 (Opus n1):** design-note line refs: review.rs actor-arm precedent is at :327/:443.

## Rejected

- None. Opus's clean-axis calls on aux fence (§10.2.1 edit-row mapping), §12 event addition, and
  symbol implementability stand as verified.
