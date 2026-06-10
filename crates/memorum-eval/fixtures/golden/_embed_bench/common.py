"""Shared corpus loading + metrics for the Memorum embedding bench."""
import os, glob, yaml, math
import numpy as np

GOLDEN = "/Users/treygoff/Code/agent-memory/crates/memorum-eval/fixtures/golden"
MEM_DIR = os.path.join(GOLDEN, "memories")
QUERIES = os.path.join(GOLDEN, "queries.yaml")


def _split_frontmatter(text):
    # File starts with '---\n', frontmatter, then '---\n', then body.
    assert text.startswith("---"), "no frontmatter"
    parts = text.split("---", 2)
    # parts[0] = '' , parts[1] = frontmatter, parts[2] = body
    fm = yaml.safe_load(parts[1])
    body = parts[2].lstrip("\n")
    return fm, body


def load_corpus():
    """Returns (ids, docs) where docs[i] = summary + body for ids[i]."""
    ids, docs = [], []
    for path in sorted(glob.glob(os.path.join(MEM_DIR, "**", "*.md"), recursive=True)):
        with open(path) as f:
            text = f.read()
        fm, body = _split_frontmatter(text)
        mid = fm["id"]
        summary = fm.get("summary", "") or ""
        doc = (summary + "\n\n" + body).strip()
        ids.append(mid)
        docs.append(doc)
    assert len(ids) == len(set(ids)), "duplicate memory ids"
    return ids, docs


def load_queries():
    d = yaml.safe_load(open(QUERIES))
    return d["cases"]


def is_abstain(case):
    # The corpus's abstention-calibration set is the 4 explicitly-named qNN-abstain-*
    # cases (empty essential+useful AND no traps -> the correct answer is "nothing").
    # q45/q46 have empty essential+useful but carry traps: they are precision probes
    # for this embedding helper (no nDCG/recall, but trap-rate applies).
    return "abstain" in case["id"]


# ---- metrics ----

def ndcg_at_k(ranked_ids, gains, k=10):
    """ranked_ids: list of doc ids best-first. gains: {id: gain}."""
    dcg = 0.0
    for i, did in enumerate(ranked_ids[:k]):
        g = gains.get(did, 0)
        if g:
            dcg += g / math.log2(i + 2)
    ideal = sorted(gains.values(), reverse=True)
    idcg = 0.0
    for i, g in enumerate(ideal[:k]):
        if g:
            idcg += g / math.log2(i + 2)
    if idcg == 0:
        return 0.0
    return dcg / idcg


def recall_at_k(ranked_ids, essential, k=5):
    if not essential:
        return None
    ess = set(essential)
    hit = sum(1 for did in ranked_ids[:k] if did in ess)
    return hit / len(ess)


def mrr_first_essential(ranked_ids, essential):
    ess = set(essential)
    for i, did in enumerate(ranked_ids):
        if did in ess:
            return 1.0 / (i + 1)
    return 0.0


def trap_in_top_k(ranked_ids, traps, k=5):
    t = set(traps)
    return 1 if any(did in t for did in ranked_ids[:k]) else 0


def evaluate(corpus_ids, doc_emb, query_emb_by_case, cases):
    """doc_emb: (N,D) normalized. query_emb_by_case: {case_id: (D,) normalized}."""
    id_index = {mid: i for i, mid in enumerate(corpus_ids)}
    ndcgs, recalls, mrrs, traprates = [], [], [], []
    abstain_top1, nonabstain_top1 = [], []
    per_case = []
    for case in cases:
        cid = case["id"]
        q = query_emb_by_case[cid]
        sims = doc_emb @ q  # cosine since normalized
        order = np.argsort(-sims)
        ranked_ids = [corpus_ids[i] for i in order]
        top1 = float(sims[order[0]])
        g = case["graded"]
        ess = g.get("essential") or []
        use = g.get("useful") or []
        traps = g.get("irrelevant_traps") or []
        if is_abstain(case):
            abstain_top1.append(top1)
            per_case.append((cid, "abstain", top1))
            continue
        nonabstain_top1.append(top1)
        # trap-rate applies to every non-abstention case collected by this helper.
        tr = trap_in_top_k(ranked_ids, traps, 5)
        traprates.append(tr)
        has_positive = bool(ess or use)
        nd = rc = mr = None
        if has_positive:
            gains = {}
            for x in ess:
                gains[x] = 2
            for x in use:
                gains[x] = 1
            nd = ndcg_at_k(ranked_ids, gains, 10)
            mr = mrr_first_essential(ranked_ids, ess)
            ndcgs.append(nd)
            mrrs.append(mr)
            rc = recall_at_k(ranked_ids, ess, 5)
            if rc is not None:
                recalls.append(rc)
        per_case.append((cid, "graded", {"ndcg": nd, "recall5": rc, "mrr": mr, "trap": tr, "top1": top1}))
    out = {
        "n_nonabstain": len(traprates),
        "n_graded_positive": len(ndcgs),
        "ndcg10": float(np.mean(ndcgs)),
        "recall5_essential": float(np.mean(recalls)),
        "mrr_first_essential": float(np.mean(mrrs)),
        "trap_rate5": float(np.mean(traprates)),
        "n_abstain": len(abstain_top1),
        "median_top1_abstain": float(np.median(abstain_top1)) if abstain_top1 else None,
        "median_top1_nonabstain": float(np.median(nonabstain_top1)) if nonabstain_top1 else None,
    }
    if out["median_top1_abstain"] is not None:
        out["abstain_gap"] = out["median_top1_nonabstain"] - out["median_top1_abstain"]
    return out, per_case
