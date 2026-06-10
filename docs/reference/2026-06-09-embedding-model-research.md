# Local Embedding Model Research for Memorum
**Date:** June 9, 2026  
**Hardware target:** Apple M4 Max MacBook Pro, 16 cores, 64 GB unified memory  
**Integration target:** Rust (fastembed crate =5.16.0), local background daemon, sqlite-vec index  
**Workload:** Short markdown chunks (50–500 tokens), semantic similarity + contradiction detection, English-dominant

---

## Candidates Table

| Model | Params | Dims (+ MRL) | Ctx len | MTEB-R / MMTEB-R (2025–2026) | License | Formats available | Rust lane in fastembed =5.16.0 | Est. RAM (fp32 / int8) | Est. latency on M4 Max |
|---|---|---|---|---|---|---|---|---|---|
| **google/embeddinggemma-300m** | 308M (100M model + 200M embed) | 768 (MRL: 128–768) | 2K | MTEB-Multilingual v2: **61.15** (full), 60.93 (Q8); MTEB-English v2: **69.67** (full), 69.49 (Q8) | Gemma (open weights, commercial use OK) | ONNX (ungated on HF: `onnx-community/embeddinggemma-300m-ONNX`); QAT GGUF (Q4_0, Q8_0, Mixed — all on Google HF repos, **gated**) | `EmbeddingModel::EmbeddingGemma300M` — ONNX/ort, **default feature set**, no extra flags | ~600 MB fp32 ONNX / <200 MB quantized (Google dev blog confirms <200MB RAM with quant) | Warm ONNX CPU: ~15–30ms/chunk (short text). CoreML EP via ort `coreml` feature: potentially <10ms. Benchmark (ghatdev, M1 Max Metal/Candle Q8): 33.5 emb/sec (~30ms/item single, throughput much higher in batch) |
| **Qwen/Qwen3-Embedding-0.6B** | 0.6B | 1024 (MRL: 32–1024) | 32K | MTEB-R (Eng v2): **61.82**; MMTEB-R: **64.64**; CMTEB-R: 71.02 | Apache 2.0 | HF safetensors; GGUF (Q8_0 ~639 MB, F16 ~1.2 GB — official Qwen HF, **ungated**); ONNX INT8 573 MB (community: n24q02m/Qwen3-Embedding-0.6B-ONNX); CoreML ANE-optimized (neuradex/Qwen3-Embedding-0.6B-CoreML-ANE) | `Qwen3TextEmbedding::from_hf(...)` — requires `qwen3` candle feature; `metal` feature enables candle Metal GPU offload | ~1.2 GB fp16 / ~639 MB Q8_0 GGUF / ~573 MB ONNX INT8 | MLX M2 Max: ~1–3ms single, 44K tok/s batch (jakedahn). CoreML ANE M1 Pro: 25ms @ seq=128, 140ms @ seq=512. Candle Metal CPU (fastembed official docs default `Device::Cpu`): likely 20–60ms; Metal GPU via `Device::new_metal(0)` expected 5–15ms |
| **Qwen/Qwen3-Embedding-4B** | 4B | 2560 (MRL: 32–2560) | 32K | MMTEB: **69.45**; MTEB-R (Eng v2): ~72 (paper table 3 shows 74.60 on CMTEB-R) | Apache 2.0 | HF safetensors; GGUF (official Qwen HF, ungated); ONNX (community) | `Qwen3TextEmbedding::from_hf("Qwen/Qwen3-Embedding-4B", ...)` — `qwen3` candle + `metal` | ~8 GB fp16 / ~4 GB Q8 GGUF | MLX M2 Max: ~18K tok/s. Candle Metal on M4 Max: est. 30–80ms/chunk. Fits within 8 GB RAM budget |
| **Qwen/Qwen3-Embedding-8B** | 8B | 4096 (MRL: 32–4096) | 32K | MMTEB: **70.58** (#1 as of May 2025, still #1–2 June 2026); MTEB-R (Eng v2): ~73.84 (CMTEB-R) | Apache 2.0 | HF safetensors; GGUF (official Qwen HF, ungated — Q8_0, F16); ONNX (community) | `Qwen3TextEmbedding::from_hf("Qwen/Qwen3-Embedding-8B", ...)` — `qwen3` candle + `metal` | ~16 GB fp16 / ~8 GB Q8 GGUF (exceeds 8 GB RAM budget in fp16; Q8 hits limit) | MLX M2 Max: ~11K tok/s. Candle Metal M4 Max: est. 80–200ms/chunk. **Tight on RAM budget** at Q8 |
| **BAAI/bge-m3** | 568M | 1024 (no MRL) | 8192 | BEIR (MTEB-R subset): 48.8; MIRACL: 69.2; real-world bilingual recall benchmark (memsearch, 2025): R@5 0.815 en / 0.783 zh (fp32), 0.814/0.776 (ONNX int8) | MIT | ONNX fp32 2.2 GB, ONNX int8 558 MB (gpahal/bge-m3-onnx-int8); fastembed built-in `BGEM3Q` quantized | `EmbeddingModel::BGEM3` (fp32) or `BGEM3Q` (quantized) — ONNX/ort, default features; **Note: BGEM3Q CPU-only, GPU ONNX path needs custom export** | ~2 GB fp32 / ~558 MB int8 | Measured on M4 Max MPS fp32: ~55ms for 8-text batch, ~145 texts/sec (FUYOH666/Services-BGE, 2026); single short text ~7ms |
| **Snowflake/snowflake-arctic-embed-l-v2.0** | 568M (303M non-embed) | 1024 (MRL: 256 viable, -0.18%) | 8192 | BEIR: **55.6**; MIRACL: 55.8; CLEF (multilingual): 52.9 | Apache 2.0 | ONNX (available via fastembed); HF safetensors | `EmbeddingModel::SnowflakeArcticEmbedL` + `SnowflakeArcticEmbedLQ` — ONNX/ort, default features | ~2.2 GB fp32 / ~600 MB quantized | Similar to BGE-M3 at same param count; est. ONNX CPU 20–50ms single short text on M4 Max |
| **nomic-ai/nomic-embed-text-v2-moe** | 475M total / 305M active | 768 (MRL: 256–768) | 512 | BEIR: 52.86; MIRACL: 65.80 | Apache 2.0 | HF safetensors (no ONNX export — MoE dynamic routing prevents it) | `NomicV2MoeTextEmbedding::from_hf(...)` — `nomic-v2-moe` candle + `metal` feature | ~1.5 GB fp32 active; ~900 MB est. with lower precision | Candle Metal: est. 15–40ms single. **512 token context limit** |

---

## Key Findings on Rust Integration

### fastembed-rs =5.16.0 model support (verified from crates.io/docs.rs)

The crate has two distinct backend paths as of v5.16.0:

**1. ONNX/ort backend (default features — what the plan currently uses)**
- Models: all BGE variants, EmbeddingGemma300M, Snowflake Arctic variants, MiniLM, GTE, MxBAI, Jina v2, nomic v1/v1.5, multilingual-e5
- Acceleration on macOS: CPU by default; CoreML EP is available through the `ort` crate's `coreml` feature (not a fastembed top-level feature, must add `ort = { features = ["coreml"] }` alongside fastembed)
- `EmbeddingGemma300M` enum variant maps to `onnx-community/embeddinggemma-300m-ONNX` — this is the same ungated repo the plan already specifies

**2. candle backend (behind feature flags)**
- `qwen3` feature: Qwen3-Embedding-0.6B/4B/8B, Qwen3-VL-Embedding-2B
- `nomic-v2-moe` feature: nomic-embed-text-v2-moe
- The `metal` feature (fastembed top-level) enables `candle-core/metal` + `candle-nn/metal` — this enables Metal GPU offload for the candle backend models
- **However:** the official fastembed-rs docs show `Device::Cpu` in all examples for Qwen3. The `metal` feature wire-up was added in commit `6bc168f` (Jan 11, 2026) alongside Qwen3 support. In practice, users must construct `Device::new_metal(0)` themselves and pass it to `Qwen3TextEmbedding::from_hf(repo, &device, ...)` — the API exposes the device parameter, so Metal is usable but not the default path shown in docs.
- The `qwen3` candle integration is a **different API surface** than the main `TextEmbedding`/`InitOptions` API — callers use `Qwen3TextEmbedding` struct directly, not `EmbeddingModel` enum variants
- **Important:** This means switching to Qwen3 requires code changes beyond a config triple swap — the embedding provider module in Task 3.0 will need a branch for the candle API path vs the ort API path

### ORT CoreML EP for EmbeddingGemma300M
FastEmbed uses `@pykeio/ort` v2. The ort crate v2 does support a CoreML execution provider on Apple Silicon (listed in ghatdev benchmark as `ort = { version = "2", features = ["coreml"] }`). This would give EmbeddingGemma300M near-ANE-speed without switching backends — keeping the existing `TextEmbedding`/`EmbeddingModel` API unchanged. The CoreML ANE benchmark for Qwen3-0.6B (M1 Pro) shows 25ms @ seq=128; EmbeddingGemma at 2K context would likely be faster given smaller model.

### llama.cpp / GGUF lane
Qwen3-Embedding GGUF support was merged to llama.cpp on 2025-08-02 (PR #15023). Official Qwen GGUFs for 0.6B/4B/8B are on HF (Qwen/Qwen3-Embedding-{size}-GGUF), ungated. The `llama-cpp-2` Rust crate (noted as runner-up in the plan) would require clang + cmake, as the plan already calls out. The llama.cpp embedding server subprocess lane is also viable but adds process management complexity.

---

## Recommendation

### Primary pick: Qwen3-Embedding-0.6B via fastembed `qwen3` + `metal` features

**Quality:** MTEB-R 61.82 (Eng retrieval), MMTEB-R 64.64, MRL 32–1024 dims, 32K context. This beats EmbeddingGemma300M on retrieval (61.82 vs ~58 MTEB-R for EmbeddingGemma; EmbeddingGemma's strong 69.67 is its MTEB-English v2 aggregate, not retrieval subset), and sits well above BGE-M3 on the English retrieval subset. The quality gap matters for contradiction detection, which needs fine-grained semantic discrimination.

**Footprint:** ~573 MB ONNX INT8 or ~639 MB Q8_0 GGUF; fp32 ~1.2 GB. Well within the 8 GB budget. On M4 Max unified memory this is trivial.

**Latency:** With Metal GPU (candle `metal` feature, `Device::new_metal(0)`): expected 5–15ms single short chunk based on M2 Max benchmarks and M4 Max's ~40% faster GPU. For the background drain loop (100ms+ budget) this is very comfortable. For interactive query embedding (<100ms budget), even CPU-only candle should pass.

**Rust integration:** In fastembed =5.16.0, `Qwen3TextEmbedding` is the API surface. It uses candle under the hood, which requires adding `features = ["qwen3", "metal"]` (or `"qwen3", "accelerate"` for CPU-accelerated Apple BLAS path as a CPU-only fallback). The device is user-specified. This is a **different struct** from `TextEmbedding` — Task 3.0's provider module needs to abstract over both, which it already does via the trait pattern.

**License:** Apache 2.0 — cleanest possible, no Gemma-style restrictions.

**MRL + asymmetric retrieval:** Yes on both. Instruction-aware (query vs document prompts). Matryoshka allows trading storage for quality, relevant for sqlite-vec.

**Triple string:** `("fastembed-candle", "Qwen/Qwen3-Embedding-0.6B", 1024)` — or truncated to 512 for storage savings with <2% quality loss per MRL behavior.

**Does this change the current plan?** Yes, materially:
- The plan locks `EmbeddingGemma300M` via the ONNX `EmbeddingModel` enum — a 768-dim fp32/ONNX model
- Qwen3-0.6B uses the `Qwen3TextEmbedding` candle API, requires `features = ["qwen3", "metal"]`, and is 1024-dim (MRL-truncatable)
- The embedding triple string changes: provider `"fastembed-onnx"` → `"fastembed-candle"`, model_ref changes, dimension changes 768 → 1024 (or truncated)
- This is contract-touching per invariant 3 — requires the stream-a spec amendment the plan already flagged as pending Trey's sign-off
- The `Cargo.toml` feature set changes: `default-features = false, features = ["qwen3", "metal", "hf-hub-native-tls", "ort-download-binaries-native-tls"]` (keep ort for the other ONNX models if they're still used; metal for Qwen3 candle path)

**Caveat on Metal path:** The fastembed docs show `Device::Cpu` in all Qwen3 examples. Metal is feature-wired but the `Qwen3TextEmbedding::from_hf` API takes a `&Device`, so `Device::new_metal(0)` should work if candle Metal is enabled. This hasn't been verified against fastembed-rs specifically — worth a quick integration smoke test. The `accelerate` feature (Apple BLAS) is a safe CPU fallback that should give 2–3x over plain CPU.

---

### Runner-up: google/embeddinggemma-300m (the current plan's pick)

**Why it's still strong:** MTEB-English v2 69.67 aggregate score is exceptional for a 308M sub-500M model — ranked #1 in that size class at time of release (Sep 2025). Under 200 MB quantized. The ONNX variant (`onnx-community/embeddinggemma-300m-ONNX`) is ungated, already in the plan, directly supported as `EmbeddingModel::EmbeddingGemma300M` in fastembed with zero extra features. No code change from the current plan. CoreML EP via ort would push it to near-ANE latency. Gemma license is acceptable for local OSS use.

**Why it's the runner-up, not the primary:** The 2K context window is tight for the 500-token chunk ceiling. On pure retrieval (MTEB-R subset), EmbeddingGemma's score (~58–62 range based on multilingual table; the 69.67 is aggregate including classification/clustering/STS) is comparable to Qwen3-0.6B's 61.82 retrieval-specific score — but Qwen3-0.6B has a longer context window (32K vs 2K), MRL down to 32 dims, and a cleaner Apache 2.0 license. The primary pick wins on retrieval score precision, context window, and license; the integration cost is one extra feature flag and a different struct.

**If staying with EmbeddingGemma300M:** keep `EmbeddingModel::EmbeddingGemma300M`, add CoreML EP for latency, and the triple is `("fastembed-onnx", "embeddinggemma-300m-onnx", 768)` as the plan already provisionally adopts. The spec amendment still needs to happen regardless.

---

## Rejected / Not Recommended

- **Qwen3-Embedding-4B**: 8 GB fp16 RAM is at the ceiling; the quality jump over 0.6B is modest for English-dominant short-text retrieval. Reserve for a future "high-quality tier" if needed.
- **Qwen3-Embedding-8B**: 16 GB fp16 exceeds budget. Even Q8 (~8 GB) is tight for an always-running daemon. Not recommended as default.
- **BAAI/bge-m3**: BEIR 48.8 is weak for an English-dominant workload (Snowflake arctic-l-v2 at 55.6 on same benchmark). Strong multilingual and hybrid sparse+dense is valuable but not required here. The ~2 GB fp32 footprint and MIT license are fine, but it underperforms both EmbeddingGemma and Qwen3-0.6B on English retrieval.
- **snowflake-arctic-embed-l-v2.0**: BEIR 55.6 is strong English-only, but the ONNX path is a drop-in competitor to EmbeddingGemma. It loses to Qwen3-0.6B on MMTEB retrieval and has no MRL at smaller dims.
- **nomic-embed-text-v2-moe**: No ONNX export possible (MoE dynamic routing). Candle-only, 512 token context limit, BEIR 52.86 — mediocre. Not competitive.

---

## Embedding Triple Strings (for spec amendment)

| Pick | provider | model_ref | dimension |
|---|---|---|---|
| **Primary (Qwen3-0.6B)** | `fastembed-candle` | `Qwen/Qwen3-Embedding-0.6B` | `1024` |
| Primary truncated (storage savings) | `fastembed-candle` | `Qwen/Qwen3-Embedding-0.6B` | `512` |
| **Runner-up (EmbeddingGemma300M)** | `fastembed-onnx` | `embeddinggemma-300m-onnx` | `768` |
| Runner-up truncated | `fastembed-onnx` | `embeddinggemma-300m-onnx` | `256` |

---

## Sources

1. MTEB Leaderboard (live) — https://huggingface.co/spaces/mteb/leaderboard
2. Qwen3 Embedding technical report (arXiv:2506.05176, June 2025) — https://arxiv.org/pdf/2506.05176
3. Qwen3 Embedding HF model card (Qwen3-Embedding-8B) — https://huggingface.co/Qwen/Qwen3-Embedding-8B
4. Qwen3 Embedding HF GGUF page (0.6B) — https://huggingface.co/Qwen/Qwen3-Embedding-0.6B-GGUF
5. Qwen3 Embedding blog post (Qwen Team, June 5 2025) — https://qwenlm.github.io/blog/qwen3-embedding/
6. EmbeddingGemma HF model card (google/embeddinggemma-300m) — https://huggingface.co/google/embeddinggemma-300m
7. EmbeddingGemma paper (arXiv:2509.20354) — https://arxiv.org/pdf/2509.20354
8. EmbeddingGemma HF blog (Sep 4 2025) — https://huggingface.co/blog/embeddinggemma
9. Google Developers blog — EmbeddingGemma (Sep 4 2025) — https://developers.googleblog.com/introducing-embeddinggemma/
10. EmbeddingGemma Google AI overview — https://ai.google.dev/gemma/docs/embeddinggemma
11. fastembed-rs crates.io v5.16.0 — https://crates.io/crates/fastembed
12. fastembed-rs docs.rs (latest) — https://docs.rs/crate/fastembed/latest
13. fastembed EmbeddingModel enum docs — https://docs.rs/fastembed/latest/fastembed/enum.EmbeddingModel.html
14. fastembed-rs GitHub (Anush008) — https://github.com/anush008/fastembed-rs
15. fastembed-rs commit: Add Qwen3 Embedding support (Jan 11 2026) — https://github.com/Anush008/fastembed-rs/commit/6bc168fa8016543b9085fb21aeb7b90fb034189a
16. fastembed-rs commit: Add nomic-embed-text-v2-moe (Feb 19 2026) — https://github.com/Anush008/fastembed-rs/commit/ccffb98f62a78093f6b985230dc260dfa09f225b
17. llama.cpp PR #15023: Qwen3-Embedding GGUF conversion support (merged Aug 2 2025) — https://github.com/ggml-org/llama.cpp/pull/15023
18. Qwen3-Embedding-0.6B-GGUF discussion (HF) — https://huggingface.co/Qwen/Qwen3-Embedding-0.6B-GGUF/discussions/8
19. Snowflake Arctic Embed 2.0 HF (l-v2.0) — https://huggingface.co/Snowflake/snowflake-arctic-embed-l-v2.0
20. Arctic Embed 2.0 paper (arXiv:2412.04506v2) — https://arxiv.org/html/2412.04506v2
21. nomic-embed-text-v2-moe HF — https://huggingface.co/nomic-ai/nomic-embed-text-v2-moe
22. BGE-M3 HF — https://huggingface.co/BAAI/bge-m3
23. BGE-M3 Apple Silicon MPS service + M4 Max benchmark (FUYOH666/Services-BGE, 2026) — https://github.com/FUYOH666/Services-BGE
24. memsearch embedding evaluation (bilingual real-world benchmark) — https://zilliztech.github.io/memsearch/home/embedding-evaluation/
25. MTEB leaderboard rankings April 2026 — https://awesomeagents.ai/leaderboards/embedding-model-leaderboard-mteb-april-2026/
26. MTEB 2026 state overview — https://app.ailog.fr/en/blog/news/rag-benchmark-mteb-2026
27. CodeSOTA MTEB 2026 table (codesota.com) — https://www.codesota.com/benchmarks/mteb
28. Qwen3-Embedding-0.6B CoreML ANE benchmark (M1 Pro) — https://huggingface.co/neuradex/Qwen3-Embedding-0.6B-CoreML-ANE
29. MLX Qwen3 embedding server Apple Silicon benchmark (M2 Max, jakedahn) — https://github.com/jakedahn/qwen3-embeddings-mlx
30. Rust embedding benchmark: EmbeddingGemma vs Qwen3, Apple Silicon M1 Max Metal (ghatdev, Dec 2025) — https://github.com/ghatdev/embedding-benchmark
31. Candle Metal backend Apple Silicon (HF candle issue #313) — https://github.com/huggingface/candle/issues/313
32. qdrant/fastembed PR #605: Qwen3-Embedding-0.6B ONNX support — https://github.com/qdrant/fastembed/pull/605
