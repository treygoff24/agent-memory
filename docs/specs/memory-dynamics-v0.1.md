# Memory Dynamics — Feature Spec v0.1

**Status:** Draft for Trey sign-off. Authorized by approvals A1 (Stream E v0.5 → v0.6 ranking behavior change), A2 (Stream F amendment: archival deferral + calibration log surface) granted 2026-06-09 against `docs/plans/2026-06-09-dynamics-eval-hardening.md` §2.

**Date:** 2026-06-09. **Author:** Claude (Fable), reviewed against shipped code (`crates/memoryd/src/reality_check/scoring.rs`, `crates/memoryd/src/recall/rank.rs`, `crates/memoryd/src/recall/candidates.rs`).

**Relationship to other specs:** consumes Stream A's `events_log` SQLite mirror and `memory_supersession` projection (system-v0.2 §19, shipped). Changes Stream E recall ranking (v0.6 bump). Amends Stream F cleanup-layer archival and adds one on-disk surface (`dreams/calibration/`). Adds no MCP tools (system-v0.2 §14.1 stays frozen at ten). Adds no frontmatter fields.

## 1. Purpose

Memorum stores and gates memories well; nothing yet makes a memory *live*. Use does not strengthen, disuse does not fade, and review outcomes teach the system nothing. This spec adds the dynamics layer:

1. **Strength** — a per-memory `[0,1]` score derived from actual use (recall hits, recency of use, corroboration) that feeds recall *ranking competition*.
2. **Archival deferral** — substrate fragments that dreaming keeps citing live longer (bounded).
3. **Calibration log** — review decisions are recorded against the candidate's self-reported confidence, producing the data system-v0.2 §12 says must exist before any v1.1 auto-promotion.

### 1.1 Design principles

- **Use changes ranking, never existence.** Low strength loses ranking competitions; it never deletes, archives, or tombstones a canonical memory. Tombstones remain the only deletion path (governance principle 8 untouched).
- **Confidence is provenance; strength is use.** `confidence` continues to mean "how well-grounded was this when written/reviewed." Strength is a separate axis and never mutates confidence.
- **One usage *query*, two consumers.** Reality Check drift scoring uses *inverse* frequency ("validate what you haven't been using"); strength uses *direct* frequency ("trust what keeps proving useful"). Both read the same raw inputs through one shared query (`recall_counts_30d`, pool max, distinct sources) — but apply **different normalizations by design**: RC stays linear (`count/max`), strength is log-scaled (`log1p/log1p`). Share the query, never the curve.

## 2. Strength function

```
strength(m) = w_f · freq_norm(m) + w_r · recency(m) + w_c · corroboration(m)
```

- `freq_norm(m) = log1p(recall_count_30d(m)) / log1p(max_recall_count_30d_active)` — log-scaled so a 50-hit memory doesn't drown a 10-hit one 5:1; normalized over the active candidate pool, same denominator convention as RC scoring's `recall_frequency_norm`. `0` when the pool max is `0`.
- `recency(m) = exp(-days_since_last_recall(m) / τ)`, `τ = 14` days default. `0` when the memory has never been recalled.
- `corroboration(m)` — the existing binary cross-source signal (≥2 distinct `source_harness` across the supersession chain), exactly `reality_check::scoring::cross_source_corroboration`.
- Default weights `w_f = 0.45, w_r = 0.35, w_c = 0.20`. **True renormalization:** configured weights are divided by their sum (guard: all ≥ 0 and sum > 0, else fall back to defaults with a `tracing::warn!`). Note this is deliberately *not* RC `ScoreWeights::normalized_or_default`'s posture — that function validates sum≈1.0 and silently discards user weights on mismatch (types.rs:25-34), which is a footgun for a dogfood-tunable surface. Dynamics renormalizes; RC's behavior is unchanged.
- Result clamped to `[0, 1]`.

> **Amendment (2026-06-10):** For deterministic startup and eval behavior, strength recency and the 30-day recall-usage window are anchored to `ranking_now = max(candidate.updated_at)` for the candidate set being ranked, not wall clock. This intentionally keeps repeated runs stable. Known consequence: recency discrimination can flatten when usage timestamps are newer than the content timestamps that define `ranking_now`; tuning that tradeoff belongs in the dogfood loop.

### 2.1 Data sources (all shipped today)

- `recall_count_30d` and `last_recalled_at`: `events_log WHERE kind='recall_hit'` — the exact query shipped in `reality_check/scoring.rs::recall_counts_30d` (chunked `IN`, covering index `idx_events_log_kind_memory_ts`).
- `corroboration`: the depth-bounded recursive CTE shipped in `scoring.rs::distinct_sources_by_id`.
- **No new columns, no new event kinds, no schema migration.** Strength is computed at recall-assembly time from existing projections.

### 2.2 What counts as "use" (v0.1)

Only `recall_hit` events (one per memory included in a rendered startup/delta block, deduplicated per response — Stream E's existing emission). This is deliberately the weakest, highest-volume signal: it's the only one with shipped emission today.

**Reserved extension (v0.2 of this spec, not now):** a weighted event map (`memory_get` fetch ≈ 3×, `RealityCheckConfirmed` ≈ 5×) once dogfood shows raw inclusion-counting is too noisy. Config keys are reserved under `dynamics.event_weights` but unrecognized in v0.1.

## 3. Ranking integration (Stream E v0.6)

`recall/rank.rs::score_candidate` is an additive integer-points system (status 100/50, exact-project scope 30 / user 25 / agent 15, entity match — ExactId 40 / alias 25 / tag 10 per entity.rs:149, recency-of-update 10/5/0, confidence 0–10, source 0–10). Strength joins it as one more bounded component — **not** a multiplicative blend:

```
strength_points(m) = floor(strength(m) × dynamics.alpha_points)    // default alpha_points = 12
```

> **Amendment (2026-06-10):** The implementation caps the additive term at `alpha_points - 1` when `alpha_points > 0` (`0` when `alpha_points == 0`). The invariant is the contract: strength cannot tie or overcome a structural gap `>= alpha_points`. The formula above remains the base calculation before the cap.

- **The invariant, stated precisely:** strength cannot overcome a structural gap ≥ `alpha_points`. It can and will flip *near-ties* — including across scopes — when the loser's total structural lead is < 12 points (e.g. an exact-project memory leading a user-scope memory by only 3 total points loses to a +12 full-strength bonus; that is intended behavior: heavily-used cross-scope memory beats barely-relevant in-scope memory in a close race). What strength can never flip at default: a pinned-vs-active gap (50), a full entity ExactId gap (40), or any combined structural lead ≥ 12. Operators tune the ceiling via `alpha_points`. Pinned memories dominate regardless (status 100).
- **Hydration:** `candidates.rs` fetches strength inputs in one batched, chunked query over candidate ids (shared module, §5) and attaches `strength: Option<f64>` to `RecallCandidate`. `rank.rs` stays pure/sync.
- **Soft failure:** if the usage query errors, all candidates get `strength = None` → 0 points, a `tracing::warn!`, and `dynamics_degraded: true` in the block's explanation metadata. Never a hard recall failure.
- **Observability:** per-memory strength (2 decimals) appears in the recall block explanation metadata and in trust artifacts (Stream G additive surface), so an operator can always answer "why did this rank here."
- **`recency_weight` (updated-at) is retained unchanged.** It measures content freshness; strength's recency measures use. Distinct signals.
- **Cache stability (system-v0.2 §2.9):** strength is computed once per assembly; startup blocks are per-session stable. Delta blocks were already dynamic content in the suffix. No new cache-thrash surface.

> **Amendment (2026-06-10):** Recall explanation strength is exact for the ranked candidate pool. Trust artifacts must not render strength when `dynamics.enabled == false`. When exact ranking parity is not available at artifact-render time, the artifact strength line must be explicitly labeled approximate and computed with the configured dynamics weights and `tau_days`, not defaults.

## 4. Substrate fragment archival deferral (Stream F amendment)

The cleanup layer currently archives substrate fragments at a hard lifetime cutoff (14 days). Amendment:

- At cleanup time, a fragment whose id is cited ≥ `dynamics.citation_defer_threshold` (default 2) times has its archival deferred by one base lifetime (14 days). **Citation source (the structured one, not journal prose):** `Evidence.reference` entries in candidate memory frontmatter (`evidence: Vec<Evidence>`, model.rs:371-381 — the same refs rehydration resolves at rehydration.rs:176), counted across active + queued candidates within the current lifetime window. Dream journal markdown is NOT scanned — it's prose, and grepping it for fragment ids is brittle by construction.
- Deferral may repeat, but total fragment lifetime is capped at `dynamics.max_fragment_lifetime_days` (default 42 = 3× base). **Nothing becomes immortal by citation.** At the cap, archival proceeds normally (archived fragments remain on disk and grounding-resolvable; this changes *when*, not *whether*).
- Cleanup run reports (`dreams/cleanup/<device_id>/<date>.json`) gain a `deferred_fragments: [{fragment_id, citations, deferred_until}]` array (additive).

> **Amendment (2026-06-10):** Citation count means distinct live citing memories, not duplicate `Evidence.reference` entries within one memory. Citations from any live memory count regardless of citation age; the prior "within the current lifetime window" qualifier is not implemented and is superseded. `citation_defer_threshold: 0` disables deferral. Cleanup reports use `cap_deadline` rather than `deferred_until`, because deferred fragments are re-evaluated on every cleanup pass and can archive before the cap if citations drop below the threshold.

## 5. Shared usage module

New `crates/memoryd/src/dynamics/` owning the usage computation:

- `usage.rs`: `recall_usage_for(ids, now) -> HashMap<MemoryId, UsageSummary{count_30d, last_recalled_at}>` and `distinct_sources_for(ids)` — **moved from** `reality_check/scoring.rs`. This is a real refactor, not a copy-paste: the functions are currently private and `score_memories_at` constructs its own `Index` inline (scoring.rs:34) — the index-acquisition path moves into the shared module too, so both consumers read through one connection path. RC behavior identical; RC tests (which assert on `component_scores` outputs) stay green.
- `strength.rs`: `strength(facts, weights) -> f64` + the component functions, mirror of RC's component style, unit-tested at boundaries (never-recalled, pool-max, single-memory pool, τ extremes).

## 6. Calibration log

**Surface:** `dreams/calibration/<device_id>.jsonl` — append-only, git-synced, per-device files merge by concatenation (same convention as `events/`).

**Emission:** on every review decision (approve / reject / edit) for any candidate whose `author.kind == dreaming` or whose status is quarantined:

```json5
{
  v: 1,
  candidate_id: "mem_...",
  scope: "project:proj_a3f2",
  author_kind: "dreaming",
  self_reported_confidence: 0.82,
  decision: "accept" | "reject" | "edit",
  edit_distance_ratio: 0.18,        // present iff decision == "edit": levenshtein(old,new)/max(len)
  decided_at: "2026-06-09T19:04:11Z",
  session_id: "sess_...",
}
```

> **Amendment (2026-06-10):** `decision: "edit"` and `edit_distance_ratio` are schema-defined for future edited approvals, but the current daemon review protocol does not emit edit decisions; today it emits accept/reject records only.

Ids and metadata only — **no memory content** crosses into this file (it syncs plaintext). Scope strings are not a new exposure: scope ids already sync in plaintext journal paths (`dreams/journal/project/<project_id>/...`, scope.rs:40-46).

**Consumer:** `memoryd dream calibration` — buckets `self_reported_confidence` into deciles, reports accept-rate, edit-rate, and count per bucket, total N, and the spread between self-report and acceptance. This is the instrument that justifies or kills the system-v0.2 §12 v1.1 auto-promotion path. No automated behavior changes based on this data in v0.1 — report only.

## 7. Configuration

```yaml
dynamics:
  enabled: true                    # false → strength_points = 0 everywhere, deferral off; calibration log still writes
  alpha_points: 12
  tau_days: 14
  weights: { frequency: 0.45, recency: 0.35, corroboration: 0.20 }
  citation_defer_threshold: 2
  max_fragment_lifetime_days: 42
```

> **Amendment (2026-06-10):** `citation_defer_threshold: 0` disables archival deferral. Positive thresholds count distinct live citing memories.

All dogfood-tunable; final defaults lock before 1.0.0 (same policy as the §16.4 drift weights). The calibration log is **not** gated by `enabled` — review-outcome data collection should never silently stop.

## 8. Anti-features (v0.1 will refuse)

1. **No confidence mutation from use.** Frequency never edits `confidence`; the RC `confirm` bump remains the only use-driven confidence touch, unchanged.
2. **No hard deletion or auto-tombstone from low strength.** Ranking competition only.
3. **No spaced-repetition resurfacing.** Strength ranks what recall already selected; it does not inject "you haven't thought about X lately" items. (Reality Check already owns that surface, deliberately, weekly.)
4. **No per-session/per-harness strength.** Strength is a property of the memory, not of who's asking. Per-context affinity is a different feature with real privacy questions; out of scope.
5. **No auto-promotion from calibration data.** v0.1 collects and reports; acting on it is a future explicitly-approved change.

## 9. Verification

1. Unit: strength component boundaries (§5); weight normalization; clamping.
2. Integration: seed `events_log` recall hits via `TestInjectEvent`, assert (a) ranking shifts for tied candidates, (b) the precise invariant: a structural gap of `alpha_points - 1` IS flipped by full strength, and a structural gap of `alpha_points` (and a pinned-vs-active gap) is NOT — both directions, so the test fails if anyone silently rescales components, (c) `dynamics.enabled: false` produces blocks byte-identical to pre-dynamics **except the `version=` attribute**. **Policy-string decision:** `STREAM_E_POLICY` (types.rs:6) bumps unconditionally to `stream-e-v0.6` — version strings denote the rendering contract, not active features; a config-dependent version string would be worse. The byte-identity test masks the version attribute only.
3. **Quality gate (hard, from the plan):** golden-corpus quality runner (plan Task 4.2) runs A/B — dynamics off vs on. nDCG must not regress; the on-mode result becomes the committed baseline only after explicit human review of the diff.
4. Calibration round-trip: decide → JSONL line → decile report.

## 10. Open questions for dogfood

1. Is inclusion-in-block too weak a "use" signal (block inclusion ≠ the agent actually leaned on it)? The §2.2 weighted-event extension is the prepared answer.
2. Is `alpha_points = 12` audible at all in real blocks, or does structural scoring drown it? The quality runner's A/B delta answers this empirically.
3. Should archival deferral also consider *recall* citations of fragments surfaced through `<pending-attention>`? (v0.1: no — dream citations only.)
