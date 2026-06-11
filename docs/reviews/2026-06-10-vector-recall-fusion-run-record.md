# Run record — vector-recall fusion (hybrid FTS+vector delta recall)

**Date:** 2026-06-10. **Plan:** `docs/plans/2026-06-10-vector-recall-fusion.md` (r2, plan-reviewer-patched). **Orchestrator:** Claude session, pure-orchestrator model — every unit of work delegated to `delegate codex work` lanes (manual worktrees under `../agent-memory-wt/`), one `codex safe` lane, and one native opus docs subagent. The orchestrator reviewed every diff, re-ran every gate itself, and owned all trunk commits.

## What shipped (trunk, in merge order)

| Commit(s) | Lane | Content |
|---|---|---|
| 621db77 | — | The plan itself. |
| 2d51186 | Wave 0 (opus docs) | Stream E **v0.6** spec (RRF k=60, seven-marker ladder, additive `vector_recall_degraded`, per-device byte-stability, §13 note) + Stream A v1.1 dated amendment for `query_hybrid_chunks`. Healed the pre-existing code-ahead-of-spec drift: `STREAM_E_POLICY` had said `stream-e-v0.6` since the dynamics arc. |
| 579be41, 820f31e | Lane S (codex) | `Substrate::query_hybrid_chunks` — per-memory hybrid candidates, structural both-or-neither via `HybridVectorQuery`, recall membership filter incl. `status IN ('active','pinned')` (stricter and more spec-faithful than legacy `query_chunks`), chunk→memory collapse, `UnknownEmbeddingTriple` contract, five acceptance tests. |
| 301d9c5, 7348715, afa191e | Lane R (codex) | memoryd integration: provider threading, `embed_query` + KNN + exported `fuse_rrf` (1-based ranks both lanes), seven-rung degradation ladder, `recall.vector_recall` config block, `memory_search` wired to the same surface (no wire-shape change), bench vector phase + threshold, 508-line integration test suite. |
| 1c593df, 9830b0e | bench-fix (codex) | cold_reindex regression fix (skip no-op supersession resync) + stream-a bench triple coherence (bench writes synthetic config before init). |
| 2dbae6c | Lane E commit A (codex) | Eval: deterministic vector population (drain via `FixtureProvider` over the substrate's active triple), fused side-report mode (`--fused-report`, `--embedding fixture|real`), default seam untouched. |
| 8398e99, 627523c | hydration-fix (codex) | Fused candidates carry representative chunk text from the index SQL; per-candidate envelope reads deleted. Restores legacy chunk-text parity. |
| 5736ae3, fa3744c | emit-batch (codex) | `record_recall_hits` batches N event appends into one write + one seq-state update (`reserve_event_sequences`); byte-equivalent log output; durability tier unchanged (was and remains best-effort for RecallHit). |

**Held off trunk (deliberate):** `vector-fusion/lane-e` commit `79ea90d` "HOLD: switch eval search seam to fused recall" — the `rank_via_search` switch + `ranking_lane` + `report_is_well_formed` updates. Lands ONLY inside the atomic human-approved `[bench-update]` commit together with the re-armed `bench/quality-baseline.json` (plan §Wave 4 step 5, B1 invariant).

## Quality numbers (the point of the arc)

Search seam, scored cases:

| Corpus / provider | nDCG@5 | recall@5 | MRR | trap@5 |
|---|---|---|---|---|
| Fictional, FTS-only (armed baseline) | 0.094 | 0.070 | 0.120 | 0.020 |
| Fictional, fused fixture (re-arm candidate) | 0.445 | 0.463 | 0.528 | 0.180 |
| Fictional, fused real Qwen3 | 0.746 | 0.753 | 0.828 | 0.200 |
| Private machine-local corpus, FTS-only | 0.151 | 0.120 | 0.196 | 0.000 |
| Private machine-local corpus, fused real Qwen3 | 0.660 | 0.680 | 0.768 | 0.152 |

Relevance lift 4–11x on every metric, both corpora. **Trap-rate@5 rises to 0.15–0.20** — the semantic lane surfaces engineered lookalikes (plan risk #1). Two mitigating notes: traps are lexical-adjacent by design, and the eval search seam is corpus-wide while production delta recall is namespace-scoped (wrong-project traps partly pre-filtered in production). Mitigation knobs if desired pre-arm: `rrf_k` / `knn_limit` (config-only). Private corpus measured via `--corpus-root` only; never committed, contents never named in any artifact.

## The perf investigation (the unexpected half of this run)

The stream-e recall bench (NOT part of `check.sh` — that's how this hid) failed its release thresholds. Instrumented attribution, stage by stage:

1. **Fused machinery is ~2.6ms** (embed 30µs / KNN 2.1ms / fuse 16µs at 200 memories, 1024-dim fixture vectors). The vector lane itself was never the problem.
2. **`validate_delta_request` ≈ 55ms on EVERY delta request** — pre-existing since (at latest) the dynamics arc; explains the legacy delta running ~62ms vs the accepted 9–10ms. Likely session-binding/git work per request. **Open follow-up arc — not fixed here.** Also implicated in startup warm p95 (~1.2s pre-arc at 200 memories vs accepted 12ms; startup has additional unexplained cost, likely strength hydration — same follow-up arc).
3. **`emit_recall_hits` cost ~12ms per included item** — one atomic seq-state file write per RecallHit event. Invisible before this arc because the legacy FTS bench phase included **zero** items; fused recall includes 15 because it actually finds things. Fixed by emit-batch (one seq reservation + one append for the batch): with-vector p95 235ms → **93.8ms (200) / 104.5ms (1000)** at 10 warm runs — under the §13 120ms cap.
4. Two earlier false leads, both caught by measurement: hydration envelope reads (fixed anyway for parity; wasn't the bottleneck) and machine-load contamination (two bench failures were load artifacts; everything above was re-measured on a quiet box).
5. stream-a bench: cold_reindex +14% attributed to a no-op supersession resync pass inside the timed reindex (fixed); bench triple incoherence after the production-Qwen default (fixed, bench-binary-only). tree_validator +12% attributed as downstream fallout; re-check at the next quiet-box bench gate run.

**§13 status:** with-vector and five-entity phases pass their caps. `delta_no_match` sits at 59–62ms against a 60ms cap — coin-flip, entirely the validate tax. The budgets themselves were not renegotiated; the durable fix is the validate-tax follow-up arc.

## Process notes

- Delegate lane self-reports were unrecoverable (`alias: codex` collision returns a stale unrelated report). Diff review + orchestrator-run gates were the source of truth throughout — which CLAUDE.md already mandates. Consider per-run `--alias` flags in future runs.
- Two pipeline-exit-code traps (`cmd | tail` masking failures) — one inherited, one self-inflicted mid-run. `set -o pipefail` everywhere now.
- The stream-e recall bench should join some scheduled gate; two regressions (validate tax, emission cost) accumulated invisibly because nothing runs it.

## Pending at time of writing

1. **Trey decisions:** (a) arm-vs-tune-first on trap-rate; (b) the atomic `[bench-update]` re-arm commit (HOLD `79ea90d` + re-armed baseline + lockstep test edits) — prepared on request after (a); (c) push (~70 commits unpushed).
2. ~~Full `check.sh` result on final trunk~~ — **GREEN** (exit 0 verified with pipefail): workspace tests incl. the armed quality tripwire, two-clone convergence, durability, rustdoc, and the stream-a bench regression check all pass on `fa3744c`. The bench-fix lane resolved the cold_reindex trip; tree_validator no longer trips either.
3. Promote `bench/stream-e-recall-results.darwin-arm64.json.proposed` → unsuffixed (human commit) once Trey accepts the latency numbers; regenerate it from the final trunk first.
4. Follow-up arc: validate_delta_request tax + startup-warm attribution (+ tree_validator re-check).
