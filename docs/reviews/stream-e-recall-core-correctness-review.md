# Stream E Recall Core Correctness Review — Gate B

**Date:** 2026-04-30
**Scope:** Tasks 6, 7, and 8 recall core from `docs/plans/2026-04-30-stream-e-passive-recall.md`, checked against `docs/specs/stream-e-passive-recall-v0.5.md`.
**Review mode:** Read-only review except this report. No fixes applied.

## Intended outcome

Tasks 6-8 are intended to establish the Stream E recall core before daemon/MCP/CLI wiring: exact v0.5 DTOs, deterministic budgeting/rendering primitives, strict session/project binding, and a candidate/entity/ranking core that uses Stream A recall-index projections rather than raw Markdown scans or blanket envelope hydration. Stream A remains the only persisted substrate, Stream C remains the lifecycle authority, and Stream D remains the privacy/reveal authority.

## Verdict

**Changes requested.** The deterministic DTO/budget/rendering primitives and project-binding happy/error paths are mostly covered by passing targeted tests, but Task 8 does not yet meet the collection contract and has one concrete entity-resolution correctness bug. These should be fixed before moving past Gate B because later protocol wiring would otherwise build on an incomplete recall-core boundary.

## Findings

### P1 — Entity alias collision drops unrelated valid matches in the whole section

- **Evidence:** `crates/memoryd/src/recall/entity.rs:30-33` computes alias collisions and immediately returns `EntityResolution { candidates: Vec::new(), ... }` whenever any collision exists. That suppresses every candidate in the section, not just facts based on the ambiguous alias. `crates/memoryd/tests/startup_recall_ranking.rs:107-138` only tests the all-colliding case and therefore locks in the over-broad empty-candidate behavior.
- **Why it matters:** Spec v0.5 §7 says that when one alias maps to multiple entity ids, Stream E must not emit a fact based on that alias and must emit one collision omission for that `(section, alias)`. It does not say that a collision should erase other candidates that matched by exact entity id, tag, label, or a different non-ambiguous alias. In a real startup block, one ambiguous project alias like `forge` could incorrectly suppress unrelated exact-id or tag matches for the same section, reducing recall quality and making explanations misleading.
- **Test evidence:** The required command `cargo test -p memoryd --test startup_recall_ranking` passes, but no test combines an alias collision with another valid non-colliding candidate in the same section.
- **Recommendation:** Change entity resolution so collision omissions only exclude matches whose selected match path depends on the ambiguous alias. Preserve candidates that match exact ids, tags, labels, or non-colliding aliases. Add a regression where two candidates collide on alias `forge` and a third candidate matches seed `ent_safe` or a tag; assert the collision omission is emitted and the safe candidate remains selected.
- **Confidence:** High.

### P1 — Candidate collection is not Stream A-backed, so the recall-index/envelope-bound contract is not enforced

- **Evidence:** `crates/memoryd/src/recall/candidates.rs:37-57` exposes `collect_recall_candidates(section, rows: Vec<RecallIndexRow>)`; it only post-filters rows already supplied by the caller. A targeted search found no `query_recall_index`, `RecallIndexQuery`, `MemoryQuery`, `Substrate`, `read_memory_envelope`, or `query_chunks` usage under `crates/memoryd/src/recall/`; the only raw file read in recall core is project config loading in `project.rs`. Tests also construct synthetic `RecallIndexRow` values directly (`crates/memoryd/tests/startup_recall_governance.rs:82-103`, `crates/memoryd/tests/startup_recall_ranking.rs:178-199`) rather than exercising Stream A query behavior.
- **Why it matters:** Task 8 and spec v0.5 §6 require candidate enumeration to be served from Stream A APIs using index-side namespace/status/passive-recall/updated filters, and require envelope reads to be bounded to selected/rendered memories rather than every active/pinned candidate. The current core does not scan raw Markdown, which is good, but it also does not implement the Stream A collection boundary at all. Later wiring could pass rows from an inefficient or over-broad source and still satisfy these unit tests.
- **Test evidence:** The required commands `cargo test -p memoryd --test startup_recall_governance` and `cargo test -p memoryd --test startup_recall_ranking` pass, but they do not prove any Stream A API call, namespace filter, active/pinned query split, updated-since query, or bounded envelope-read behavior.
- **Recommendation:** Add a Stream A-backed collection entrypoint for each section that builds `RecallIndexQuery` values with the required namespace/status/passive/updated filters and consumes `query_recall_index`. Keep the pure `Vec<RecallIndexRow>` filtering as an internal helper if useful. Add tests with either a small real `memory_substrate::Substrate` fixture or a narrow trait/fake that records calls, asserting that recall-index projections are used and no envelope reads occur during candidate enumeration.
- **Confidence:** High.

### P2 — Candidate governance filters omit max-scope/sensitivity compatibility and encrypted metadata-only behavior

- **Evidence:** `crates/memoryd/src/recall/candidates.rs:60-75` filters only `passive_recall`, pending review/confirmation, and lifecycle status. It does not inspect `RecallIndexRow.sensitivity`, `max_scope`, or `index_body`, even though those fields are present on the row fixtures (`crates/memoryd/tests/startup_recall_governance.rs:93-99`, `crates/memoryd/tests/startup_recall_ranking.rs:189-195`). The governance tests cover inactive statuses, passive recall disabled, confirmation, human review, and pending review state, but no max-scope/sensitivity or encrypted/metadata-only cases.
- **Why it matters:** Spec v0.5 §6 requires candidate filters to enforce that memory scope is visible, sensitivity is compatible with `retrieval_policy.max_scope`, and encrypted records are represented as metadata-only unless Stream D supplied safe projections. Without this in the core, a future renderer can receive candidates that should have been excluded or down-scoped before ranking, increasing privacy and governance risk.
- **Test evidence:** `cargo test -p memoryd --test startup_recall_governance` passes with 4 tests, but those tests do not vary `sensitivity`, `max_scope`, or encrypted/index-body behavior.
- **Recommendation:** Either implement these filters in candidate collection now or explicitly split responsibility so rendering cannot emit body/snippet content for such rows. Add regression tests for out-of-scope `max_scope`, incompatible sensitivity, and metadata-only/encrypted rows to verify they do not become factual body recall.
- **Confidence:** Medium.

## Checklist assessment

- **DTOs exactly match v0.5 serialized shape:** Mostly satisfied. `StartupResponse`, `SessionBinding`, `ProjectBinding`, `RecallExplanation`, `RecallSectionExplanation`, `RecallOmission`, `OmissionReason`, and `RecallSectionName` are present with v0.5 policy string and additive alias/colliding-id omission fields (`crates/memoryd/src/recall/types.rs:3-107`). Serde tests cover alias omission compatibility and project binding source naming.
- **Budget estimator and UTF-8 truncation deterministic:** Satisfied for primitives. `estimated_tokens` uses byte length div-ceil by 4 (`crates/memoryd/src/recall/budget.rs:10-12`), and `truncate_utf8_bytes` preserves character boundaries while reserving space for `…` (`budget.rs:14-29`). Determinism tests pass.
- **Project binding rejects malformed configs:** Satisfied for the specified tested cases. Tests cover empty/non-mapping/duplicate/unknown/unsupported-scalar YAML plus invalid canonical ids and alias length. Note: implementation uses a fail-closed line pre-parser plus `serde_yaml`, not a low-level YAML event parser; I did not find a malformed case accepted during this review, but this remains a parser robustness area for later review.
- **Candidate collection relies on Stream A APIs and avoids raw Markdown scans for facts:** Not satisfied. No raw Markdown scan exists in recall core, but no Stream A-backed collection exists either; see P1 finding.
- **Candidate collection and ranking rely on Stream A recall-index projections, envelope reads bounded to selected/rendered memories:** Partially satisfied. Ranking consumes `RecallIndexRow` fields directly and no envelope reads occur in current core, but the collection path does not call Stream A recall-index APIs or prove envelope-read bounds; see P1 finding.
- **Entity/ranking tests exercise tie-breakers and stable omitted metadata:** Partially satisfied. Ranking weights/tie-breakers, pre-shuffle stability, budget omissions, and alias-collision omission shape are tested. Missing mixed collision + valid match coverage; see P1 finding.

## Commands run

```text
cargo test -p memoryd --test startup_recall_determinism
Result: PASS — 10 passed; 0 failed

cargo test -p memoryd --test startup_recall_project_binding
Result: PASS — 9 passed; 0 failed

cargo test -p memoryd --test startup_recall_governance
Result: PASS — 4 passed; 0 failed

cargo test -p memoryd --test startup_recall_ranking
Result: PASS — 7 passed; 0 failed
```

## Changed path

- `docs/reviews/stream-e-recall-core-correctness-review.md`

## Residual risk

This review did not run full workspace gates or clippy because Task 9 requested the four targeted recall-core tests. Protocol/MCP/CLI integration, privacy acceptance, performance, and docs contract review are explicitly later gates in the plan.

## Fix status — 2026-04-30

- **P1 entity alias collision:** Fixed. `resolve_entity_matches` now emits the section-level ambiguous-alias omission while suppressing only candidates whose selected match depends on that ambiguous entity alias. Candidates matching exact entity id, normalized tag, exact entity label, top-level memory alias, or non-colliding entity alias remain eligible. Added `alias_collision_only_suppresses_candidates_depending_on_ambiguous_alias`.
- **P1 Stream A-backed collection:** Fixed. Added a narrow `RecallIndexReader` trait plus `collect_recall_candidates_from_index`, implemented for `memory_substrate::Substrate`, that builds `RecallIndexQuery` calls with namespace, active/pinned status, passive-recall, and updated-since filters. Added a fake-backed regression proving enumeration uses recall-index projections and performs no envelope hydration.
- **P2 governance/privacy filters:** Fixed for core factual-body recall. Candidate post-filtering now rejects rows whose scope exceeds `max_scope`, rows with `index_body=false`, and confidential/personal rows as `encrypted_body_hidden`, preventing unsafe factual body recall before ranking. Added coverage for max-scope, confidential/encrypted, and metadata-only/index-body-disabled rows.

Verification after fixes:

```text
cargo test -p memoryd --test startup_recall_governance
Result: PASS — 6 passed; 0 failed

cargo test -p memoryd --test startup_recall_ranking
Result: PASS — 8 passed; 0 failed

cargo test -p memoryd --test startup_recall_determinism
Result: PASS — 10 passed; 0 failed

cargo clippy -p memoryd --all-targets --all-features -- -D warnings
Result: PASS

cargo fmt --all -- --check
Result: PASS

git diff --check
Result: PASS
```
