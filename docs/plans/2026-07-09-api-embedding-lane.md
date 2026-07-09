# API embedding lane (opt-in) — implementation plan v0.1 DRAFT

**Date:** 2026-07-09
**Status:** DRAFT — model selection pending SOTA research (sonnet + codex scouts in flight); design open for Trey review
**Owner:** Claude (Stream B); implementation delegated per lab convention
**Prereq reading:** `docs/specs/system-v0.3.md` §embedding, `crates/memoryd/src/embedding/mod.rs` module docs, `docs/plans/2026-06-09-dynamics-eval-hardening.md` Task 3.0 (why Qwen3 won)

## Goal

Ship an **opt-in** API-served embedding lane (working candidate: Voyage AI; final model decided by bake-off) so users who accept the privacy trade can run a **~10–50 MB daemon** instead of the ~1.27 GB local-model footprint. Local Qwen3 stays the default. The API lane must be consent-gated, classification-aware (sensitive content never leaves the machine), fail-open to FTS-only, and quality-gated by the same golden-corpus bench that selected Qwen3.

## Why this is cheap (architecture audit, 2026-07-09)

- `EmbeddingProvider` (`crates/memoryd/src/embedding/mod.rs:108`) is a 4-method sync trait: `triple()`, `embed_query`, `embed_document`, `embed_documents` (batch default provided). All call sites already run it under `spawn_blocking`, so a blocking HTTP client is the correct shape — no async refactor.
- Invariant 3 (triple = identity) gives the API lane its own per-triple vec tables for free; no migration hazard against the local index, typed `UnknownEmbeddingTriple`/`DimensionMismatch` errors on misconfiguration.
- The only hard gate is `server.rs:167` — `is_fastembed_candle_triple` or bust. Opening it is a provider-string dispatch in `spawn_embedding_worker`.
- Lifecycle slot (`EmbeddingProviderSlot`) already handles dormant→loading→active→failed with retry backoff and doctor surfacing; an API provider's "load" is constructing an HTTP client + a credential check, so the same state machine works with a trivial loader.
- Fail-open exists end-to-end: query-time embed timeout → `embedding_timeout` degrade marker → FTS-only; `MEMORUM_DISABLE_EMBEDDING_WORKER` path; doctor findings for load failure.

## Tiering story (product surface)

| Tier | Provider triple | Footprint | Privacy | Default |
| --- | --- | --- | --- | --- |
| `local` | `(fastembed-candle, Qwen/Qwen3-Embedding-0.6B, 1024)` | ~1.27 GB warm / ~110 MB idle | everything on-device | ✓ |
| `api` | `(voyage, <model TBD>, <dims TBD>)` (working candidate) | ~10–50 MB always | non-sensitive text sent to vendor; consent-gated | opt-in |
| `none` | worker disabled | ~10 MB | n/a — FTS-only recall | opt-out |

## Decisions (proposed — Trey to ratify)

- **D1 — Vendor-specific provider, not a generic "OpenAI-compatible" shim.** First implementation targets one vendor's API exactly (asymmetric `input_type`, batch endpoint, error taxonomy). A generic shim invites silent contract drift (wrong prompt handling = quietly degraded retrieval). Second vendor, if ever, is a second small provider.
- **D2 — API key lives in device-local runtime state, never `config.yaml`.** Same rationale as invariant 4 (device IDs): `config.yaml` syncs across clones. Resolution order: env var (`MEMORUM_<VENDOR>_API_KEY`) → runtime-state key file (0600). Keychain integration deferred.
- **D3 — Privacy fence is classification-driven, not just a consent checkbox.** Even with the lane enabled, content classified `RequiresEncryption`/sensitive is **never sent to the API**. Those memories remain FTS-only under the API lane (no local-model fallback in v1 — dual-provider residency would reintroduce the footprint the user opted out of). Consent covers: memory/chunk text of non-sensitive memories, and **query text** (recall queries transit the API too — the consent language must say so explicitly).
- **D4 — Switching lanes = full re-embed of eligible memories.** Triple change enqueues embedding jobs for all active memories (existing reindex machinery). Old triple's tables are left in place (cheap, enables switch-back); a `doctor` note reports orphaned triples with a cleanup hint.
- **D5 — Gates never touch the network.** All tests run against a mock HTTP server (or the FixtureProvider); real-API runs are off-gate manual, same policy as real-model bench runs.
- **D6 — Query-time embed timeout gets a lane-specific default.** Local default 50 ms cannot survive an HTTP round trip; API lane defaults to ~250 ms (config: `recall.vector_recall.embed_timeout_ms`), still fail-open to FTS on timeout. p95 budgets renegotiated with measured numbers per the Stream E amendment discipline, never silently busted.
- **D7 — Model choice is empirical.** Scout research (in flight) produces the candidate shortlist; the golden-corpus bench (`fixtures/golden/_embed_bench/`) produces the decision. Ship gate: candidate must beat or match Qwen3-0.6B on **trap-rate@5** and **abstention gap** — nDCG wins alone do not qualify (that's how EmbeddingGemma lost).

## Open questions (blocking full plan finalization)

- **Q1 — RESOLVED (audit 2026-07-09).** `pending_embedding_jobs` itself carries no sensitivity, but the fetch already joins `memory_chunks` (`chunk_id`), and `memory_chunks.memory_id → memories.sensitivity` (`index/schema.rs:26`, indexed at `:53`). The fence is a `WHERE` clause + one field on the `PendingEmbeddingJob` DTO — an **additive** Stream A query change, no schema migration. Enforce in SQL at the fetch boundary (single choke point, per R1), not in worker-side filtering.
- **Q2 — RESOLVED (audit 2026-07-09).** Encrypted memories never index plaintext: `write_encrypted` (`api/write.rs:154`) stores metadata + ciphertext (`encrypted_ciphertext_path`), so chunks for `RequiresEncryption` memories are masked/metadata text at most. The API lane therefore cannot leak raw encrypted bodies **but masked summaries of sensitive memories are still indexed text** — the D3 fence filters on `memories.sensitivity` tier so even masked derivatives of sensitive memories stay local-only. Exposure is strictly narrower than local behavior.
- **Q3 — Model + dims:** sonnet scout reported 2026-07-09 (see Model candidates below); codex cross-check pending. Bake-off (T4.1) still decides. Affects only the triple literal and cost table, not the architecture.
- **Q6 — Voyage retention posture (NEW, from scout):** Voyage's direct-API training-on-inputs/retention policy could not be confirmed from public docs ("governed by commercial agreement"); same gap for Gemini and Jina specifics. **Follow-up research task before any vendor is ratified** — privacy posture is a first-class selection criterion for this feature, not a tiebreaker.
- **Q4 — Rate limits / retry-after:** vendor-specific; drain worker needs 429 handling with honest backoff (jobs stay pending — the retry-budget machinery exists, but `Retry-After` respect is new).
- **Q5 — Consent UX wording:** exact `memoryd init` / `memoryd config` prompt copy; must name query-text transit (D3) and the vendor's retention posture (from scout research).

## Model candidates (scout research, 2026-07-09)

Sonnet scout (exa-grounded, source URLs in the session record); codex decorrelated cross-check pending.

| Rank | Model | Dims (MRL) | Context | Price /M tok | Asymmetric mechanism | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | **voyage-4** (+ `-large`/`-lite`/open-weight `-nano`) | 1024 (256–2048) | 32K | $0.06 (lite $0.02; 200M free) | `input_type=query\|document`; 4-series shares one embedding space across model sizes (cheap query model + strong doc model, valid cosine) | Anthropic's own docs route embeddings to Voyage. MoE serving. **Retention posture unverified — Q6.** |
| 2 | **gemini-embedding-001** | 3072 (→1536/768) | TBD | $0.15 / $0.075 batch | `task_type=RETRIEVAL_QUERY\|RETRIEVAL_DOCUMENT` | #1 MTEB English (68.32, Mar 2026) — but raw nDCG is exactly the axis that didn't predict our failure modes. Bake-off, not benchmark-trust. |
| 3 | **Cohere embed-v4.0** | 256–1536 | 128K | per-token rate not confirmed | `input_type` family | Only native text+image+PDF unified space — revisit if Memorum ever embeds attachments; no edge for pure-text short memories. |
| 4 | **jina-embeddings-v4** | 128–2048 | 32K | free 10M; tiers TBD | task-specific LoRA adapters | Qwen2.5-VL backbone (same lineage as our local model); multi-vector/ColBERT support. |
| — | **voyage-context-3** | — | — | — | contextualized chunk embeddings | Different product: embeds chunks with sibling-chunk context. Dedicated look someday for the wrong-project-lookalike problem; out of scope v1. |

**Scout flags adopted into this plan:**
- **RTEB benchmark integrity:** RTEB was co-developed with Voyage, which had private-test-set access; MTEB maintainers pulled the private RTEB column 2026-01-14 (mteb issue #3934) pending redesign. **No Voyage-vs-competitor RTEB claim is admissible evidence here.** Core MTEB + our own golden-corpus harness are the only accepted signals — which D7 already requires.
- **No public evidence exists on hard-negative discrimination or similarity calibration for any API vendor** — the metrics Memorum actually selects on. Validates D7: our bake-off isn't optional diligence, it's the only measurement of what we care about that exists.
- Anthropic has no embedding API (docs route to Voyage); Mistral Embed is stale (unchanged since 2023) — both excluded.
- Working bake-off pair: **voyage-4 (and voyage-4-lite) vs gemini-embedding-001**, pending Q6 retention verification and codex cross-check.

## Task graph (v0.1 — sizes are rough; owned files disjoint unless noted)

**Wave 0 — audits — DONE 2026-07-09 (lead, read-only)**
- **T0.1** ✅ Q1/Q2 resolved (see Open questions). Net effect on the graph: Wave 2 T2.1 becomes an additive Stream A fetch-query change (`Index::pending_embedding_jobs` gains a sensitivity predicate + DTO field) — small, but it crosses the crate boundary into `memory-substrate`, so it gets its own review eyes.

**Wave 1 — provider + config plumbing**
- **T1.1** `ApiEmbeddingProvider` (new: `crates/memoryd/src/embedding/api_provider.rs`): blocking HTTP client (reqwest blocking or ureq — pick what's already in-tree), vendor request/response types, asymmetric `input_type` mapping, batch `embed_documents` override, dimension check via `check_dimension`, typed error mapping (auth / rate-limit / transport / contract). Unit tests against a mock server. Gate: `cargo test -p memoryd -- --test-threads=2` (crate-scoped).
- **T1.2** Credential resolution (env → runtime-state file, 0600) + `EmbeddingError::Auth` surfacing; never logged, never in synced config. Owned: `api_provider.rs`, `paths.rs` (additive).
- **T1.3** Open the `server.rs:167` gate: provider-string dispatch (`fastembed-candle` → existing loader; `voyage`/vendor-string → API loader with credential check at load time so a missing key is a clean `Failed` slot state + doctor finding, not a per-job error storm).

**Wave 2 — privacy fence + drain integration**
- **T2.1** Classification-aware job filtering per T0.1 findings: sensitive jobs skipped (not failed) under an API triple, counted, surfaced in `status`/`doctor` ("N memories held local-only under API lane"). Owned: `worker.rs`.
- **T2.2** Rate-limit handling: 429/`Retry-After` → drain backoff (jobs stay pending); API microbatch sizing (token budget reused from the footprint-lab machinery; API batch caps are vendor-documented).
- **T2.3** Query path: lane-specific `embed_timeout_ms` default; degrade marker unchanged.

**Wave 3 — surfaces**
- **T3.1** `memoryd init` / config CLI: lane selection with explicit consent prompt (Q5 copy), triple write, re-embed enqueue on switch (D4). Agent envelope additions per CLI contract v1 conventions.
- **T3.2** Doctor findings: missing/invalid key, sustained rate-limiting, offline-with-API-lane, orphaned-triple note.
- **T3.3** Docs: runbook page + `using-memorum` skill note; spec amendment (dated, additive) for the new provider string — **needs Trey's explicit go-ahead per repo rules**.

**Wave 4 — quality + ship gates**
- **T4.1** Golden-corpus bake-off of scout-shortlisted candidates via `fixtures/golden/_embed_bench/` (off-gate, real API, manual). Decision recorded here; triple literal finalized. Ship gate per D7.
- **T4.2** Live dogfood: switch `~/memorum` to the API lane, verify footprint (~10–50 MB via `footprint -p`), recall quality spot-check, switch back. Field notes → `docs/reviews/`.
- **T4.3** Full `scripts/check.sh` on integrated trunk (once, at the end, per CPU discipline).

## Risks

- **R1 — Privacy fence gaps.** A single sensitive chunk leaking to the API is a trust-model breach, not a bug. Mitigation: fence enforced at the worker fetch boundary (single choke point), adversarial review specifically tasked on it, test matrix includes every `ClassificationOutcome` variant.
- **R2 — Vendor drift.** API models get deprecated/re-versioned; a silent server-side model swap changes vector space. Mitigation: triple pins the exact model string; dimension check catches gross drift; doctor can't catch subtle drift — document as a known limitation of any API lane.
- **R3 — Quality regression on our failure-mode metrics** despite better MTEB numbers. Mitigation: D7 ship gate is non-negotiable.
- **R4 — Cost surprise on big imports.** A 10k-memory import is ~ millions of tokens. Mitigation: `init` consent prompt shows a cost estimate for the current corpus size before enqueueing re-embed.
- **R5 — Latency p95 regressions on recall.** Mitigation: D6 timeout + degrade marker; measured renegotiation only.

## Plan revision history

- **v0.1 (2026-07-09):** first draft, authored while scout research (sonnet exa lane + codex lane) in flight. Model choice, Q1–Q5 open.
- **v0.1 amendments (2026-07-09, same day):** Q1/Q2 resolved by lead audit (fence = sensitivity predicate on existing join; encrypted bodies never indexed in plaintext). Sonnet scout results folded in: candidate table, RTEB conflict-of-interest exclusion, Q6 (vendor retention verification) added as a ratification blocker. Codex cross-check pending.
