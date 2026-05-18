# Wright loop — gap analysis from the export v0.1 post-ship review

**Date:** 2026-05-18
**Source incident:** code review of the four wright-loop trial commits (`0542bb3` → `4969f57`) found **5 blockers, 10 design issues, 4 nits**. All four items had `wright verify` PASS. All four commits landed on the `feature/tiered-gate-dashboard-workflow` feature branch. The bugs reached the reviewed branch because wright's "verify" bar was, in practice, "did the one acceptance test pass."
**Audience:** anyone designing wright M2 (or M3). The dogfood-learnings doc at `docs/handoffs/2026-05-16-wright-first-dogfood.md` covered what *worked* in the M1 loop; this covers what didn't and why.

This is a design-input document. Concrete proposals are in §5–§7; they're starting points, not commitments.

---

## 1. What slipped through verify=PASS

Five contract-level bugs landed on the reviewed feature branch with all four wright items marked `implemented`:

| ID | Bug | How it got through verify |
| --- | --- | --- |
| **B1** | `--since 2026-05-01T00:00:00+00:00` (offset-qualified RFC3339) rejected with exit 2, contradicting spec §5 "Accepts the canonical RFC3339 form (`...Z` or `...+00:00`)." | Acceptance closure for item 02 only spelled out the `Z` form. Implementer wrote the minimal test to satisfy the closure. The `+00:00` form was never exercised. |
| **B2** | Unreadable envelopes silently dropped from the export; `memory_count` shrinks, no stderr diagnostic, no exit code change. Implementer wrote `Err(_) => None` to be "defensive." | Acceptance test fixtures only contain readable envelopes. No spec text required a corrupt-file test. No structural lint flagged the implementer-introduced silent-error path. |
| **B3** | Dead/no-op timestamp conditional (`if created_at == "1970-01-01T00:00:00Z" { epoch.clone() } else { created_at }` — both arms produce the same string). Either the author intended `format_rfc3339_millis` on one side, or this is straight dead code. | `cargo clippy` was never run as part of verify. Clippy's `if_same_then_else` catches this pattern. The targeted `cargo test` passes because no test exercises the branch. |
| **B4** | `source_device_id` silently becomes `""` when device config is missing (`.unwrap_or_default()` on `Result<Option<...>, _>::Ok(None)`). A fresh-clone export emits an empty device id with no error. | Test fixture always sets a device id. Acceptance closure said "matches the fixture's device id, non-empty" — vacuously passes when the fixture is the source. The "what about absent state" case lives outside the closure. |
| **B5** | Spec §7 MUST: "Acknowledge the substrate open side effects in the user-facing docs (README / help text)." Help text says only `"Export substrate contents as a portable JSON snapshot."` — no mention of runtime-dir creation, index repair, event-log rebuild. No README change. | This MUST lived in §7 (Implementation boundaries), not §8 (Acceptance items). Wright items 01–04 each cover one §8 closure; nothing maps to §7's MUSTs. The "every spec MUST has an item that covers it" check doesn't exist. |

Plus ten design/idiom issues — async-blocking sync IO, 140-line function, 280 LOC of duplicate test fixtures across four files, stringified errors, etc. Those are quality issues a code reviewer would catch in seconds. None of them were caught because **no code review step exists in the wright loop**.

The single root cause: **`wright verify` = "the one acceptance test passed." That bar is too low.**

---

## 2. Why each bug got through

Per-bug walk-through. The point isn't to relitigate the bugs; it's to identify the *class* of gap each one represents, because the wright-loop redesign should target classes, not instances.

### B1 — Spec contract surface wider than the test

Spec §5 enumerates two valid forms for `--since`: `2026-05-01T00:00:00Z` and `2026-05-01T00:00:00+00:00`. The wright item-02 closure quoted *one* of them as an example. The implementer wrote one test row. The other valid form was never executed against the binary.

**Class:** *Closure-narrower-than-spec.* When an acceptance closure cites a concrete value from the spec, the spec usually states or implies a *set* of valid values; the closure pulls a single representative. The implementer reads the closure, writes the minimal test that satisfies it, and the rest of the contract envelope goes unverified.

**Implementer's belief:** "I wrote a test that matches what the closure asks for, so the contract is closed."
**Reality:** the closure was a fragment of the contract, not the whole thing.

### B2 — Implementer-introduced silent failure path

The substrate iterator returns `Result<MemoryEnvelope, _>` per file. The implementer chose to `Err(_) => None` — a defensive shortcut that converts "any read error" into "exclude this row from the export." Nothing in the spec covers what should happen on a corrupt file. Nothing in the test fixtures includes a corrupt file. Wright has no way to know this branch exists.

**Class:** *Silent-failure introduction.* The implementer adds a `.unwrap_or`, `.ok()`, `Err(_) => None`, or similar Result-eating construct to satisfy the type system without writing the unhappy-path logic. The acceptance test passes (the path is never taken). The unhappy case ships as silent data loss.

**Implementer's belief:** "Skipping unreadable files is safer than failing the export."
**Reality:** silently underreporting `memory_count` is *less* safe than an explicit error, because the consumer has no signal anything went wrong.

### B3 — No static-analysis gate

```rust
let created_at = if created_at == "1970-01-01T00:00:00Z" { epoch.clone() } else { created_at };
```

Both arms are the same string. Clippy's `if_same_then_else` lint catches this in milliseconds. The wright-M1 verify command was `cargo test -p memoryd --test export_json_shape` — no clippy, no fmt-check, no rustdoc, no nothing.

**Class:** *Quality gate is "test passes" only.* Wright M1 trusts the implementer to have run their own clippy. In practice an LLM implementer racing against a stream-idle timeout doesn't.

### B4 — Edge case outside the fixture surface

`load_local_device_config` returns `Result<Option<DeviceConfig>, _>`. The implementer handled `Err` (good) and `Ok(None)` (silently → empty string, bad). The fixture's `Substrate::init` always sets a device id, so `Ok(None)` is never hit during the test. The acceptance closure says "non-empty" — vacuously satisfied.

**Class:** *Acceptance fixture is the only execution path.* If the test only writes one shape of input, the implementer's branch-coverage is whatever the test happens to hit. The Option arms / Result arms / error paths that lie outside the fixture are unverified.

This compounds with B1 (which is the same gap, viewed from the spec side rather than the fixture side). The two together suggest a structural property: **the acceptance test is a single point, and the spec contract is a region; the gap between them is where bugs live.**

### B5 — Spec MUSTs outside §8 don't map to items

The export spec is sectioned: §1 Goal, §2 Non-goals, §3 CLI surface, §4 Output schema, §5 Filters, §6 Encrypted handling, §7 Implementation boundaries (with MUSTs and MAYs), §8 Acceptance items, §9 Open questions.

Items 01–04 live in §8 and each closes one §8 closure. §7 contains *its own* MUSTs — including "Acknowledge the substrate open side effects in the user-facing docs (README / help text)." That MUST has no item, was not in any closure, and never got done.

**Class:** *Spec-coverage hole.* MUSTs scattered through non-acceptance sections of the spec have no item to map to. The implementer reads the item closure, not the whole spec. Wright doesn't compute "is every spec MUST claimed by some acceptance closure?" — so MUSTs outside §8 silently fall off the floor.

### I-series — no code review

Issues like 140-line functions, sync IO inside `async fn`, four near-duplicate fixture builders, `.unwrap_or` on infallible Serialize calls — these aren't contract bugs, they're craft bugs. They're exactly what a code reviewer catches in five minutes. The wright loop has no reviewer phase. Item commits land directly on the branch after verify passes.

---

## 3. Recurring gap patterns

Distilling §2 down:

**P1. The acceptance test is one point in a contract region.** Whatever the test fixture happens to exercise is the only path the implementer must satisfy. The spec contract is wider than that point in all five blocker cases.

**P2. Wright trusts implementer self-discipline for quality.** No clippy, no fmt, no doc check, no review. The M1 design said "the verifier is the test," and the test is whatever the item file declares. There's no second opinion.

**P3. Acceptance items are a fragmented projection of the spec.** Items live in §8. MUSTs scattered through §7, §3, §4, §5, §6 don't have an item per MUST. Authors mostly remember to lift them into closures, but the export spec demonstrably missed §7's "acknowledge in docs" MUST.

**P4. Implementer-introduced control flow is invisible.** When the implementer adds a Result-eating construct, a default-on-None, a panic-replaced-by-empty-value, the diff has it but verify doesn't see it. There's no lint, no review, no spec citation requirement.

**P5. The wright item is closed by a single signal (test passes). No fix loop.** Today's loop: claim → implement → verify → done. If verify passes, item is `implemented`, full stop. There's no "verify-then-review-then-fix-then-re-verify" structure. The single PASS terminates the loop.

P5 is the centerpiece for the user's "must require code review and fix loops before exit" requirement — it's the structural cause of every other gap shipping past wright's bar. Once the loop is closed (PASS → implemented → next item), there's nowhere for a reviewer's findings to go.

---

## 4. What's load-bearing about the M1 design we should NOT change

Before proposing changes, stake out the things that worked and should survive M2:

- **External durable queue + atomic per-item work.** The handoff doc at `2026-05-16-wright-first-dogfood.md` §2 is right — this shape held under real work. Don't dissolve the per-item boundary.
- **The context bundle as a single primitive.** Spec quote + source coordinates + acceptance tests + test command + scope hint, emitted by `wright claim <id>`, is the right LLM-facing surface. Don't change.
- **Verifier-decided closure, not agent self-assessment.** The verifier flips the status, not the agent. The principle is correct; the problem is that the verifier's evidence (a single test) is too thin, not that the principle is wrong.
- **Dependency ordering on items.** Worked first try. Keep.
- **The state machine as the source of truth.** `Approved → Claimed → Implemented → Verified → Regressed` (the last one fixed mid-flight per the prior handoff). Keep.

The M2 changes should *extend* the verifier and *insert* phases between claim and done — not redesign the queue.

---

## 5. Proposed wright-loop changes

Listed in order of how much they address the gap classes from §3. **C1 is the centerpiece** and is what the user asked for. C2–C7 are the supporting structural changes — none individually as big as C1, but together they close the contract-surface gap, the static-analysis gap, and the spec-coverage gap.

### C1. Mandatory review + fix loop before `implemented`

**Goal:** close P5. No item exits to `implemented` on a single PASS.

**New state-machine shape:**

```
Approved
  → Claimed
    → ImplementedDraft       (test passes, but item is not closed)
      → InReview              (reviewer subagent dispatched)
        → ReviewBlocked       (reviewer found blockers; item remains open and re-claimable)
        → ReviewApproved      (reviewer found no blockers OR all blockers resolved)
          → Verified          (final verify pass over the post-review state)
            → Implemented     (terminal)
```

The new states are `ImplementedDraft`, `InReview`, `ReviewBlocked`, `ReviewApproved`.

**`wright verify` behavior change:**

- On test PASS, status moves to `ImplementedDraft`, not `Implemented`.
- A second command, `wright review <id>`, is required. It spawns a reviewer agent (see C2), captures structured findings to `.wright/acceptance/<id>/reviews/<timestamp>.json`, and flips status to `ReviewApproved` (no blockers) or `ReviewBlocked` (blockers; release lock; item remains open and re-claimable with `regression_reason: "review found blockers"`).
- On `ReviewBlocked`, the next `wright claim` returns the same item with the prior review report appended to the context bundle. Implementer iterates. New verify PASS → new review pass.
- `wright done` only counts items in terminal `Implemented`. `ImplementedDraft` / `InReview` / `ReviewBlocked` / `ReviewApproved` are open.

**Reviewer-blocker definition (machine-readable):**

```json
{
  "review_id": "rev_01HYZ...",
  "item_id": "export-json-shape-01",
  "reviewed_commit": "0542bb3",
  "reviewer": "claude-opus-4-7-review",
  "verdict": "blocked",
  "blockers": [
    {"severity": "spec_contract", "spec_ref": "feature-memoryd-export-v0.1.md#5",
     "summary": "--since +00:00 parsing broken; spec §5 requires both Z and offset forms",
     "file": "crates/memoryd/src/export.rs", "line_start": 134, "line_end": 150},
    ...
  ],
  "issues": [...],
  "nits": [...]
}
```

A `blockers` array with `length > 0` flips the status to `ReviewBlocked`. Issues and nits are non-blocking but surfaced to the implementer on the next claim.

**Iteration ceiling.** After N (3? 5?) review-blocked iterations on the same item, escalate: status → `EscalatedToHuman`, queue surface flags it. Prevents infinite loops where the implementer can't satisfy the reviewer.

**Why this addresses the centerpiece:** every blocker found in the export-v0.1 review (B1-B5) would have been caught by a competent reviewer pass with the spec quoted at it. P5 is the structural cause: there's no place for review findings to go in M1. C1 creates that place.

### C2. Adversarial reviewer subagent, like `plan-reviewer`

The `plan-reviewer` agent in `~/.claude-shared/agents/` is the template. It runs in fresh context, gets briefed with: the spec section being closed, the diff, the prior verifications, and the wright item file. It produces the JSON above.

**Why fresh context:** the implementer agent is biased by the path it walked to get the test passing. A fresh reviewer with the spec in hand and no implementation memory finds the gap between spec and code. This is exactly what plan-reviewer does today for plan documents.

**Quality bar for the reviewer:** the spec quote in the context bundle should be the entire spec section the item draws from — not just the §8 closure. The reviewer should be instructed to compare the implementation against the spec wholesale ("did the implementer satisfy §5 for *all* forms named in §5, not just the one in the §8 closure?").

**Cost discipline.** Reviews cost tokens. Default to opus (fresh-context bias matters more than speed). Cache the spec quote across all four items in a feature so it's one cache hit per feature, not per item.

### C3. Static-analysis gate as part of verify

**Goal:** close P2 for the cheap-to-catch class (B3, plus most I-series Rust idiom issues).

Wright item files declare a stack (`rust`, `ts`, `python`, …). For `rust`, verify runs in sequence:

1. Formatting — use `rustfmt --check <touched-rust-files>` for item-local Rust file checks, or `cargo fmt --all -- --check` when the item touches enough Rust surface that package/workspace formatting is the safer boundary.
2. `cargo clippy --all-targets -p <touched-crate> -- -D warnings` — only the crate(s) touched by the item.
3. The item's `test_command`.

Failure on any of the three flips status to `Regressed`, releases the lock, and leaves the item re-claimable. The reviewer subagent doesn't run until static gates pass — keep token-cost for high-value review work.

**Scope discipline:** clippy on the full workspace would be slow and noisy. Clippy on just the crate the item touched is fast and contained. Per CLAUDE.md gate policy this is the "fast inner loop" tier.

Non-Rust support follows the same shape: a `stack: typescript` item runs `pnpm run check:fast` for the package; `stack: python` runs `ruff` + `pytest`.

### C4. Spec-coverage check at feature open and feature close

**Goal:** close P3.

At `wright ingest` time (feature start), the ingester parses the spec for MUST / MUST NOT clauses (a regex pass, plus light heuristics — "the export MUST", "MUST NOT", "is required to"). For each clause it asks the spec author to map it to either:

- An acceptance item (`covered_by: ["item-id"]`), OR
- An explicit `not_an_acceptance_target` field on the spec section ("this MUST is covered by other means: ___").

`wright ingest` exits non-zero if any MUST is unmapped. The export-v0.1 spec would have flagged §7's "Acknowledge the substrate open side effects" MUST: no item covered it.

At `wright done` time (feature close), the same check runs in reverse: for every claimed MUST, the diff covering its item must touch some file that plausibly satisfies it. For "user-facing docs" MUSTs, a help-text or README file glob. For "no Stream D reveal path invoked" MUSTs, an absence-of-symbol grep.

This is the load-bearing piece for B5. The check is heuristic, not airtight — but raising the question "did anyone close §7 MUSTs?" at ingest time is enough.

### C5. Silent-failure lint

**Goal:** close P4.

A simple grep-based lint that runs on the item's diff:

- `Err(_) =>` → require either a comment with a spec section reference, or an acceptance entry on the reviewer's checklist.
- `.unwrap_or(` / `.unwrap_or_default()` / `.unwrap_or_else(` → same.
- `.ok()` on a `Result` → same.
- `let _ = <expr>?` or similar fire-and-forget patterns.

The implementer can suppress per-occurrence with a structured comment:

```rust
// wright-silent: spec §6 — corrupt envelope deliberately skipped, see review rev_01HYZ
.filter_map(|result| match result { Err(_) => None, Ok(env) => Some(env) })
```

Without the marker, the lint fails the item. With the marker, it requires the reviewer to actively bless the silent failure.

**Why heuristic-grep, not a real Rust analyzer:** the goal is to surface a discussion, not to be airtight. False positives are fine — the implementer adds a marker or refactors. False negatives are the expensive case; the reviewer subagent catches what the lint misses.

This would have caught B2 directly (the `Err(_) => None` line had no comment) and B4 (the `.unwrap_or_default()` on `source_device_id` would have flagged).

### C6. Spec-contract envelope fan-out at item-author time

**Goal:** close P1.

When the spec author writes an acceptance closure that cites a concrete value, the wright item-author tool prompts: "Spec §5 enumerates these valid forms: `Z`, `+00:00`. Your closure cites `Z`. Add the others to the test envelope?"

Mechanism: at `wright ingest`, the ingester scans each item's closure for spec citations (regex / known-spec-anchor pattern). For each citation, it pulls the surrounding spec sentence and looks for "or", "either", "any of" — i.e. enumerations of valid values. It surfaces them to the spec author for inclusion.

This is the most ambitious of the proposals; it crosses into "wright understands the spec" territory which is hard. Lighter version: the reviewer subagent (C2) gets the *entire* relevant spec section, not just the §8 closure, and is asked "what valid forms does §5 mandate that the test does NOT exercise?" The reviewer answer becomes a soft blocker for the next iteration.

I think C2 + C5 together cover most of P1 in practice. C6 is the formal version if we later find we want it.

### C7. Postmortem auto-emit

**Goal:** make M2's design decisions self-evidencing.

At `wright done` for a feature, wright emits a markdown skeleton under `docs/handoffs/<date>-<feature>-postmortem.md` with:

- Per-item iteration counts (today's `2026-05-16-wright-first-dogfood.md` §4 — but auto-generated, not hand-typed).
- Total tokens spent on the implementer, reviewer, and verifier.
- Findings counts: blockers raised, blockers resolved, blockers escalated.
- Static-gate runs: clippy warnings caught, fmt diffs, lint flags.
- Spec-coverage final state: which MUSTs covered, which closed via `not_an_acceptance_target`.

The current handoffs are written by hand after the fact. Auto-emission means M3 designers have a structured corpus, not anecdotes.

---

## 6. The minimum-viable change

The user said: *"at a minimum, it should absolutely require code review and fix loops before it's allowed to exit somehow."*

That's **C1 + C2 + C5**, in that order:

1. **C1 — State machine.** Add `ImplementedDraft`, `InReview`, `ReviewBlocked`, `ReviewApproved`. `wright verify` PASS goes to `ImplementedDraft`, not `Implemented`. `wright review <id>` is required to reach `Implemented`.
2. **C2 — Reviewer subagent.** Implement the adversarial-review pass. Without C2, C1 is just renamed states.
3. **C5 — Silent-failure lint.** Cheap; addresses two of five blockers; catches the class of "implementer added a defensive shortcut" that no test-based gate ever catches.

C3 (clippy/fmt) is a near-free add and probably belongs in the same M2 cut — it catches B3 in milliseconds for zero token cost. Realistically: **C1 + C2 + C3 + C5** is the M2 floor.

C4 (spec coverage) and C6 (envelope fan-out) are heavier lifts. They address real classes (P1, P3) but are more design work and could go to M3.

C7 (postmortem) is mostly mechanical and could go in any release. It's a quality-of-life win for the designers of M3.

---

## 7. Open design questions for Trey

These are the calls I don't think wright-M2 can make without you:

**Q1. Reviewer model choice.** Opus 4.7 (1M ctx) is the natural default — fresh context, high quality. But every item runs a review. Token cost adds up. Sonnet 4.6 for first-pass review, opus only on escalation? Or always opus and accept the cost as the price of correctness?

**Q2. Iteration ceiling.** When the reviewer blocks N times on the same item, what's N, and what happens at N? Hard escalation to a human, or a "lower the review bar after N attempts" mode? My instinct is hard escalation — silent bar-lowering reintroduces the "we shipped wrong code because the loop got tired" failure mode.

**Q3. Spec-MUST coverage strictness.** Should `wright ingest` *block* on unmapped MUSTs (export-v0.1 wouldn't have ingested without §7's docs MUST being addressed), or warn-and-proceed? Hard block is the right shape if we trust the heuristic; warn-only is the right shape if we don't.

**Q4. Silent-failure lint scope.** Run only on the diff (cheap, false-negative-prone for refactors that move silent paths around) or on the full crate (slower, catches more). My instinct: diff-only for M2, full-crate as an opt-in `wright lint --full` for periodic sweeps.

**Q5. Where does the reviewer's blocker text live?** Inline as commit-message trailers? As `.wright/acceptance/<id>/reviews/*.json` (structured) plus a generated review summary in the commit message? Both? The trail-of-evidence matters for postmortems.

**Q6. Does the reviewer pass become part of the agent-memory dogfood?** Each review is a `memory_observe` candidate — "what surface gap was caught by review on item-X?" Over time this becomes substrate for the kind of "did wright's reviewer drift?" pattern-detection a Memorum recall block could surface. Maybe a v3 feature, but worth thinking about now.

---

## 8. Summary

The wright M1 loop shipped four acceptance items. All four passed `wright verify`. Code review of the resulting branch found 5 spec-contract blockers, 10 design issues, and 4 nits. None of the blockers were exotic; the four most-impactful (B1-B4) would have been caught by a 5-minute reviewer pass plus `cargo clippy`. The fifth (B5) was a §7 MUST orphaned from the §8 acceptance items.

The structural cause is single-signal verify: `cargo test` PASS → item done. The M1 design intentionally trusted the test as the closure mechanism. That trust held for *mechanical correctness* (the test really does pass) but didn't transfer to *contract correctness* (the test covers one point in a region).

The minimum fix is **mandatory code review + fix loop**: insert `InReview` between `ImplementedDraft` and `Implemented`, dispatch an adversarial reviewer with the full spec section in hand, and require all blockers to clear before the item closes. Add `cargo clippy` and a silent-failure lint as cheap pre-review gates.

Everything else in §5 is supporting structure. C1 + C2 + C3 + C5 is the M2 floor. C4, C6, C7 are M3 candidates.

---

## Appendix A — Bug-to-mitigation matrix

| Bug | Class | C1 review | C2 reviewer subagent | C3 static gate | C4 spec coverage | C5 silent-failure lint | C6 envelope fan-out |
| --- | --- | --- | --- | --- | --- | --- | --- |
| B1 (`+00:00` rejected) | Closure-narrower-than-spec | ✓ | ✓ (sees §5 fully) | — | — | — | ✓ |
| B2 (silent skip of unreadable) | Silent-failure introduction | ✓ | ✓ | — | — | ✓ | — |
| B3 (dead epoch conditional) | No static gate | ✓ | ✓ | ✓ | — | — | — |
| B4 (empty device id) | Edge case outside fixture | ✓ | ✓ | — | — | ✓ | ✓ |
| B5 (no docs MUST) | Spec-coverage hole | ✓ (if reviewer sees §7) | ✓ | — | ✓ | — | — |

Every blocker is hit by C1+C2. C3 and C5 are cheap secondary nets for B2, B3, B4. C4 is the only one that catches B5 cleanly without relying on the reviewer remembering to check §7. C6 is a nice-to-have for B1/B4.

---

## Appendix B — How this doc plugs into the prior wright-M2 surface

`docs/handoffs/2026-05-16-wright-first-dogfood.md` §6 listed seven wright-M2 design issues from the implementation side (verify-timeout, concurrent-verify TOCTOU, parent-dir fsync, stale-lock detection, postmortem tooling, subagent-handoff orchestration, Regressed-state-as-ready). Those are still load-bearing. This doc adds a parallel set from the *review* side:

- **M2-A.** Mandatory review-and-fix phase (C1+C2 above).
- **M2-B.** Pre-review static gate (C3).
- **M2-C.** Silent-failure lint (C5).
- **M2-D.** Spec-MUST coverage at ingest (C4).

M2-A is the load-bearing one. M2-B–D are supporting.

The two docs together cover most of the M2 surface. Prior doc's M2-1 (verify timeout enforcement) remains the hardest prerequisite for any unsupervised overnight run, since neither C1's review pass nor C3's static gate help if the verifier itself hangs.
