"""Run one embedding model over the golden corpus and emit JSON metrics.

Usage: run_model.py <model_key>
model_key in: qwen3-0.6b, qwen3-4b, embeddinggemma, bge-m3
Each model runs in its own process so an OOM/load failure is isolated.
"""
import sys, os, json, time, traceback
import numpy as np

import common

MODELS = {
    "qwen3-0.6b": {
        "repo": "Qwen/Qwen3-Embedding-0.6B",
        "kind": "qwen3",
    },
    "qwen3-4b": {
        "repo": "Qwen/Qwen3-Embedding-4B",
        "kind": "qwen3",
    },
    "embeddinggemma": {
        # try gated first, then onnx-community fallback handled in loader
        "repo": "google/embeddinggemma-300m",
        "onnx_repo": "onnx-community/embeddinggemma-300m-ONNX",
        "kind": "gemma",
    },
    "bge-m3": {
        "repo": "BAAI/bge-m3",
        "kind": "bge",
    },
}

# Qwen3 official query instruction (retrieval task).
QWEN3_QUERY_INSTRUCT = (
    "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
)


def normalize(a):
    a = np.asarray(a, dtype=np.float32)
    n = np.linalg.norm(a, axis=-1, keepdims=True)
    n[n == 0] = 1.0
    return a / n


def load_st(repo, device, **kw):
    from sentence_transformers import SentenceTransformer
    return SentenceTransformer(repo, device=device, **kw)


def encode_docs_queries(model_key, spec):
    """Returns dict with doc_emb, query_emb_by_case, meta."""
    import torch
    ids, docs = common.load_corpus()
    cases = common.load_queries()
    meta = {"model_key": model_key, "repo": spec["repo"], "device": None,
            "dims": None, "load_path": None, "notes": []}

    device = "mps" if torch.backends.mps.is_available() else "cpu"
    kind = spec["kind"]

    # ---- load ----
    model = None
    if kind == "qwen3":
        try:
            model = load_st(spec["repo"], device,
                            model_kwargs={"torch_dtype": torch.float16})
            meta["load_path"] = f"sentence-transformers {spec['repo']} (fp16, {device})"
        except Exception as e:
            meta["notes"].append(f"mps fp16 load failed: {e}; retry cpu fp32")
            device = "cpu"
            model = load_st(spec["repo"], device)
            meta["load_path"] = f"sentence-transformers {spec['repo']} (fp32, cpu)"
    elif kind == "bge":
        model = load_st(spec["repo"], device)
        meta["load_path"] = f"sentence-transformers {spec['repo']} ({device})"
    elif kind == "gemma":
        # try gated safetensors first, then onnx-community via onnx backend
        try:
            model = load_st(spec["repo"], device)
            meta["load_path"] = f"sentence-transformers {spec['repo']} ({device})"
        except Exception as e:
            meta["notes"].append(f"gated repo load failed: {repr(e)[:200]}")
            last = None
            for fn in ("model_quantized.onnx", "model_fp16.onnx", "model.onnx"):
                try:
                    model = load_st(
                        spec["onnx_repo"], "cpu", backend="onnx",
                        model_kwargs={
                            "file_name": fn,
                            "provider": "CPUExecutionProvider",
                        },
                    )
                    device = "cpu"
                    meta["repo"] = spec["onnx_repo"]
                    meta["load_path"] = (
                        f"sentence-transformers {spec['onnx_repo']} backend=onnx "
                        f"file={fn} provider=CPU"
                    )
                    meta["notes"].append(f"onnx fallback loaded with {fn}")
                    break
                except Exception as e2:
                    last = e2
                    meta["notes"].append(f"onnx {fn} failed: {repr(e2)[:160]}")
                    model = None
            if model is None:
                raise RuntimeError(f"gated failed AND all onnx fallbacks failed: {repr(last)[:300]}") from last
    meta["device"] = device

    # ---- doc embedding ----
    t0 = time.time()
    if kind == "qwen3":
        doc_emb = model.encode(docs, batch_size=16, convert_to_numpy=True,
                               normalize_embeddings=False, show_progress_bar=False)
    elif kind == "gemma":
        # EmbeddingGemma uses task prompts; documents use the "document" prompt if registered.
        try:
            doc_emb = model.encode(docs, prompt_name="document", batch_size=16,
                                   convert_to_numpy=True, normalize_embeddings=False,
                                   show_progress_bar=False)
            meta["notes"].append("gemma docs used prompt_name=document")
        except Exception:
            doc_emb = model.encode(docs, batch_size=16, convert_to_numpy=True,
                                   normalize_embeddings=False, show_progress_bar=False)
            meta["notes"].append("gemma docs used plain encode (no document prompt)")
    else:  # bge
        doc_emb = model.encode(docs, batch_size=16, convert_to_numpy=True,
                               normalize_embeddings=False, show_progress_bar=False)
    doc_embed_secs = time.time() - t0
    doc_emb = normalize(doc_emb)
    meta["dims"] = int(doc_emb.shape[1])

    # ---- query embedding ----
    query_emb_by_case = {}
    per_query_times = []
    for case in cases:
        qtext = case["query"]
        t1 = time.time()
        if kind == "qwen3":
            # ST registers a "query" prompt for Qwen3; use it, else manual instruct.
            try:
                emb = model.encode([qtext], prompt_name="query", convert_to_numpy=True,
                                   normalize_embeddings=False, show_progress_bar=False)
            except Exception:
                emb = model.encode([QWEN3_QUERY_INSTRUCT + " " + qtext],
                                   convert_to_numpy=True, normalize_embeddings=False,
                                   show_progress_bar=False)
        elif kind == "gemma":
            try:
                emb = model.encode([qtext], prompt_name="query", convert_to_numpy=True,
                                   normalize_embeddings=False, show_progress_bar=False)
            except Exception:
                emb = model.encode([qtext], convert_to_numpy=True,
                                   normalize_embeddings=False, show_progress_bar=False)
        else:  # bge: no instruction prefixes
            emb = model.encode([qtext], convert_to_numpy=True,
                               normalize_embeddings=False, show_progress_bar=False)
        per_query_times.append(time.time() - t1)
        query_emb_by_case[case["id"]] = normalize(emb)[0]

    # record whether qwen3/gemma "query" prompt actually existed
    if kind in ("qwen3", "gemma"):
        prompts = getattr(model, "prompts", {}) or {}
        meta["registered_prompts"] = list(prompts.keys())

    meta["doc_embed_secs"] = doc_embed_secs
    meta["mean_query_embed_ms"] = float(np.mean(per_query_times) * 1000)
    return ids, doc_emb, query_emb_by_case, cases, meta


def main():
    model_key = sys.argv[1]
    spec = MODELS[model_key]
    out = {"model_key": model_key}
    try:
        ids, doc_emb, qemb, cases, meta = encode_docs_queries(model_key, spec)
        metrics, _per = common.evaluate(ids, doc_emb, qemb, cases)
        out["status"] = "ok"
        out["meta"] = meta
        out["metrics"] = metrics
    except Exception as e:
        out["status"] = "fail"
        out["error"] = repr(e)
        out["traceback"] = traceback.format_exc()
    res_path = f"/tmp/embed-bench/result_{model_key}.json"
    with open(res_path, "w") as f:
        json.dump(out, f, indent=2)
    print(json.dumps(out.get("metrics", out), indent=2))
    print("STATUS:", out["status"], "->", res_path)


if __name__ == "__main__":
    main()
