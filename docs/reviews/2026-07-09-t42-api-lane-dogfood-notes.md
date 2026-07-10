# T4.2 — API-lane live dogfood field notes (2026-07-09)

Runbook: `docs/runbooks/api-lane-dogfood-t42.md`. Live repo `~/memorum`, daemon rebuilt from `main`
(post-Wave-3, spec amendment applied). Trey directive: full local ship — reinstall, reimport, switch to
the API lane, land there.

## T4.1 bake-off (same day, precondition)

Real-API golden-corpus run, Gemini only (no Voyage/Jina keys on device). Results copied into
`crates/memorum-eval/fixtures/golden/_embed_bench/result_gemini-*.json`.

| model | ndcg10 | recall5 | mrr | trap5 | abstain gap |
| --- | --- | --- | --- | --- | --- |
| qwen3-0.6b (shipped local) | 0.751 | 0.89 | 0.724 | 0.261 | 0.332 |
| embeddinggemma | 0.843 | 0.91 | 0.827 | 0.348 | 0.137 |
| **gemini-768** | **0.845** | **0.92** | 0.825 | 0.346 | 0.164 |
| gemini-1536 | 0.849 | 0.90 | 0.835 | 0.365 | 0.172 |
| gemini-3072 | 0.854 | 0.90 | 0.835 | 0.365 | 0.169 |

**Verdict: 768 ratified.** 1536/3072 buy +0.004/+0.009 nDCG for 2–4× vector storage; 768 has the best
recall@5 in the whole table and the best Gemini trap rate. Mean query embed 206 ms (inside the 250 ms
API-lane recall timeout). Shipped default triple unchanged: `(gemini-api, gemini-embedding-2, 768)`.

## Timeline

1. Rebuilt + `cargo install`ed memoryd from `main`; `launchctl kickstart -k gui/501/com.memorum.daemon`.
2. 18 governance quarantines (7/8 import class) promoted via `memoryd review approve` → doctor healthy.
3. `memoryd import` (idempotent) picked up new sources; 7 new quarantines promoted the same way.
4. Baseline: footprint 6,200 MB (local model warm, draining backlog); recall spot-checks captured
   (`/tmp/t42-baseline-recall.txt`).
5. Key installed at `~/memorum/.memoryd/gemini_api_key` (0600). Consent ceremony:
   `memoryd config embedding-lane --lane gemini-api --consent` → estimate **1,127 eligible chunks,
   ~465k tokens, ~$0.093 upper bound**; `restart_required: true`; old vec tables retained.
6. Daemon restarted on the API lane. Provider loaded (`gemini-embedding-2`, dim 768).

## Drain (the T2.2 load test)

- Full re-embed of 1,127 chunks completed in well under 10 minutes via ~12 `batchEmbedContents` calls
  (batch=100). **Zero 429s observed** — microbatching kept the run under the rate cap, so the
  Retry-After path was NOT exercised live; it remains covered by the mock-server tests only.
- New active-triple table `vec_cf595066…` holds exactly 1,127 rows. Old local table (1,155 rows)
  retained by design (`embedding_orphaned_triples` advisory).

## Fence verification (live)

- `held_local_jobs: 28` — all confidential/personal-tier jobs held local, never fetched for the API
  lane. Doctor shows the `embedding_api_lane_held_local` advisory naming the count.
- No sensitive-tier text observed in any drain batch (fence is enforced at job fetch, verified by the
  Wave-2 cross-family review; live counts corroborate).

## 🐛 Production bug caught by the dogfood

Every **query** embed failed:
`parse Gemini embedding response: missing field 'embeddings'` → vector recall silently degraded to
FTS-only (the degrade ladder worked as designed — recall never broke).

Root cause: `embed_prefixed` calls Gemini's single-shot `:embedContent`, which returns
`{"embedding": {...}}` (singular); the parser only accepted the batch shape `{"embeddings": [...]}`.
The mock server returned the batch shape for BOTH endpoints — the mock encoded the parser's wrong
assumption, so every test was green. The T4.1 bake-off only used `batchEmbedContents`, so it couldn't
catch it either.

Fix: `GeminiEmbeddingsResponse` accepts both shapes (`embedding: Option<…>` wins when present);
single-endpoint mock tests now serve the real singular shape, with a comment pinning the incident.
Lesson (general): **a mock written from the parser instead of from the wire format proves nothing** —
when staging an API integration, capture one real response per endpoint first.

## Post-fix verification

(filled in below after redeploy)

## Record

- Cost estimate ~$0.093 upper bound vs actual ≈ $0.09 (465k tokens at $0.20/M ≈ full estimate; bytes/4
  heuristic was nearly exact on this corpus).
- 429 count: 0. Drain wall-clock: minutes, not hours.
- Footprint: baseline 6,200 MB (local warm) → see post-fix numbers below.
