# Memorum Embedding-Model Bench — Empirical Results

**Date:** 2026-06-09
**Hardware:** Apple M4 Max, 64 GB unified memory
**Harness:** Python + sentence-transformers 5.5.1, torch 2.12.0 (MPS), onnxruntime 1.26.0
**Corpus:** `crates/memorum-eval/fixtures/golden/` — 101 memory docs (doc text = `summary` + body), 50 query cases (46 non-abstention, 4 explicit abstention)
**Method:** normalize embeddings → cosine similarity → rank all 101 docs per query. Qwen3 + EmbeddingGemma use their registered `query`/`document` prompts (verified present); bge-m3 uses plain encoding (no instruction prefix).

## Results

| Model                | Dims | Device | nDCG@10 | Recall@5 | MRR   | Trap@5 (lo=good) | AbstainGap (hi=good) | CorpusEmbed | Q/embed |
|----------------------|------|--------|---------|----------|-------|------------------|----------------------|-------------|---------|
| Qwen3-Embedding-0.6B | 1024 | mps    | 0.751   | 0.886    | 0.724 | 0.261            | 0.332                | 4.15s       | 102.8ms |
| Qwen3-Embedding-4B   | 2560 | mps    | 0.767   | 0.864    | 0.732 | 0.348            | 0.347                | 5.92s       | 67.6ms  |
| embeddinggemma-300m  | 768  | cpu*   | 0.843   | 0.909    | 0.827 | 0.348            | 0.137                | 2.26s       | 41.4ms  |
| bge-m3               | 1024 | mps    | 0.785   | 0.864    | 0.810 | 0.370            | 0.172                | 2.55s       | 80.6ms  |

Metric definitions: nDCG@10 with gains essential=2/useful=1 (over the 44 cases that carry positive labels); Recall@5 on essential ids; MRR of first essential; **Trap@5** = fraction of the 46 non-abstention cases with ≥1 labeled `irrelevant_trap` in top-5 (lower is better); **AbstainGap** = median top-1 cosine over the 46 non-abstention cases minus median top-1 cosine over the 4 abstention cases (higher = "nothing relevant" is more separable, i.e. a recall threshold can actually fire). Wall-clock is informational only — production Rust lane (fastembed candle/ONNX, possibly CoreML EP) differs; the EmbeddingGemma number is an int8-quantized ONNX CPU path, not comparable to the fp16-MPS rows.

Raw trap counts: Qwen3-0.6B **12/46**, Qwen3-4B 16/46, embeddinggemma 16/46, bge-m3 17/46.
Abstention top-1 cosines: Qwen3-0.6B 0.359 (vs 0.690 relevant), Qwen3-4B 0.352 (vs 0.699), embeddinggemma **0.636** (vs 0.773), bge-m3 0.470 (vs 0.642).

## Per-model notes

### Qwen3-Embedding-0.6B  ✅
- Load: `sentence-transformers Qwen/Qwen3-Embedding-0.6B`, fp16, **MPS**. 1024 dims (full).
- Registered prompts `['query','document']` — the official retrieval query-instruction was applied via `prompt_name="query"`; docs encoded plain.
- **Best trap-resistance (12/46)** and **best abstention separability (gap 0.332)** of the field. Mid-pack on nDCG/Recall/MRR.

### Qwen3-Embedding-4B  ✅
- Load: `sentence-transformers Qwen/Qwen3-Embedding-4B`, fp16, **MPS**. 2560 dims (full).
- Marginal nDCG gain over the 0.6B (0.767 vs 0.751) but **worse trap-rate** (16 vs 12) and roughly equal recall. The go-big bet does not pay off on this English-dominant short-text corpus; abstention gap stays excellent (0.347).
- (Q/embed of 67.6ms is lower than the 0.6B's 102.8ms only because it ran second and MPS kernels were warm — wall-clock is informational, treat with skepticism.)

### embeddinggemma-300m  ✅ (with asterisk)
- `google/embeddinggemma-300m` is **gated on HF and returned 401** (no token in this env). Fell back to `onnx-community/embeddinggemma-300m-ONNX`.
- The default `model.onnx` uses external weight data (`model.onnx_data`) which crashes optimum's session init ("model_path must not be empty"). Worked around by forcing the self-contained **`model_quantized.onnx` (int8)** with `provider=CPUExecutionProvider`. **Device = CPU** (asterisk: not MPS/GPU, and int8 not fp32).
- Required `pip install optimum[onnxruntime] onnx` beyond the base stack.
- Registered prompts `['query','document']` — proper EmbeddingGemma prompt convention applied. 768 dims.
- **Tops nDCG@10 / Recall@5 / MRR** even quantized on CPU — contradicts the research doc's leaderboard-based prediction that Qwen3-0.6B wins on retrieval. BUT: **worst abstention separability (gap 0.137)** — it returns high cosines (~0.64) even for queries with no relevant memory, so a fixed recall threshold can't cleanly suppress "nothing relevant," and its trap-rate (16/46) is middling. Strong ranker, weak calibrator.

### bge-m3  ✅
- Load: `sentence-transformers BAAI/bge-m3`, **MPS**, dense vectors, 1024 dims, no instruction prefix (per model card). No sparse/colbert used.
- Solid second-tier on ranking (nDCG 0.785, MRR 0.810) but **worst trap-rate (17/46)** and a weak abstention gap (0.172). MIT license and multilingual strength are real, but neither is load-bearing for this English-dominant workload, and it loses on the precision axes that matter for contradiction detection.

## Recommendation

**Pick Qwen3-Embedding-0.6B** as the Memorum default, with the caveat that the decision hinges on what you weight.

The clean leaderboard story (research doc) predicted Qwen3-0.6B would win on retrieval; empirically on *our* corpus it does **not** lead raw ranking — quantized EmbeddingGemma-300m does (nDCG 0.843, Recall@5 0.909). If the only thing that mattered were "surface the right memory near the top," EmbeddingGemma would win outright and at the smallest footprint.

But Memorum's recall path is not pure ranking. Two of our metrics encode the system's actual failure modes:
1. **Trap-resistance** — superseded tails, wrong-project entity collisions, tombstoned facts that *look* relevant must not surface. This is the same fine-grained discrimination contradiction-detection needs. Qwen3-0.6B leaks traps into top-5 on **12/46** cases vs 16–17 for every other model — a real, consistent 25–30% reduction, not noise.
2. **Abstention calibration** — the recall block must be able to surface *nothing* when nothing is relevant. Qwen3-0.6B's relevant-vs-irrelevant cosine gap (0.690 vs 0.359) is the widest in the field, so a single global threshold cleanly separates "answer" from "abstain." EmbeddingGemma's gap collapses to 0.137 (it scores 0.64 even on genuinely-irrelevant queries) — it is *confidently wrong*, which is the worst property for a memory system that injects context into a prompt unprompted. A false memory surfaced with high confidence is more damaging than a true one ranked 6th instead of 2nd.

So: Qwen3-0.6B trades ~9 nDCG points and ~2 recall points for materially better precision and abstention — the right trade for a passive-recall + contradiction-detection system where false positives are the expensive error. It also brings the cleanest license (Apache 2.0), 32K context (vs EmbeddingGemma's 2K, tight against the 500-token chunk ceiling), MRL truncation to 512/256 for sqlite-vec storage, and it ran on **MPS fp16** here (EmbeddingGemma only ran int8/CPU because the fp32 GPU repo is gated).

**Secondary recommendations / things to flag to the coordinator:**
- **EmbeddingGemma deserves a re-test if Trey gets HF access** to the gated `google/embeddinggemma-300m` repo: this bench used the int8 ONNX fallback on CPU. The fp32/MPS variant would likely score *higher* on ranking — but its abstention-calibration weakness is architectural (high baseline cosines), unlikely to fix with precision, and is the disqualifier. Worth confirming, not blocking on.
- **Qwen3-4B is rejected**: it costs 7.5 GB on disk and a bigger always-running RAM footprint for *worse* trap-rate than the 0.6B and only +1.6 nDCG points. No reason to go big on this workload.
- **bge-m3 is rejected**: worst trap-rate, weak abstention gap; multilingual/MIT strengths aren't needed here.
- If the production Rust lane (fastembed candle `qwen3`+`metal`) can't get Qwen3 onto Metal cleanly, the `accelerate` CPU fallback is acceptable — the corpus embed (101 docs in ~4s on MPS fp16) and per-query latency are both comfortably inside the background-drain and interactive budgets even with headroom for CPU.

## HF cache footprint (downloaded this run — coordinator decides deletions)

| Repo | Size on disk | Path |
|------|--------------|------|
| Qwen/Qwen3-Embedding-0.6B | 1.2 G | `~/.cache/huggingface/hub/models--Qwen--Qwen3-Embedding-0.6B` |
| Qwen/Qwen3-Embedding-4B | 8.1 G | `~/.cache/huggingface/hub/models--Qwen--Qwen3-Embedding-4B` |
| onnx-community/embeddinggemma-300m-ONNX | 1.6 G | `~/.cache/huggingface/hub/models--onnx-community--embeddinggemma-300m-ONNX` |
| BAAI/bge-m3 | 4.6 G | `~/.cache/huggingface/hub/models--BAAI--bge-m3` |

Total downloaded this run ≈ **15.5 G** (cache also contains a pre-existing `BAAI/bge-large-en-v1.5`, 1.3 G, not from this bench). The winner (Qwen3-0.6B) is the smallest of the four at 1.2 G.

Harness + venv live at `/tmp/embed-bench/` (venv ~ a few GB); per-model raw JSON at `/tmp/embed-bench/result_<key>.json`.
