"""T4.1 golden-corpus bake-off: run one API embedding candidate over the golden corpus.

STAGED for Trey-approved manual runs ONLY (real API, real money, off-gate). Never runs in CI or check.sh.

Usage: run_api_model.py <candidate_key>
Candidates (plan v0.2, Model candidates table):
  gemini-768 | gemini-1536 | gemini-3072   (gemini-embedding-2, MRL truncation via output_dimensionality)
  voyage-lite-512 | voyage-lite-1024        (voyage-4-lite, input_type asymmetry)
  jina-small-1024                           (jina-embeddings-v5-text-small, retrieval adapters)

Env: MEMORUM_GEMINI_API_KEY / VOYAGE_API_KEY / JINA_API_KEY (only the one you're running).
Results -> /tmp/embed-bench/result_<key>.json, same shape as the local-model results so the
decision table in the plan can compare directly. Abstention thresholds are fit per candidate from
the abstain-vs-nonabstain top1 medians that common.evaluate already emits — never reused across
models/dims (plan: abstention-threshold discipline).

Gemini prefixes are imported conceptually from the shipped provider
(crates/memoryd/src/embedding/api_provider.rs GEMINI_QUERY_PREFIX / GEMINI_DOCUMENT_PREFIX) so the
bake-off measures exactly what the daemon will do. If you tune prefixes here, the provider consts
must change with them.
"""
import sys, os, json, time, traceback
import urllib.request

import numpy as np

import common

# MUST stay in sync with crates/memoryd/src/embedding/api_provider.rs
GEMINI_QUERY_PREFIX = "task: search result | query: "
GEMINI_DOCUMENT_PREFIX = "title: none | text: "
GEMINI_BATCH_MAX = 100  # mirrors GEMINI_BATCH_MAX_REQUESTS

CANDIDATES = {
    "gemini-768": {"kind": "gemini", "model": "gemini-embedding-2", "dims": 768},
    "gemini-1536": {"kind": "gemini", "model": "gemini-embedding-2", "dims": 1536},
    "gemini-3072": {"kind": "gemini", "model": "gemini-embedding-2", "dims": 3072},
    "voyage-lite-512": {"kind": "voyage", "model": "voyage-4-lite", "dims": 512},
    "voyage-lite-1024": {"kind": "voyage", "model": "voyage-4-lite", "dims": 1024},
    "jina-small-1024": {"kind": "jina", "model": "jina-embeddings-v5-text-small", "dims": 1024},
}


def _post_json(url, payload, headers):
    body = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json", **headers})
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                return json.load(resp)
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < 4:
                delay = int(e.headers.get("Retry-After") or 15)
                print(f"  429; sleeping {delay}s", file=sys.stderr)
                time.sleep(delay)
                continue
            raise RuntimeError(f"HTTP {e.code}: {e.read().decode()[:500]}") from e
    raise RuntimeError("unreachable")


# Per-vendor embed helpers; each returns list[list[float]]

def gemini_embed(texts, spec, is_query):
    key = os.environ["MEMORUM_GEMINI_API_KEY"]
    model = spec["model"]
    prefix = GEMINI_QUERY_PREFIX if is_query else GEMINI_DOCUMENT_PREFIX
    out = []
    for i in range(0, len(texts), GEMINI_BATCH_MAX):
        chunk = texts[i : i + GEMINI_BATCH_MAX]
        payload = {
            "requests": [
                {
                    "model": f"models/{model}",
                    "content": {"parts": [{"text": prefix + t}]},
                    "output_dimensionality": spec["dims"],
                }
                for t in chunk
            ]
        }
        resp = _post_json(
            f"https://generativelanguage.googleapis.com/v1beta/models/{model}:batchEmbedContents",
            payload,
            {"x-goog-api-key": key},
        )
        out.extend(e["values"] for e in resp["embeddings"])
    return out


def voyage_embed(texts, spec, is_query):
    key = os.environ["VOYAGE_API_KEY"]
    out = []
    for i in range(0, len(texts), 128):
        chunk = texts[i : i + 128]
        resp = _post_json(
            "https://api.voyageai.com/v1/embeddings",
            {
                "model": spec["model"],
                "input": chunk,
                "input_type": "query" if is_query else "document",
                "output_dimension": spec["dims"],
            },
            {"Authorization": f"Bearer {key}"},
        )
        out.extend(d["embedding"] for d in sorted(resp["data"], key=lambda d: d["index"]))
    return out


def jina_embed(texts, spec, is_query):
    key = os.environ["JINA_API_KEY"]
    out = []
    for i in range(0, len(texts), 128):
        chunk = texts[i : i + 128]
        resp = _post_json(
            "https://api.jina.ai/v1/embeddings",
            {
                "model": spec["model"],
                "input": chunk,
                "task": "retrieval.query" if is_query else "retrieval.passage",
                "dimensions": spec["dims"],
            },
            {"Authorization": f"Bearer {key}"},
        )
        out.extend(d["embedding"] for d in sorted(resp["data"], key=lambda d: d["index"]))
    return out


EMBEDDERS = {"gemini": gemini_embed, "voyage": voyage_embed, "jina": jina_embed}


def normalize(a):
    a = np.asarray(a, dtype=np.float32)
    n = np.linalg.norm(a, axis=-1, keepdims=True)
    n[n == 0] = 1.0
    return a / n


def main():
    key = sys.argv[1]
    spec = CANDIDATES[key]
    embed = EMBEDDERS[spec["kind"]]
    out = {"model_key": key}
    try:
        ids, docs = common.load_corpus()
        cases = common.load_queries()
        meta = {"model_key": key, "model": spec["model"], "dims": spec["dims"], "api": spec["kind"], "notes": []}

        t0 = time.time()
        doc_emb = normalize(embed(docs, spec, is_query=False))
        meta["doc_embed_secs"] = time.time() - t0
        assert doc_emb.shape[1] == spec["dims"], f"dims {doc_emb.shape[1]} != {spec['dims']}"

        qtimes, qemb = [], {}
        for case in cases:
            t1 = time.time()
            vec = normalize(embed([case["query"]], spec, is_query=True))[0]
            qtimes.append(time.time() - t1)
            qemb[case["id"]] = vec
        meta["mean_query_embed_ms"] = float(np.mean(qtimes) * 1000)

        metrics, _per = common.evaluate(ids, doc_emb, qemb, cases)
        out.update(status="ok", meta=meta, metrics=metrics)
    except Exception as e:
        out.update(status="fail", error=repr(e), traceback=traceback.format_exc())
    os.makedirs("/tmp/embed-bench", exist_ok=True)
    res_path = f"/tmp/embed-bench/result_{key}.json"
    with open(res_path, "w") as f:
        json.dump(out, f, indent=2)
    print(json.dumps(out.get("metrics", out), indent=2))
    print("STATUS:", out["status"], "->", res_path)


if __name__ == "__main__":
    main()
