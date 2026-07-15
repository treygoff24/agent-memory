# W4b T1 enrichment v2 — review round 1 triage (2026-07-15)

Reviewers: Grok 4.5 (cursor safe) + native Opus. Author: Sol xhigh (codex-89). Both reviewers:
all five pre-registered integrity properties (v1-byte-identical, no-structural-fallback, key
parity, provenance refusal, deterministic windowing) HOLD, with independent call-site-level
verification. Not dry — accepted fixes below.

## Accepted

- **T1-F1 (Grok M1 + Opus n4, severity adjudicated MAJOR):** ingestion forwards sidecar entries
  blindly; add the null-with-cues refusal at `apply_enrichment_meta`/v2 ingest so a corrupt or
  hand-edited entry cannot attach wrong meta. Producer-side validation already exists.
- **T1-F2 (Grok M2, MAJOR — protocol hazard):** `--split` defaults to `both`; a bare v2 run would
  enrich holdout under a still-tunable prompt (adaptive-leakage violation of the pre-registered
  rule). Fix: default `--split dev`. Explicit `--split holdout|both` remains available (T4 uses it
  deliberately, post-freeze).
- **T1-F3 (Opus m1, MINOR but freeze-critical):** `v2_prompt_template()` (hashed) and
  `prompt_for_context()` (sent) are two `format!` skeletons sharing only the instruction block —
  they can drift and silently falsify `prompt_sha256`. Single-source the skeleton; add a test that
  the rendered prompt for a known context matches the hashed template with substitutions.
- **T1-F4 (Grok m4, MINOR):** enrich run can exit 0 with pending holes; the benchmark's fail-closed
  bar catches it later but wastes an eval launch. End-of-run: any enumerated key missing → non-zero
  exit + explicit count (still resumable).
- **T1-F5 (Grok m5, MINOR):** add the E2E date-ordinal ingest test (fixture with a session-date
  body; assert ingestion finds the `date_metadata` entry under the one-past-last key).
- **T1-F6 (Opus n3, NIT):** `report.generated` aggregates harness+sensitive+null for v2; log the
  disposition breakdown at progress time so the T2 null-rate check reads the true numbers.

## Rejected (with reasons)

- **Grok m3 (date-only batches don't reset the breaker):** transparent-is-correct — an all-date
  batch makes zero harness calls and carries no evidence of harness health; resetting the streak on
  it would let interleaved date batches mask a genuinely dead harness. Opus read the same code as
  correct. No change.
- **Fence escaping (both reviewers, residual):** v1-parity injection discipline is pre-registered
  (plan §T1 step 5); blast radius bounded by output validation + caps. No change tonight.
- **Opus n5 (v1 report JSON gains 3 additive fields):** invariance control compares dataset
  sha256s, item IDs, dispositions, retrieved-context sets — not raw JSON shape. No change.
